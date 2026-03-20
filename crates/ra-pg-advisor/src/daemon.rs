//! Advisor daemon that monitors `PostgreSQL` and generates plan advice.
//!
//! The daemon connects to `PostgreSQL`, polls `pg_stat_statements` for
//! slow queries, runs them through the RA optimizer, and applies
//! the resulting advice.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

use crate::advice_gen::{AdviceGenerator, PlanAdvice};
use crate::monitor::{
    QueryCandidate, QueryObservation, QueryTracker,
};
use crate::pg_api::{AdviceBackend, PgAdviceApi};

/// Errors from the advisor daemon.
#[derive(Debug, Error)]
pub enum DaemonError {
    /// `PostgreSQL` connection failed.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// Query to `pg_stat_statements` failed.
    #[error("monitoring query failed: {0}")]
    MonitoringFailed(String),

    /// Advice application failed.
    #[error("advice application failed: {0}")]
    AdviceFailed(String),
}

/// Configuration for the advisor daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// `PostgreSQL` connection string.
    pub connection_string: String,
    /// Polling interval.
    pub poll_interval: Duration,
    /// Slow query threshold in milliseconds.
    pub slow_threshold_ms: f64,
    /// Preferred advice backend.
    pub backend: AdviceBackend,
    /// Minimum confidence to apply advice.
    pub min_confidence: f64,
    /// Maximum number of queries to advise per cycle.
    pub max_advice_per_cycle: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            connection_string: String::new(),
            poll_interval: Duration::from_secs(5),
            slow_threshold_ms: 100.0,
            backend: AdviceBackend::PgPlanAdviceGuc,
            min_confidence: 0.7,
            max_advice_per_cycle: 10,
        }
    }
}

/// Advisor daemon state.
///
/// This struct holds the monitoring state and generates advice.
/// It does not own a `PostgreSQL` connection; the caller provides
/// query observations (from `pg_stat_statements` or similar)
/// and retrieves the generated SQL to execute.
pub struct AdvisorDaemon {
    config: DaemonConfig,
    tracker: QueryTracker,
    generator: AdviceGenerator,
    api: PgAdviceApi,
    applied_advice: Vec<AppliedAdvice>,
}

/// Record of advice that was generated and applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedAdvice {
    /// The query this advice targets.
    pub query_id: String,
    /// The combined advice string.
    pub advice_string: String,
    /// The SQL used to apply the advice.
    pub apply_sql: String,
    /// Number of advice items combined.
    pub item_count: usize,
}

impl AdvisorDaemon {
    /// Create a new daemon with the given configuration.
    #[must_use]
    pub fn new(config: DaemonConfig) -> Self {
        let tracker =
            QueryTracker::new(config.slow_threshold_ms);
        let generator =
            AdviceGenerator::new(config.min_confidence);
        let api = PgAdviceApi::new(config.backend);

        Self {
            config,
            tracker,
            generator,
            api,
            applied_advice: Vec::new(),
        }
    }

    /// Feed a query observation into the tracker.
    pub fn observe(&mut self, obs: &QueryObservation) {
        debug!(
            query = obs.query_text.as_str(),
            duration_ms = obs.duration_ms,
            "observed query"
        );
        self.tracker.observe(obs);
    }

    /// Run one advisory cycle: identify candidates, generate
    /// advice, and return the SQL statements to execute.
    ///
    /// The caller is responsible for executing the returned SQL
    /// against `PostgreSQL`.
    #[must_use]
    pub fn advise_cycle(
        &mut self,
        plan_lookup: &dyn Fn(&str) -> Option<ra_core::RelExpr>,
    ) -> Vec<AppliedAdvice> {
        let candidates = self.tracker.candidates();
        if candidates.is_empty() {
            debug!("no candidates in this cycle");
            return Vec::new();
        }

        info!(
            count = candidates.len(),
            "found optimization candidates"
        );

        let mut results = Vec::new();
        let limit = self.config.max_advice_per_cycle;

        for candidate in candidates.iter().take(limit) {
            if let Some(applied) =
                self.advise_single(candidate, plan_lookup)
            {
                results.push(applied);
            }
        }

        self.applied_advice.extend(results.clone());
        results
    }

    /// Generate advice for a single query (one-shot mode).
    #[must_use]
    pub fn advise_query(
        &self,
        query_text: &str,
        optimized_plan: &ra_core::RelExpr,
    ) -> Option<AppliedAdvice> {
        let query_id = format!(
            "{:016x}",
            simple_hash(query_text)
        );

        let advice =
            self.generator.generate(&query_id, optimized_plan);
        if advice.is_empty() {
            return None;
        }

        let filtered: Vec<_> = advice
            .into_iter()
            .filter(|a| a.confidence >= self.config.min_confidence)
            .collect();
        if filtered.is_empty() {
            return None;
        }

        let combined =
            AdviceGenerator::combine_advice(&filtered);
        let apply_sql = self.api.format_apply_sql(
            &filtered,
            Some(query_text),
        );

        info!(
            query_id = query_id.as_str(),
            advice = combined.as_str(),
            "generated advice"
        );

        Some(AppliedAdvice {
            query_id,
            advice_string: combined,
            apply_sql,
            item_count: filtered.len(),
        })
    }

    /// Get all advice that has been applied so far.
    #[must_use]
    pub fn applied_advice(&self) -> &[AppliedAdvice] {
        &self.applied_advice
    }

    /// Get the current configuration.
    #[must_use]
    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    /// Get the active backend.
    #[must_use]
    pub fn backend(&self) -> AdviceBackend {
        self.api.backend()
    }

    /// Number of tracked queries.
    #[must_use]
    pub fn tracked_queries(&self) -> usize {
        self.tracker.tracked_count()
    }

    fn advise_single(
        &self,
        candidate: &QueryCandidate,
        plan_lookup: &dyn Fn(&str) -> Option<ra_core::RelExpr>,
    ) -> Option<AppliedAdvice> {
        let plan = plan_lookup(&candidate.query_text)?;

        let advice = self.generator.generate(
            &candidate.query_id,
            &plan,
        );
        if advice.is_empty() {
            return None;
        }

        let filtered: Vec<PlanAdvice> = advice
            .into_iter()
            .filter(|a| a.confidence >= self.config.min_confidence)
            .collect();
        if filtered.is_empty() {
            return None;
        }

        let combined =
            AdviceGenerator::combine_advice(&filtered);
        let apply_sql = self.api.format_apply_sql(
            &filtered,
            Some(&candidate.query_text),
        );

        info!(
            query_id = candidate.query_id.as_str(),
            reason = %candidate.reason,
            advice = combined.as_str(),
            "generated advice for candidate"
        );

        Some(AppliedAdvice {
            query_id: candidate.query_id.clone(),
            advice_string: combined,
            apply_sql,
            item_count: filtered.len(),
        })
    }
}

fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash
            .wrapping_mul(33)
            .wrapping_add(u64::from(byte));
    }
    hash
}


#[cfg(test)]
#[allow(clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use ra_core::{Const, Expr, JoinType, RelExpr};

    fn test_config() -> DaemonConfig {
        DaemonConfig {
            connection_string: "test".into(),
            slow_threshold_ms: 100.0,
            min_confidence: 0.5,
            ..DaemonConfig::default()
        }
    }

    fn test_plan() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::Scan {
                table: "orders".to_string(),
                alias: None,
            }),
            right: Box::new(RelExpr::Scan {
                table: "customers".to_string(),
                alias: None,
            }),
        }
    }

    #[test]
    fn one_shot_advice() {
        let daemon = AdvisorDaemon::new(test_config());
        let plan = test_plan();
        let result = daemon.advise_query(
            "SELECT * FROM orders JOIN customers ON o.id = c.id",
            &plan,
        );
        let applied = result
            .expect("advice should be generated");
        assert!(!applied.advice_string.is_empty());
        assert!(!applied.apply_sql.is_empty());
        assert!(applied.item_count > 0);
    }

    #[test]
    fn advise_cycle_no_candidates() {
        let mut daemon = AdvisorDaemon::new(test_config());
        let lookup = |_: &str| -> Option<RelExpr> { None };
        let results = daemon.advise_cycle(&lookup);
        assert!(results.is_empty());
    }

    #[test]
    fn advise_cycle_with_candidates() {
        let mut daemon = AdvisorDaemon::new(test_config());

        daemon.observe(&QueryObservation {
            query_text: "SELECT * FROM orders".into(),
            duration_ms: 500.0,
            total_cost: 1000.0,
            observed_at: std::time::Instant::now(),
        });

        let plan = test_plan();
        let lookup = move |_: &str| -> Option<RelExpr> {
            Some(plan.clone())
        };
        let results = daemon.advise_cycle(&lookup);
        assert!(!results.is_empty());
    }

    #[test]
    fn applied_advice_accumulates() {
        let mut daemon = AdvisorDaemon::new(test_config());

        daemon.observe(&QueryObservation {
            query_text: "SELECT * FROM t".into(),
            duration_ms: 500.0,
            total_cost: 1000.0,
            observed_at: std::time::Instant::now(),
        });

        let plan = test_plan();
        let lookup = move |_: &str| -> Option<RelExpr> {
            Some(plan.clone())
        };
        let _ = daemon.advise_cycle(&lookup);
        assert!(!daemon.applied_advice().is_empty());
    }

    #[test]
    fn default_config_values() {
        let config = DaemonConfig::default();
        assert_eq!(
            config.poll_interval,
            Duration::from_secs(5)
        );
        assert_eq!(config.slow_threshold_ms, 100.0);
        assert_eq!(
            config.backend,
            AdviceBackend::PgPlanAdviceGuc
        );
    }

    #[test]
    fn daemon_tracks_backend() {
        let daemon = AdvisorDaemon::new(test_config());
        assert_eq!(
            daemon.backend(),
            AdviceBackend::PgPlanAdviceGuc
        );
    }

    #[test]
    fn daemon_tracks_query_count() {
        let mut daemon = AdvisorDaemon::new(test_config());
        assert_eq!(daemon.tracked_queries(), 0);

        daemon.observe(&QueryObservation {
            query_text: "q1".into(),
            duration_ms: 10.0,
            total_cost: 1.0,
            observed_at: std::time::Instant::now(),
        });
        assert_eq!(daemon.tracked_queries(), 1);
    }
}
