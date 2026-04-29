//! Runtime statistics collected during query execution.
//!
//! Each operator in a query plan emits statistics as rows flow
//! through it. These observed values are compared against the
//! optimizer's estimates to detect misestimates that warrant
//! reoptimization.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Unique identifier for a node in the execution plan.
pub type NodeId = u64;

/// Statistics observed at a single operator during execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperatorStats {
    /// Actual number of rows produced so far.
    pub actual_rows: u64,
    /// Estimated number of rows (from the optimizer).
    pub estimated_rows: f64,
    /// Cumulative wall-clock time spent in this operator.
    pub elapsed: Duration,
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,
    /// Per-column value distribution observed at runtime.
    pub column_sketches: HashMap<String, ColumnSketch>,
}

impl OperatorStats {
    /// Create stats for an operator with only an estimate.
    #[must_use]
    pub fn with_estimate(estimated_rows: f64) -> Self {
        Self {
            actual_rows: 0,
            estimated_rows,
            elapsed: Duration::ZERO,
            peak_memory_bytes: 0,
            column_sketches: HashMap::new(),
        }
    }

    /// The ratio of actual to estimated rows.
    ///
    /// Returns `None` when the estimate is zero (avoids division
    /// by zero).
    #[must_use]
    pub fn cardinality_ratio(&self) -> Option<f64> {
        if self.estimated_rows.abs() < f64::EPSILON {
            return None;
        }
        let ratio = self.actual_rows as f64 / self.estimated_rows;
        Some(ratio)
    }

    /// Record that additional rows were produced.
    pub fn record_rows(&mut self, count: u64) {
        self.actual_rows = self.actual_rows.saturating_add(count);
    }

    /// Update elapsed time.
    pub fn record_elapsed(&mut self, elapsed: Duration) {
        self.elapsed = elapsed;
    }

    /// Update peak memory.
    pub fn record_memory(&mut self, bytes: u64) {
        if bytes > self.peak_memory_bytes {
            self.peak_memory_bytes = bytes;
        }
    }
}

/// Lightweight sketch of column value distribution observed at
/// runtime. Used for skew detection and selectivity correction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnSketch {
    /// Approximate number of distinct values seen.
    pub approx_distinct: u64,
    /// Number of NULL values seen.
    pub null_count: u64,
    /// Total values seen (including NULLs).
    pub total_count: u64,
    /// Most frequent value and its count, if tracked.
    pub most_frequent: Option<(String, u64)>,
}

impl ColumnSketch {
    /// Create an empty sketch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            approx_distinct: 0,
            null_count: 0,
            total_count: 0,
            most_frequent: None,
        }
    }

    /// Observed null fraction in `[0.0, 1.0]`.
    #[must_use]
    pub fn null_fraction(&self) -> f64 {
        if self.total_count == 0 {
            return 0.0;
        }
        self.null_count as f64 / self.total_count as f64
    }

    /// Observed selectivity for an equality predicate.
    ///
    /// Uses `1 / approx_distinct` when available, falls back to a
    /// default of `0.1`.
    #[must_use]
    pub fn equality_selectivity(&self) -> f64 {
        if self.approx_distinct > 0 {
            1.0 / self.approx_distinct as f64
        } else {
            0.1
        }
    }

    /// Whether the distribution appears skewed.
    ///
    /// A column is considered skewed when the most frequent value
    /// accounts for more than `threshold` fraction of all rows.
    #[must_use]
    pub fn is_skewed(&self, threshold: f64) -> bool {
        if let Some((_, freq_count)) = &self.most_frequent {
            if self.total_count == 0 {
                return false;
            }
            let fraction = *freq_count as f64 / self.total_count as f64;
            fraction > threshold
        } else {
            false
        }
    }
}

impl Default for ColumnSketch {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregate of runtime statistics across all operators in a plan.
#[derive(Debug, Clone, Default)]
pub struct PlanStats {
    /// Per-operator statistics, keyed by node identifier.
    pub operators: HashMap<NodeId, OperatorStats>,
}

impl PlanStats {
    /// Create an empty collection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an operator with its estimated row count.
    pub fn register(&mut self, node_id: NodeId, estimated_rows: f64) {
        self.operators
            .insert(node_id, OperatorStats::with_estimate(estimated_rows));
    }

    /// Record rows produced by an operator.
    ///
    /// Returns `false` if the `node_id` was not registered.
    pub fn record_rows(&mut self, node_id: NodeId, count: u64) -> bool {
        if let Some(stats) = self.operators.get_mut(&node_id) {
            stats.record_rows(count);
            true
        } else {
            false
        }
    }

    /// Record elapsed time for an operator.
    pub fn record_elapsed(&mut self, node_id: NodeId, elapsed: Duration) -> bool {
        if let Some(stats) = self.operators.get_mut(&node_id) {
            stats.record_elapsed(elapsed);
            true
        } else {
            false
        }
    }

    /// Get stats for a specific operator.
    #[must_use]
    pub fn get(&self, node_id: NodeId) -> Option<&OperatorStats> {
        self.operators.get(&node_id)
    }

    /// Total rows produced across all operators.
    #[must_use]
    pub fn total_rows(&self) -> u64 {
        self.operators.values().map(|s| s.actual_rows).sum()
    }

    /// Total elapsed time across all operators.
    #[must_use]
    pub fn total_elapsed(&self) -> Duration {
        self.operators.values().map(|s| s.elapsed).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_stats_cardinality_ratio() {
        let mut stats = OperatorStats::with_estimate(100.0);
        stats.record_rows(200);
        let ratio = stats
            .cardinality_ratio()
            .expect("ratio should be computable");
        assert!((ratio - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn operator_stats_zero_estimate() {
        let stats = OperatorStats::with_estimate(0.0);
        assert!(stats.cardinality_ratio().is_none());
    }

    #[test]
    fn operator_stats_record_memory() {
        let mut stats = OperatorStats::with_estimate(10.0);
        stats.record_memory(1024);
        assert_eq!(stats.peak_memory_bytes, 1024);
        stats.record_memory(512);
        assert_eq!(stats.peak_memory_bytes, 1024);
        stats.record_memory(2048);
        assert_eq!(stats.peak_memory_bytes, 2048);
    }

    #[test]
    fn column_sketch_null_fraction() {
        let sketch = ColumnSketch {
            approx_distinct: 10,
            null_count: 25,
            total_count: 100,
            most_frequent: None,
        };
        assert!((sketch.null_fraction() - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn column_sketch_null_fraction_empty() {
        let sketch = ColumnSketch::new();
        assert!((sketch.null_fraction() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn column_sketch_equality_selectivity() {
        let sketch = ColumnSketch {
            approx_distinct: 50,
            null_count: 0,
            total_count: 100,
            most_frequent: None,
        };
        assert!((sketch.equality_selectivity() - 0.02).abs() < f64::EPSILON);
    }

    #[test]
    fn column_sketch_equality_selectivity_zero_distinct() {
        let sketch = ColumnSketch::new();
        assert!((sketch.equality_selectivity() - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn column_sketch_skew_detection() {
        let sketch = ColumnSketch {
            approx_distinct: 100,
            null_count: 0,
            total_count: 1000,
            most_frequent: Some(("hot_key".into(), 600)),
        };
        assert!(sketch.is_skewed(0.5));
        assert!(!sketch.is_skewed(0.7));
    }

    #[test]
    fn column_sketch_skew_no_frequent() {
        let sketch = ColumnSketch::new();
        assert!(!sketch.is_skewed(0.1));
    }

    #[test]
    fn plan_stats_register_and_record() {
        let mut plan = PlanStats::new();
        plan.register(1, 100.0);
        plan.register(2, 500.0);

        assert!(plan.record_rows(1, 120));
        assert!(plan.record_rows(2, 480));
        assert!(!plan.record_rows(99, 10));

        let s1 = plan.get(1).expect("node 1 should exist");
        assert_eq!(s1.actual_rows, 120);

        let s2 = plan.get(2).expect("node 2 should exist");
        assert_eq!(s2.actual_rows, 480);
    }

    #[test]
    fn plan_stats_total_rows() {
        let mut plan = PlanStats::new();
        plan.register(1, 10.0);
        plan.register(2, 20.0);
        plan.record_rows(1, 15);
        plan.record_rows(2, 25);
        assert_eq!(plan.total_rows(), 40);
    }

    #[test]
    fn plan_stats_total_elapsed() {
        let mut plan = PlanStats::new();
        plan.register(1, 10.0);
        plan.register(2, 20.0);
        plan.record_elapsed(1, Duration::from_millis(100));
        plan.record_elapsed(2, Duration::from_millis(200));
        assert_eq!(plan.total_elapsed(), Duration::from_millis(300));
    }

    #[test]
    fn operator_stats_serialize_roundtrip() {
        let stats = OperatorStats::with_estimate(42.0);
        let json = serde_json::to_string(&stats).expect("serialization should succeed");
        let deserialized: OperatorStats =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(stats, deserialized);
    }
}
