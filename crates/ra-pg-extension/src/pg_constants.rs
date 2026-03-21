//! PostgreSQL system constants.
//!
//! Defines named constants for PostgreSQL configuration parameters,
//! cost model defaults, and system values. Using named constants instead
//! of magic numbers improves maintainability and prevents errors.

/// PostgreSQL default cost parameters.
///
/// These match PostgreSQL's built-in defaults and are used for
/// cost calibration and GUC manipulation.
pub mod cost_defaults {
    /// Sequential page fetch cost (baseline unit).
    pub const SEQ_PAGE_COST: f64 = 1.0;

    /// Random page fetch cost (HDD default).
    ///
    /// PostgreSQL default assumes spinning disks. Modern SSDs
    /// typically use 1.0-1.5.
    pub const RANDOM_PAGE_COST: f64 = 4.0;

    /// Cost to process one tuple (row).
    pub const CPU_TUPLE_COST: f64 = 0.01;

    /// Cost to process one index tuple.
    pub const CPU_INDEX_TUPLE_COST: f64 = 0.005;

    /// Cost of a comparison operator.
    pub const CPU_OPERATOR_COST: f64 = 0.0025;
}

/// PostgreSQL GUC parameter tuning values.
///
/// These are strategic values used to manipulate the planner's
/// behavior via cost-based guidance.
pub mod guc_tuning {
    /// Low random_page_cost for SSDs - favors index scans.
    ///
    /// Modern SSDs have minimal seek penalty, so random access
    /// is nearly as cheap as sequential.
    pub const RANDOM_PAGE_COST_SSD: f64 = 1.0;

    /// High random_page_cost - strongly favors sequential scans.
    ///
    /// Used to discourage index scans when we know seq scan is better.
    pub const RANDOM_PAGE_COST_FORCE_SEQSCAN: f64 = 10.0;
}

/// Rough cardinality estimation constants.
///
/// Used for initial cost estimates when detailed statistics aren't available.
pub mod estimation {
    /// Typical rows per page (8KB page, ~80 byte rows).
    pub const ROWS_PER_PAGE: f64 = 100.0;

    /// Default row count when no statistics available.
    pub const DEFAULT_ROW_COUNT: f64 = 1000.0;

    /// Average bytes per row (rough estimate for memory usage).
    pub const BYTES_PER_ROW: f64 = 100.0;
}

/// GUC parameter names.
///
/// Centralizes string literals for GUC names to avoid typos.
pub mod guc_names {
    pub const ENABLE_HASHJOIN: &str = "enable_hashjoin";
    pub const ENABLE_MERGEJOIN: &str = "enable_mergejoin";
    pub const ENABLE_NESTLOOP: &str = "enable_nestloop";
    pub const ENABLE_SEQSCAN: &str = "enable_seqscan";
    pub const ENABLE_INDEXSCAN: &str = "enable_indexscan";
    pub const ENABLE_BITMAPSCAN: &str = "enable_bitmapscan";
    pub const RANDOM_PAGE_COST: &str = "random_page_cost";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssd_cost_less_than_hdd() {
        assert!(guc_tuning::RANDOM_PAGE_COST_SSD < cost_defaults::RANDOM_PAGE_COST);
    }

    #[test]
    fn force_seqscan_cost_high() {
        assert!(guc_tuning::RANDOM_PAGE_COST_FORCE_SEQSCAN > cost_defaults::RANDOM_PAGE_COST);
    }

    #[test]
    fn cpu_tuple_cost_higher_than_operator() {
        assert!(cost_defaults::CPU_TUPLE_COST > cost_defaults::CPU_OPERATOR_COST);
    }

    #[test]
    fn guc_names_not_empty() {
        assert!(!guc_names::ENABLE_HASHJOIN.is_empty());
        assert!(!guc_names::RANDOM_PAGE_COST.is_empty());
    }
}
