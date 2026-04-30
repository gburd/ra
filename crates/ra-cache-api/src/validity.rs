//! Statistics drift types for cached plan validity.

use crate::key::QueryKey;

/// Whether a cached plan's statistics are still fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftStatus {
    /// Statistics are within the drift threshold.
    Fresh,
    /// At least one table has drifted beyond the threshold.
    Stale,
    /// Statistics are missing for a referenced table.
    Unknown,
}

/// Which dimension of statistics drifted (RFC 0059).
#[derive(Debug, Clone, PartialEq)]
pub enum DriftDimension {
    /// Table row count changed.
    RowCount {
        /// Table name.
        table: String,
        /// Cached row count.
        old_count: f64,
        /// Current row count.
        new_count: f64,
        /// Fractional drift.
        drift: f64,
    },
    /// Column distinct count changed.
    DistinctCount {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
        /// Cached NDV.
        old_ndv: f64,
        /// Current NDV.
        new_ndv: f64,
        /// Fractional drift.
        drift: f64,
    },
    /// Histogram shape changed (different bucket count).
    HistogramShape {
        /// Table name.
        table: String,
        /// Column name.
        column: String,
        /// Cached bucket count.
        old_buckets: usize,
        /// Current bucket count.
        new_buckets: usize,
    },
    /// Index was added or dropped.
    IndexPresence {
        /// Table name.
        table: String,
        /// Index name.
        index_name: String,
        /// Whether the index was added (true) or dropped.
        added: bool,
    },
}

/// Drift information for a single table.
#[derive(Debug, Clone)]
pub struct TableDrift {
    /// Table name.
    pub table: String,
    /// Row count at optimization time.
    pub cached_row_count: f64,
    /// Current row count (if available).
    pub current_row_count: Option<f64>,
    /// Absolute fractional drift: `|current - cached| / cached`.
    pub drift_fraction: Option<f64>,
    /// Which dimensions drifted (RFC 0059).
    pub drifted_dimensions: Vec<DriftDimension>,
}

/// Aggregated drift report for a plan.
#[derive(Debug, Clone)]
pub struct PlanDrift {
    /// Overall drift status.
    pub status: DriftStatus,
    /// Per-table drift details.
    pub table_drifts: Vec<TableDrift>,
    /// Maximum drift fraction observed.
    pub max_drift: f64,
    /// All dimensions that drifted (RFC 0059).
    pub dimensions: Vec<DriftDimension>,
}

/// Drift report across all cached plans.
#[derive(Debug, Clone)]
pub struct DriftReport {
    /// Plans that have drifted beyond the threshold.
    pub stale_plans: Vec<(QueryKey, PlanDrift)>,
}

impl DriftReport {
    /// Create an empty drift report.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stale_plans: Vec::new(),
        }
    }
}

impl Default for DriftReport {
    fn default() -> Self {
        Self::new()
    }
}
