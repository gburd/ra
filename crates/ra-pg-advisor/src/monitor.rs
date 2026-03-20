//! Query monitoring and candidate identification.
//!
//! Tracks query execution times, detects plan regressions, and
//! identifies queries that would benefit from plan advice.

#![allow(clippy::cast_precision_loss)]

use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// A query identified as a candidate for optimization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryCandidate {
    /// Unique identifier for the query (hash of normalized SQL).
    pub query_id: String,
    /// The SQL text.
    pub query_text: String,
    /// Average execution time in milliseconds.
    pub avg_duration_ms: f64,
    /// Number of times this query was observed.
    pub call_count: u64,
    /// Total cost from the planner.
    pub total_cost: f64,
    /// Why this query was flagged.
    pub reason: CandidateReason,
}

/// Why a query was identified as an optimization candidate.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
)]
pub enum CandidateReason {
    /// Exceeds the slow query threshold.
    SlowQuery,
    /// Plan cost increased compared to historical baseline.
    PlanRegression,
    /// High total time (frequent + moderately slow).
    HighTotalTime,
    /// Large cardinality misestimate detected.
    CardinalityMisestimate,
}

impl fmt::Display for CandidateReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlowQuery => write!(f, "slow query"),
            Self::PlanRegression => {
                write!(f, "plan regression")
            }
            Self::HighTotalTime => {
                write!(f, "high total time")
            }
            Self::CardinalityMisestimate => {
                write!(f, "cardinality misestimate")
            }
        }
    }
}

/// Observation of a single query execution.
#[derive(Debug, Clone)]
pub struct QueryObservation {
    /// Normalized query text.
    pub query_text: String,
    /// Execution duration in milliseconds.
    pub duration_ms: f64,
    /// Planner estimated cost.
    pub total_cost: f64,
    /// When this observation was recorded.
    pub observed_at: Instant,
}

/// Internal state for a tracked query.
struct TrackedQuery {
    query_text: String,
    durations: Vec<f64>,
    costs: Vec<f64>,
    #[allow(dead_code)]
    last_seen: Instant,
}

impl TrackedQuery {
    fn avg_duration(&self) -> f64 {
        if self.durations.is_empty() {
            return 0.0;
        }
        self.durations.iter().sum::<f64>()
            / self.durations.len() as f64
    }

    fn avg_cost(&self) -> f64 {
        if self.costs.is_empty() {
            return 0.0;
        }
        self.costs.iter().sum::<f64>()
            / self.costs.len() as f64
    }

    fn call_count(&self) -> u64 {
        self.durations.len() as u64
    }

    fn total_duration(&self) -> f64 {
        self.durations.iter().sum()
    }
}

/// Tracks queries over time and identifies optimization candidates.
pub struct QueryTracker {
    /// Duration threshold in ms above which a query is "slow".
    slow_threshold_ms: f64,
    /// Total time threshold in ms (calls * `avg_duration`).
    total_time_threshold_ms: f64,
    /// Cost regression ratio (e.g. 2.0 = cost doubled).
    regression_ratio: f64,
    /// Maximum history entries per query.
    max_history: usize,
    /// Tracked queries keyed by normalized query hash.
    queries: HashMap<u64, TrackedQuery>,
}

impl QueryTracker {
    /// Create a tracker with the given thresholds.
    #[must_use]
    pub fn new(slow_threshold_ms: f64) -> Self {
        Self {
            slow_threshold_ms,
            total_time_threshold_ms: slow_threshold_ms * 100.0,
            regression_ratio: 2.0,
            max_history: 100,
            queries: HashMap::new(),
        }
    }

    /// Create a tracker with custom thresholds.
    #[must_use]
    pub fn with_thresholds(
        slow_threshold_ms: f64,
        total_time_threshold_ms: f64,
        regression_ratio: f64,
    ) -> Self {
        Self {
            slow_threshold_ms,
            total_time_threshold_ms,
            regression_ratio,
            max_history: 100,
            queries: HashMap::new(),
        }
    }

    /// Record a query observation.
    pub fn observe(&mut self, obs: &QueryObservation) {
        let hash = query_hash(&obs.query_text);
        let entry = self.queries.entry(hash).or_insert_with(|| {
            TrackedQuery {
                query_text: obs.query_text.clone(),
                durations: Vec::new(),
                costs: Vec::new(),
                last_seen: obs.observed_at,
            }
        });

        entry.durations.push(obs.duration_ms);
        entry.costs.push(obs.total_cost);
        entry.last_seen = obs.observed_at;

        if entry.durations.len() > self.max_history {
            entry.durations.remove(0);
            entry.costs.remove(0);
        }
    }

    /// Identify queries that are candidates for optimization.
    #[must_use]
    pub fn candidates(&self) -> Vec<QueryCandidate> {
        let mut results = Vec::new();
        for tracked in self.queries.values() {
            if let Some(reason) = self.classify(tracked) {
                results.push(QueryCandidate {
                    query_id: format!(
                        "{:016x}",
                        query_hash(&tracked.query_text)
                    ),
                    query_text: tracked.query_text.clone(),
                    avg_duration_ms: tracked.avg_duration(),
                    call_count: tracked.call_count(),
                    total_cost: tracked.avg_cost(),
                    reason,
                });
            }
        }
        results.sort_by(|a, b| {
            b.avg_duration_ms
                .partial_cmp(&a.avg_duration_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }

    /// Number of distinct queries being tracked.
    #[must_use]
    pub fn tracked_count(&self) -> usize {
        self.queries.len()
    }

    /// Clear all tracked queries.
    pub fn clear(&mut self) {
        self.queries.clear();
    }

    fn classify(
        &self,
        tracked: &TrackedQuery,
    ) -> Option<CandidateReason> {
        if tracked.avg_duration() > self.slow_threshold_ms {
            return Some(CandidateReason::SlowQuery);
        }

        if self.detect_regression(tracked) {
            return Some(CandidateReason::PlanRegression);
        }

        if tracked.total_duration()
            > self.total_time_threshold_ms
        {
            return Some(CandidateReason::HighTotalTime);
        }

        None
    }

    fn detect_regression(&self, tracked: &TrackedQuery) -> bool {
        let costs = &tracked.costs;
        if costs.len() < 4 {
            return false;
        }

        let midpoint = costs.len() / 2;
        let old_avg: f64 = costs[..midpoint].iter().sum::<f64>()
            / midpoint as f64;
        let new_avg: f64 = costs[midpoint..].iter().sum::<f64>()
            / (costs.len() - midpoint) as f64;

        if old_avg > 0.0 {
            new_avg / old_avg > self.regression_ratio
        } else {
            false
        }
    }
}

/// Simple hash for query text.
fn query_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash
            .wrapping_mul(33)
            .wrapping_add(u64::from(byte));
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(
        query: &str,
        duration_ms: f64,
        cost: f64,
    ) -> QueryObservation {
        QueryObservation {
            query_text: query.to_string(),
            duration_ms,
            total_cost: cost,
            observed_at: Instant::now(),
        }
    }

    #[test]
    fn slow_query_detection() {
        let mut tracker = QueryTracker::new(100.0);
        tracker.observe(&obs("SELECT * FROM t", 200.0, 500.0));

        let candidates = tracker.candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].reason, CandidateReason::SlowQuery);
    }

    #[test]
    fn fast_queries_not_flagged() {
        let mut tracker = QueryTracker::new(100.0);
        tracker.observe(&obs("SELECT 1", 5.0, 1.0));

        let candidates = tracker.candidates();
        assert!(candidates.is_empty());
    }

    #[test]
    fn regression_detection() {
        let mut tracker = QueryTracker::new(1000.0);

        for _ in 0..4 {
            tracker.observe(&obs("SELECT * FROM big", 50.0, 100.0));
        }
        for _ in 0..4 {
            tracker.observe(&obs("SELECT * FROM big", 50.0, 500.0));
        }

        let candidates = tracker.candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].reason,
            CandidateReason::PlanRegression
        );
    }

    #[test]
    fn high_total_time() {
        let mut tracker =
            QueryTracker::with_thresholds(100.0, 5000.0, 2.0);

        for _ in 0..100 {
            tracker.observe(&obs("SELECT * FROM freq", 60.0, 10.0));
        }

        let candidates = tracker.candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].reason,
            CandidateReason::HighTotalTime
        );
    }

    #[test]
    fn tracked_count() {
        let mut tracker = QueryTracker::new(100.0);
        tracker.observe(&obs("q1", 10.0, 1.0));
        tracker.observe(&obs("q2", 10.0, 1.0));
        tracker.observe(&obs("q1", 20.0, 2.0));
        assert_eq!(tracker.tracked_count(), 2);
    }

    #[test]
    fn clear_tracker() {
        let mut tracker = QueryTracker::new(100.0);
        tracker.observe(&obs("q1", 200.0, 500.0));
        assert_eq!(tracker.tracked_count(), 1);
        tracker.clear();
        assert_eq!(tracker.tracked_count(), 0);
        assert!(tracker.candidates().is_empty());
    }

    #[test]
    fn candidates_sorted_by_duration() {
        let mut tracker = QueryTracker::new(100.0);
        tracker.observe(&obs("slow", 200.0, 500.0));
        tracker.observe(&obs("slower", 500.0, 1000.0));
        tracker.observe(&obs("slowest", 800.0, 2000.0));

        let candidates = tracker.candidates();
        assert_eq!(candidates.len(), 3);
        assert!(
            candidates[0].avg_duration_ms
                >= candidates[1].avg_duration_ms
        );
        assert!(
            candidates[1].avg_duration_ms
                >= candidates[2].avg_duration_ms
        );
    }

    #[test]
    fn candidate_reason_display() {
        assert_eq!(
            format!("{}", CandidateReason::SlowQuery),
            "slow query"
        );
        assert_eq!(
            format!("{}", CandidateReason::PlanRegression),
            "plan regression"
        );
    }

    #[test]
    fn max_history_enforced() {
        let mut tracker = QueryTracker::new(100.0);
        for i in 0..150 {
            tracker.observe(&obs(
                "q",
                f64::from(i),
                f64::from(i),
            ));
        }
        assert_eq!(tracker.tracked_count(), 1);
    }

    #[test]
    fn custom_thresholds() {
        let mut tracker =
            QueryTracker::with_thresholds(50.0, 500.0, 3.0);
        tracker.observe(&obs("q", 60.0, 100.0));
        let candidates = tracker.candidates();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].reason, CandidateReason::SlowQuery);
    }

    #[test]
    fn regression_needs_enough_history() {
        let mut tracker = QueryTracker::new(1000.0);
        tracker.observe(&obs("q", 50.0, 100.0));
        tracker.observe(&obs("q", 50.0, 500.0));
        assert!(tracker.candidates().is_empty());
    }
}
