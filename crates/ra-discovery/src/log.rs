//! Execution log collection for rule discovery.
//!
//! Records query plans alongside their execution metrics (wall time,
//! actual cardinalities, costs) so that downstream mining algorithms
//! can identify optimization opportunities.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use ra_core::algebra::RelExpr;
use ra_core::cost::Cost;

/// A single recorded execution of a query plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLog {
    /// Unique identifier for this log entry.
    pub id: u64,
    /// The original (unoptimized) query plan.
    pub original_plan: RelExpr,
    /// The optimized query plan that was actually executed.
    pub optimized_plan: RelExpr,
    /// Estimated cost from the optimizer.
    pub estimated_cost: Cost,
    /// Actual wall-clock execution time.
    pub execution_time: Duration,
    /// Actual cardinalities observed at each operator node.
    ///
    /// Keys are node identifiers (depth-first index in the plan
    /// tree), values are the row counts observed.
    pub actual_cardinalities: HashMap<usize, u64>,
    /// Estimated cardinalities from the optimizer at each node.
    pub estimated_cardinalities: HashMap<usize, f64>,
    /// Optional tags for grouping logs (e.g., workload name).
    pub tags: Vec<String>,
}

impl ExecutionLog {
    /// Compute the cardinality estimation error ratio for a node.
    ///
    /// Returns `estimated / actual` when both are available and
    /// actual is nonzero.  A ratio of 1.0 means perfect estimation;
    /// values above 1.0 indicate overestimation.
    #[must_use]
    pub fn estimation_error(&self, node_id: usize) -> Option<f64> {
        let estimated = self.estimated_cardinalities.get(&node_id)?;
        let actual = self.actual_cardinalities.get(&node_id)?;
        if *actual == 0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(estimated / *actual as f64)
    }
}

/// Collects execution logs and provides query access.
#[derive(Debug, Default)]
pub struct LogStore {
    logs: Vec<ExecutionLog>,
    next_id: u64,
}

impl LogStore {
    /// Create an empty log store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new execution log entry and return its id.
    pub fn record(&mut self, mut log: ExecutionLog) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        log.id = id;
        self.logs.push(log);
        id
    }

    /// Return all stored logs.
    #[must_use]
    pub fn logs(&self) -> &[ExecutionLog] {
        &self.logs
    }

    /// Return logs matching the given tag.
    #[must_use]
    pub fn logs_with_tag(&self, tag: &str) -> Vec<&ExecutionLog> {
        self.logs
            .iter()
            .filter(|log| log.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// Return the number of stored logs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.logs.len()
    }

    /// Check if the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.logs.is_empty()
    }

    /// Return logs where a given node has estimation error above
    /// the threshold (ratio of estimated/actual).
    #[must_use]
    pub fn logs_with_high_error(&self, threshold: f64) -> Vec<&ExecutionLog> {
        self.logs
            .iter()
            .filter(|log| {
                log.actual_cardinalities.keys().any(|node_id| {
                    log.estimation_error(*node_id)
                        .is_some_and(|ratio| ratio > threshold || ratio < 1.0 / threshold)
                })
            })
            .collect()
    }

    /// Split logs into training and validation sets.
    ///
    /// Uses a deterministic split: the first `fraction` of logs
    /// become training data, the rest become validation data.
    #[must_use]
    pub fn split(&self, fraction: f64) -> (&[ExecutionLog], &[ExecutionLog]) {
        let fraction = fraction.clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation)]
        #[allow(clippy::cast_sign_loss)]
        #[allow(clippy::cast_precision_loss)]
        let split_idx = (self.logs.len() as f64 * fraction) as usize;
        self.logs.split_at(split_idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::cost::Cost;

    fn sample_log(tags: Vec<String>) -> ExecutionLog {
        let mut actual = HashMap::new();
        actual.insert(0, 1000);
        actual.insert(1, 100);

        let mut estimated = HashMap::new();
        estimated.insert(0, 1000.0);
        estimated.insert(1, 500.0);

        ExecutionLog {
            id: 0,
            original_plan: RelExpr::scan("t"),
            optimized_plan: RelExpr::scan("t"),
            estimated_cost: Cost::new(10.0, 5.0, 0.0, 1024),
            execution_time: Duration::from_millis(50),
            actual_cardinalities: actual,
            estimated_cardinalities: estimated,
            tags,
        }
    }

    #[test]
    fn record_assigns_sequential_ids() {
        let mut store = LogStore::new();
        let id0 = store.record(sample_log(vec![]));
        let id1 = store.record(sample_log(vec![]));
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn logs_with_tag_filters() {
        let mut store = LogStore::new();
        store.record(sample_log(vec!["tpch".into()]));
        store.record(sample_log(vec!["custom".into()]));
        store.record(sample_log(vec!["tpch".into()]));

        let tpch = store.logs_with_tag("tpch");
        assert_eq!(tpch.len(), 2);

        let custom = store.logs_with_tag("custom");
        assert_eq!(custom.len(), 1);
    }

    #[test]
    fn estimation_error_calculated() {
        let log = sample_log(vec![]);
        let err0 = log.estimation_error(0);
        assert!(err0.is_some());
        assert!((err0.unwrap_or(0.0) - 1.0).abs() < f64::EPSILON);

        let err1 = log.estimation_error(1);
        assert!(err1.is_some());
        assert!((err1.unwrap_or(0.0) - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimation_error_missing_node() {
        let log = sample_log(vec![]);
        assert!(log.estimation_error(999).is_none());
    }

    #[test]
    fn high_error_detection() {
        let mut store = LogStore::new();
        store.record(sample_log(vec![]));

        let high = store.logs_with_high_error(2.0);
        assert_eq!(high.len(), 1);

        let none = store.logs_with_high_error(10.0);
        assert!(none.is_empty());
    }

    #[test]
    fn split_logs() {
        let mut store = LogStore::new();
        for _ in 0..10 {
            store.record(sample_log(vec![]));
        }

        let (train, val) = store.split(0.8);
        assert_eq!(train.len(), 8);
        assert_eq!(val.len(), 2);
    }

    #[test]
    fn split_edge_cases() {
        let mut store = LogStore::new();
        for _ in 0..10 {
            store.record(sample_log(vec![]));
        }

        let (train, val) = store.split(0.0);
        assert_eq!(train.len(), 0);
        assert_eq!(val.len(), 10);

        let (train, val) = store.split(1.0);
        assert_eq!(train.len(), 10);
        assert_eq!(val.len(), 0);
    }

    #[test]
    fn empty_store() {
        let store = LogStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }
}
