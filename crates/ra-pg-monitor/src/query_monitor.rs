//! Query tracking and analysis.
//!
//! Monitors query execution, identifies expensive queries (slow,
//! high I/O, cache misses), logs plan inefficiencies, and detects
//! plan regressions from changed statistics.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// How bad is this query?
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
    Hash, Serialize, Deserialize,
)]
pub enum QuerySeverity {
    /// Within acceptable thresholds.
    Normal,
    /// Slower than typical, worth investigating.
    Slow,
    /// Much slower than expected, should be optimized.
    VerySlow,
    /// Actively degrading system performance.
    Critical,
}

impl fmt::Display for QuerySeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => write!(f, "OK"),
            Self::Slow => write!(f, "SLOW"),
            Self::VerySlow => write!(f, "VERY SLOW"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// What kind of plan node is this query using?
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
)]
pub enum PlanNodeType {
    /// Sequential scan (full table scan).
    SeqScan,
    /// Index scan.
    IndexScan,
    /// Index-only scan.
    IndexOnlyScan,
    /// Bitmap scan.
    BitmapScan,
    /// Nested loop join.
    NestedLoop,
    /// Hash join.
    HashJoin,
    /// Merge join.
    MergeJoin,
    /// Sort operation.
    Sort,
    /// Hash aggregate.
    HashAggregate,
    /// Group aggregate.
    GroupAggregate,
    /// Materialize.
    Materialize,
    /// Other/unknown.
    Other,
}

impl fmt::Display for PlanNodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SeqScan => write!(f, "Seq Scan"),
            Self::IndexScan => write!(f, "Index Scan"),
            Self::IndexOnlyScan => write!(f, "Index Only Scan"),
            Self::BitmapScan => write!(f, "Bitmap Scan"),
            Self::NestedLoop => write!(f, "Nested Loop"),
            Self::HashJoin => write!(f, "Hash Join"),
            Self::MergeJoin => write!(f, "Merge Join"),
            Self::Sort => write!(f, "Sort"),
            Self::HashAggregate => write!(f, "Hash Aggregate"),
            Self::GroupAggregate => write!(f, "Group Aggregate"),
            Self::Materialize => write!(f, "Materialize"),
            Self::Other => write!(f, "Other"),
        }
    }
}

/// Plan node information extracted from EXPLAIN output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanNode {
    /// Type of plan node.
    pub node_type: PlanNodeType,
    /// Table being scanned (if applicable).
    pub relation: Option<String>,
    /// Estimated rows from planner.
    pub estimated_rows: f64,
    /// Actual rows (if EXPLAIN ANALYZE).
    pub actual_rows: Option<f64>,
    /// Startup cost.
    pub startup_cost: f64,
    /// Total cost.
    pub total_cost: f64,
}

/// A recorded query with timing and plan information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRecord {
    /// The SQL text.
    pub query: String,
    /// Execution duration in milliseconds.
    pub duration_ms: f64,
    /// Total estimated cost from the planner.
    pub total_cost: f64,
    /// Root plan node type.
    pub root_plan: PlanNodeType,
    /// Plan nodes with details.
    pub plan_nodes: Vec<PlanNode>,
    /// Number of rows returned.
    pub rows_returned: u64,
    /// Shared buffers hit (cache hits).
    pub shared_hit: u64,
    /// Shared buffers read (cache misses).
    pub shared_read: u64,
    /// Severity classification.
    pub severity: QuerySeverity,
    /// Actionable suggestion.
    pub suggestion: String,
    /// Whether this represents a plan regression.
    pub is_regression: bool,
}

/// Tracks queries and identifies performance issues.
pub struct QueryMonitor {
    /// Threshold in ms above which a query is "slow".
    slow_threshold_ms: f64,
    /// Recorded queries.
    records: Vec<QueryRecord>,
    /// Historical cost per query hash for regression detection.
    cost_history: HashMap<u64, Vec<f64>>,
}

impl QueryMonitor {
    /// Create a new monitor with the given slow query threshold.
    #[must_use]
    pub fn new(slow_threshold_ms: f64) -> Self {
        Self {
            slow_threshold_ms,
            records: Vec::new(),
            cost_history: HashMap::new(),
        }
    }

    /// Record a query observation.
    pub fn record(&mut self, mut record: QueryRecord) {
        record.severity = self.classify_severity(&record);
        record.suggestion = self.generate_suggestion(&record);
        record.is_regression =
            self.detect_regression(&record);

        let hash = simple_hash(&record.query);
        self.cost_history
            .entry(hash)
            .or_default()
            .push(record.total_cost);

        self.records.push(record);
    }

    /// Get all recorded queries.
    #[must_use]
    pub fn all_queries(&self) -> &[QueryRecord] {
        &self.records
    }

    /// Get queries that exceeded the slow threshold.
    #[must_use]
    pub fn slow_queries(&self) -> Vec<&QueryRecord> {
        self.records
            .iter()
            .filter(|r| r.severity >= QuerySeverity::Slow)
            .collect()
    }

    /// Get the most recent N queries.
    #[must_use]
    pub fn recent_queries(&self, n: usize) -> &[QueryRecord] {
        let start = self.records.len().saturating_sub(n);
        &self.records[start..]
    }

    /// Clear all recorded queries.
    pub fn clear(&mut self) {
        self.records.clear();
    }

    fn classify_severity(
        &self,
        record: &QueryRecord,
    ) -> QuerySeverity {
        if record.duration_ms > self.slow_threshold_ms * 10.0 {
            QuerySeverity::Critical
        } else if record.duration_ms > self.slow_threshold_ms * 3.0
        {
            QuerySeverity::VerySlow
        } else if record.duration_ms > self.slow_threshold_ms {
            QuerySeverity::Slow
        } else {
            QuerySeverity::Normal
        }
    }

    fn generate_suggestion(
        &self,
        record: &QueryRecord,
    ) -> String {
        let mut suggestions = Vec::new();

        for node in &record.plan_nodes {
            if node.node_type == PlanNodeType::SeqScan {
                if node.estimated_rows > 10_000.0 {
                    let table = node
                        .relation
                        .as_deref()
                        .unwrap_or("unknown");
                    suggestions.push(format!(
                        "Add index on '{table}' to avoid sequential scan \
                         ({:.0} rows)",
                        node.estimated_rows,
                    ));
                }
            }

            if let Some(actual) = node.actual_rows {
                if node.estimated_rows > 0.0 {
                    let ratio = actual / node.estimated_rows;
                    if ratio > 10.0 || ratio < 0.1 {
                        suggestions.push(format!(
                            "Cardinality misestimate on {}: \
                             estimated {:.0}, actual {:.0}. \
                             Consider running ANALYZE.",
                            node.node_type,
                            node.estimated_rows,
                            actual,
                        ));
                    }
                }
            }
        }

        let cache_total = record.shared_hit + record.shared_read;
        if cache_total > 0 {
            let hit_ratio = record.shared_hit as f64
                / cache_total as f64;
            if hit_ratio < 0.9 {
                suggestions.push(format!(
                    "Low cache hit ratio ({:.1}%). Consider \
                     increasing shared_buffers.",
                    hit_ratio * 100.0,
                ));
            }
        }

        if suggestions.is_empty() && record.duration_ms > self.slow_threshold_ms {
            suggestions.push(
                "Review query plan for optimization opportunities"
                    .to_string(),
            );
        }

        suggestions.join("; ")
    }

    fn detect_regression(
        &self,
        record: &QueryRecord,
    ) -> bool {
        let hash = simple_hash(&record.query);
        if let Some(history) = self.cost_history.get(&hash) {
            if history.len() >= 2 {
                let avg: f64 =
                    history.iter().sum::<f64>()
                        / history.len() as f64;
                return record.total_cost > avg * 2.0;
            }
        }
        false
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
mod tests {
    use super::*;

    fn make_record(
        query: &str,
        duration_ms: f64,
        cost: f64,
    ) -> QueryRecord {
        QueryRecord {
            query: query.to_string(),
            duration_ms,
            total_cost: cost,
            root_plan: PlanNodeType::SeqScan,
            plan_nodes: vec![],
            rows_returned: 100,
            shared_hit: 90,
            shared_read: 10,
            severity: QuerySeverity::Normal,
            suggestion: String::new(),
            is_regression: false,
        }
    }

    #[test]
    fn classify_normal() {
        let monitor = QueryMonitor::new(100.0);
        let record = make_record("SELECT 1", 10.0, 1.0);
        assert_eq!(
            monitor.classify_severity(&record),
            QuerySeverity::Normal,
        );
    }

    #[test]
    fn classify_slow() {
        let monitor = QueryMonitor::new(100.0);
        let record = make_record("SELECT *", 150.0, 1000.0);
        assert_eq!(
            monitor.classify_severity(&record),
            QuerySeverity::Slow,
        );
    }

    #[test]
    fn classify_very_slow() {
        let monitor = QueryMonitor::new(100.0);
        let record = make_record("SELECT *", 350.0, 5000.0);
        assert_eq!(
            monitor.classify_severity(&record),
            QuerySeverity::VerySlow,
        );
    }

    #[test]
    fn classify_critical() {
        let monitor = QueryMonitor::new(100.0);
        let record =
            make_record("SELECT *", 1500.0, 50000.0);
        assert_eq!(
            monitor.classify_severity(&record),
            QuerySeverity::Critical,
        );
    }

    #[test]
    fn suggestion_for_seq_scan() {
        let monitor = QueryMonitor::new(100.0);
        let record = QueryRecord {
            query: "SELECT * FROM orders".to_string(),
            duration_ms: 200.0,
            total_cost: 5000.0,
            root_plan: PlanNodeType::SeqScan,
            plan_nodes: vec![PlanNode {
                node_type: PlanNodeType::SeqScan,
                relation: Some("orders".to_string()),
                estimated_rows: 1_000_000.0,
                actual_rows: None,
                startup_cost: 0.0,
                total_cost: 5000.0,
            }],
            rows_returned: 1000,
            shared_hit: 50,
            shared_read: 50,
            severity: QuerySeverity::Normal,
            suggestion: String::new(),
            is_regression: false,
        };
        let suggestion = monitor.generate_suggestion(&record);
        assert!(suggestion.contains("Add index"));
        assert!(suggestion.contains("orders"));
    }

    #[test]
    fn regression_detection() {
        let mut monitor = QueryMonitor::new(100.0);
        let r1 = make_record("SELECT * FROM t", 50.0, 100.0);
        monitor.record(r1);
        let r2 = make_record("SELECT * FROM t", 50.0, 100.0);
        monitor.record(r2);

        let r3 = make_record("SELECT * FROM t", 500.0, 500.0);
        monitor.record(r3);

        let last = monitor.records.last();
        assert!(last.is_some());
        assert!(
            last.map_or(false, |r| r.is_regression),
            "cost 500 > avg(100,100)*2 should be regression"
        );
    }

    #[test]
    fn slow_queries_filter() {
        let mut monitor = QueryMonitor::new(100.0);
        monitor.record(make_record("fast", 10.0, 1.0));
        monitor.record(make_record("slow", 200.0, 1000.0));
        monitor.record(make_record("fast2", 5.0, 0.5));

        let slow = monitor.slow_queries();
        assert_eq!(slow.len(), 1);
        assert_eq!(slow[0].query, "slow");
    }

    #[test]
    fn recent_queries() {
        let mut monitor = QueryMonitor::new(100.0);
        for i in 0..10 {
            monitor.record(make_record(
                &format!("q{i}"),
                10.0,
                1.0,
            ));
        }
        let recent = monitor.recent_queries(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].query, "q7");
        assert_eq!(recent[2].query, "q9");
    }

    #[test]
    fn clear_queries() {
        let mut monitor = QueryMonitor::new(100.0);
        monitor.record(make_record("q1", 10.0, 1.0));
        assert_eq!(monitor.all_queries().len(), 1);
        monitor.clear();
        assert!(monitor.all_queries().is_empty());
    }

    #[test]
    fn cardinality_misestimate_suggestion() {
        let monitor = QueryMonitor::new(100.0);
        let record = QueryRecord {
            query: "SELECT * FROM t".to_string(),
            duration_ms: 200.0,
            total_cost: 500.0,
            root_plan: PlanNodeType::SeqScan,
            plan_nodes: vec![PlanNode {
                node_type: PlanNodeType::SeqScan,
                relation: Some("t".to_string()),
                estimated_rows: 100.0,
                actual_rows: Some(50_000.0),
                startup_cost: 0.0,
                total_cost: 500.0,
            }],
            rows_returned: 50000,
            shared_hit: 90,
            shared_read: 10,
            severity: QuerySeverity::Normal,
            suggestion: String::new(),
            is_regression: false,
        };
        let suggestion = monitor.generate_suggestion(&record);
        assert!(suggestion.contains("Cardinality misestimate"));
        assert!(suggestion.contains("ANALYZE"));
    }

    #[test]
    fn low_cache_hit_ratio() {
        let monitor = QueryMonitor::new(100.0);
        let record = QueryRecord {
            query: "SELECT * FROM big".to_string(),
            duration_ms: 200.0,
            total_cost: 1000.0,
            root_plan: PlanNodeType::SeqScan,
            plan_nodes: vec![],
            rows_returned: 10000,
            shared_hit: 10,
            shared_read: 90,
            severity: QuerySeverity::Normal,
            suggestion: String::new(),
            is_regression: false,
        };
        let suggestion = monitor.generate_suggestion(&record);
        assert!(suggestion.contains("cache hit ratio"));
        assert!(suggestion.contains("shared_buffers"));
    }
}
