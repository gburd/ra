//! Cost and benefit estimation for index recommendations.

#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use serde::{Deserialize, Serialize};

use crate::candidate::IndexType;

/// Estimated benefit from creating an index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexBenefit {
    /// Query IDs that would benefit from this index.
    pub affected_queries: Vec<String>,
    /// Average speedup factor for affected queries.
    pub avg_speedup: f64,
    /// Total cost units saved across all query executions.
    pub total_cost_saved: f64,
}

/// Estimated cost of creating and maintaining an index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexCost {
    /// Storage overhead in bytes.
    pub storage_bytes: u64,
    /// Write overhead factor (0.0 to 1.0, where 0.05 = 5% slower
    /// writes).
    pub write_overhead: f64,
    /// Estimated time to build the index in seconds.
    pub build_time_secs: f64,
}

/// Storage size ratios relative to table size for different index
/// types.
const BTREE_SIZE_RATIO: f64 = 0.3;
const BRIN_SIZE_RATIO: f64 = 0.001;
const GIN_SIZE_RATIO: f64 = 1.0;
const GIST_SIZE_RATIO: f64 = 0.4;
const HASH_SIZE_RATIO: f64 = 0.25;

/// Write overhead per operation for different index types.
const BTREE_WRITE_OVERHEAD: f64 = 0.05;
const BRIN_WRITE_OVERHEAD: f64 = 0.005;
const GIN_WRITE_OVERHEAD: f64 = 0.10;
const GIST_WRITE_OVERHEAD: f64 = 0.08;
const HASH_WRITE_OVERHEAD: f64 = 0.03;

impl IndexBenefit {
    /// Create a new benefit estimation.
    #[must_use]
    pub fn new(
        affected_queries: Vec<String>,
        avg_speedup: f64,
        total_cost_saved: f64,
    ) -> Self {
        Self {
            affected_queries,
            avg_speedup,
            total_cost_saved,
        }
    }

    /// Check if this index provides any benefit.
    #[must_use]
    pub fn has_benefit(&self) -> bool {
        !self.affected_queries.is_empty()
            && self.total_cost_saved > 0.0
    }

    /// Format benefit as human-readable string.
    #[must_use]
    pub fn format_benefit(&self) -> String {
        format!(
            "Affects {} queries, {:.1}x avg speedup, \
             {:.0} cost units saved",
            self.affected_queries.len(),
            self.avg_speedup,
            self.total_cost_saved
        )
    }
}

impl IndexCost {
    /// Create a new cost estimation.
    #[must_use]
    pub fn new(
        storage_bytes: u64,
        write_overhead: f64,
        build_time_secs: f64,
    ) -> Self {
        Self {
            storage_bytes,
            write_overhead,
            build_time_secs,
        }
    }

    /// Estimate cost for a given index type and table statistics.
    #[must_use]
    pub fn estimate(
        index_type: IndexType,
        table_size_bytes: u64,
        row_count: f64,
        num_columns: usize,
    ) -> Self {
        let size_ratio = match index_type {
            IndexType::BTree => BTREE_SIZE_RATIO,
            IndexType::BRIN => BRIN_SIZE_RATIO,
            IndexType::GIN => GIN_SIZE_RATIO,
            IndexType::GiST => GIST_SIZE_RATIO,
            IndexType::Hash => HASH_SIZE_RATIO,
        };

        let write_overhead = match index_type {
            IndexType::BTree => BTREE_WRITE_OVERHEAD,
            IndexType::BRIN => BRIN_WRITE_OVERHEAD,
            IndexType::GIN => GIN_WRITE_OVERHEAD,
            IndexType::GiST => GIST_WRITE_OVERHEAD,
            IndexType::Hash => HASH_WRITE_OVERHEAD,
        };

        let storage_bytes =
            (table_size_bytes as f64 * size_ratio * num_columns as f64)
                as u64;

        // Build time: proportional to row count and index complexity
        let build_cost_factor = match index_type {
            IndexType::BTree => 3.0,
            IndexType::BRIN => 1.0,
            IndexType::GIN => 5.0,
            IndexType::GiST => 4.0,
            IndexType::Hash => 2.0,
        };
        let build_time_secs =
            row_count / 100_000.0 * build_cost_factor;

        Self {
            storage_bytes,
            write_overhead: write_overhead * num_columns as f64,
            build_time_secs,
        }
    }

    /// Get total cost as a single normalized value.
    #[must_use]
    pub fn total(&self) -> f64 {
        let storage_cost =
            self.storage_bytes as f64 / (1024.0 * 1024.0);
        let write_cost = self.write_overhead * 1000.0;
        let build_cost = self.build_time_secs * 10.0;
        storage_cost + write_cost + build_cost
    }

    /// Format storage size as human-readable string.
    #[must_use]
    pub fn format_storage(&self) -> String {
        if self.storage_bytes < 1024 {
            format!("{} B", self.storage_bytes)
        } else if self.storage_bytes < 1024 * 1024 {
            format!(
                "{:.1} KB",
                self.storage_bytes as f64 / 1024.0
            )
        } else if self.storage_bytes < 1024 * 1024 * 1024 {
            format!(
                "{:.1} MB",
                self.storage_bytes as f64 / (1024.0 * 1024.0)
            )
        } else {
            format!(
                "{:.1} GB",
                self.storage_bytes as f64
                    / (1024.0 * 1024.0 * 1024.0)
            )
        }
    }

    /// Format write overhead as percentage.
    #[must_use]
    pub fn format_write_overhead(&self) -> String {
        format!("{:.1}%", self.write_overhead * 100.0)
    }

    /// Format build time as human-readable duration.
    #[must_use]
    pub fn format_build_time(&self) -> String {
        if self.build_time_secs < 1.0 {
            format!("{:.0} ms", self.build_time_secs * 1000.0)
        } else if self.build_time_secs < 60.0 {
            format!("{:.1} s", self.build_time_secs)
        } else if self.build_time_secs < 3600.0 {
            format!("{:.1} min", self.build_time_secs / 60.0)
        } else {
            format!("{:.1} h", self.build_time_secs / 3600.0)
        }
    }

    /// Format cost as human-readable string.
    #[must_use]
    pub fn format_cost(&self) -> String {
        format!(
            "{} storage, {} write overhead, {} build time",
            self.format_storage(),
            self.format_write_overhead(),
            self.format_build_time()
        )
    }
}

/// Estimate BRIN effectiveness based on column correlation and query
/// selectivity. Returns the fraction of the table that BRIN can skip
/// (0.0 = no benefit, 1.0 = perfect skip).
#[must_use]
pub fn estimate_brin_effectiveness(
    correlation: f64,
    table_pages: u64,
    pages_per_range: u32,
    selectivity: f64,
) -> f64 {
    if table_pages == 0 || pages_per_range == 0 {
        return 0.0;
    }

    let n_ranges = table_pages / pages_per_range as u64;
    if n_ranges == 0 {
        return 0.0;
    }

    // With perfect correlation, selectivity directly maps to ranges
    // scanned. With no correlation, all ranges must be scanned.
    let abs_corr = correlation.abs();
    let range_selectivity =
        selectivity * abs_corr + (1.0 - abs_corr);

    let scanned_ranges =
        (n_ranges as f64 * range_selectivity).ceil() as u64;
    let scanned_pages =
        scanned_ranges.min(table_pages) * pages_per_range as u64;

    // Effectiveness = fraction of table NOT scanned
    1.0 - (scanned_pages as f64 / table_pages as f64).min(1.0)
}

/// Compare BRIN vs B-tree storage cost for a given table.
/// Returns the storage savings ratio (e.g., 300.0 means BRIN is 300x
/// smaller).
#[must_use]
pub fn brin_storage_savings(table_size_bytes: u64) -> f64 {
    let btree_size = table_size_bytes as f64 * BTREE_SIZE_RATIO;
    let brin_size = table_size_bytes as f64 * BRIN_SIZE_RATIO;
    if brin_size > 0.0 {
        btree_size / brin_size
    } else {
        0.0
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn benefit_calculation() {
        let benefit = IndexBenefit::new(
            vec!["q1".into(), "q2".into()],
            5.0,
            1000.0,
        );
        assert!(benefit.has_benefit());
        assert_eq!(benefit.affected_queries.len(), 2);
        assert!((benefit.avg_speedup - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn benefit_no_queries() {
        let benefit = IndexBenefit::new(vec![], 1.0, 0.0);
        assert!(!benefit.has_benefit());
    }

    #[test]
    fn cost_calculation() {
        let cost = IndexCost::new(
            10 * 1024 * 1024,
            0.05,
            2.5,
        );
        assert_eq!(cost.format_storage(), "10.0 MB");
        assert_eq!(cost.format_write_overhead(), "5.0%");
        assert_eq!(cost.format_build_time(), "2.5 s");
        // Total: 10 (storage) + 50 (write) + 25 (build) = 85
        assert!((cost.total() - 85.0).abs() < 0.01);
    }

    #[test]
    fn format_sizes() {
        assert_eq!(
            IndexCost::new(500, 0.0, 0.0).format_storage(),
            "500 B"
        );
        assert_eq!(
            IndexCost::new(2048, 0.0, 0.0).format_storage(),
            "2.0 KB"
        );
        assert_eq!(
            IndexCost::new(5 * 1024 * 1024, 0.0, 0.0)
                .format_storage(),
            "5.0 MB"
        );
        assert_eq!(
            IndexCost::new(2 * 1024 * 1024 * 1024, 0.0, 0.0)
                .format_storage(),
            "2.0 GB"
        );
    }

    #[test]
    fn format_times() {
        assert_eq!(
            IndexCost::new(0, 0.0, 0.5).format_build_time(),
            "500 ms"
        );
        assert_eq!(
            IndexCost::new(0, 0.0, 30.0).format_build_time(),
            "30.0 s"
        );
        assert_eq!(
            IndexCost::new(0, 0.0, 120.0).format_build_time(),
            "2.0 min"
        );
        assert_eq!(
            IndexCost::new(0, 0.0, 7200.0).format_build_time(),
            "2.0 h"
        );
    }

    #[test]
    fn brin_cost_much_smaller_than_btree() {
        let table_size = 1_000_000_000; // 1 GB table
        let row_count = 10_000_000.0;

        let btree_cost = IndexCost::estimate(
            IndexType::BTree, table_size, row_count, 1,
        );
        let brin_cost = IndexCost::estimate(
            IndexType::BRIN, table_size, row_count, 1,
        );

        // BRIN storage should be ~300x smaller
        assert!(
            btree_cost.storage_bytes > brin_cost.storage_bytes * 100
        );
        // BRIN write overhead should be much lower
        assert!(btree_cost.write_overhead > brin_cost.write_overhead);
        // BRIN build time should be faster
        assert!(
            btree_cost.build_time_secs > brin_cost.build_time_secs
        );
    }

    #[test]
    fn brin_effectiveness_perfect_correlation() {
        let eff = estimate_brin_effectiveness(
            1.0, 10_000, 128, 0.01,
        );
        // With perfect correlation and 1% selectivity, should skip
        // ~99% of ranges
        assert!(eff > 0.95);
    }

    #[test]
    fn brin_effectiveness_no_correlation() {
        let eff = estimate_brin_effectiveness(
            0.0, 10_000, 128, 0.01,
        );
        // With no correlation, BRIN provides no benefit
        assert!(eff < 0.01);
    }

    #[test]
    fn brin_effectiveness_high_correlation() {
        let eff = estimate_brin_effectiveness(
            0.95, 10_000, 128, 0.10,
        );
        // With 0.95 correlation and 10% selectivity, good benefit
        assert!(eff > 0.5);
    }

    #[test]
    fn brin_effectiveness_negative_correlation() {
        let eff = estimate_brin_effectiveness(
            -0.98, 10_000, 128, 0.05,
        );
        // Negative correlation is equally useful for BRIN
        assert!(eff > 0.8);
    }

    #[test]
    fn brin_effectiveness_zero_pages() {
        let eff = estimate_brin_effectiveness(1.0, 0, 128, 0.01);
        assert!(eff.abs() < f64::EPSILON);
    }

    #[test]
    fn brin_storage_savings_ratio() {
        let savings = brin_storage_savings(1_000_000_000);
        // B-tree is ~0.3x table, BRIN is ~0.001x table
        // Ratio should be ~300x
        assert!((savings - 300.0).abs() < 1.0);
    }
}
