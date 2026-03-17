//! Statistics types catalog for query optimization.
//!
//! Models database statistics at multiple levels:
//! - Table-level: row counts, pages, size
//! - Column-level: NDV, null fraction, MCV, histograms
//! - Index: clustering, leaf pages, height
//! - Correlation: functional dependencies, multi-column NDV

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a column.
pub type ColumnId = String;

/// Table-level statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableStats {
    /// Total number of rows in the table.
    pub row_count: u64,
    /// Total number of pages (blocks) used.
    pub page_count: u64,
    /// Average row size in bytes.
    pub average_row_size: f64,
    /// Total table size in bytes.
    pub table_size_bytes: u64,
    /// Number of live tuples (MVCC systems).
    pub live_tuples: Option<u64>,
    /// Number of dead tuples (MVCC systems).
    pub dead_tuples: Option<u64>,
    /// Last time statistics were gathered.
    pub last_analyzed: Option<i64>,
}

/// Column-level statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    /// Column identifier.
    pub column_id: ColumnId,
    /// Number of distinct values (NDV/cardinality).
    pub ndv: u64,
    /// Fraction of NULL values (0.0 to 1.0).
    pub null_fraction: f64,
    /// Average column width in bytes.
    pub avg_width: f64,
    /// Most common values with frequencies.
    pub mcv: Option<MostCommonValues>,
    /// Histogram for value distribution.
    pub histogram: Option<Histogram>,
    /// Correlation with physical row order (-1.0 to 1.0).
    pub correlation: Option<f64>,
}

/// Most common values (MCV) list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MostCommonValues {
    /// List of (value, frequency) pairs.
    /// Frequency is a fraction from 0.0 to 1.0.
    pub values: Vec<(String, f64)>,
    /// Total fraction covered by MCV (0.0 to 1.0).
    pub total_fraction: f64,
}

/// Histogram for value distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Histogram {
    /// Equi-width histogram with bucket boundaries.
    EquiWidth {
        /// Bucket boundaries (sorted).
        boundaries: Vec<f64>,
        /// Counts per bucket.
        counts: Vec<u64>,
    },
    /// Equi-depth (equi-height) histogram.
    EquiDepth {
        /// Bucket boundaries (sorted).
        boundaries: Vec<f64>,
        /// Target rows per bucket.
        rows_per_bucket: u64,
    },
    /// End-biased histogram (`PostgreSQL` style).
    EndBiased {
        /// Most common values fraction.
        mcv_fraction: f64,
        /// Histogram boundaries for non-MCV values.
        boundaries: Vec<f64>,
    },
    /// T-Digest sketch for percentile estimation.
    TDigest {
        /// Centroids (mean, weight).
        centroids: Vec<(f64, u64)>,
    },
}

/// Index statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexStats {
    /// Index name or identifier.
    pub index_id: String,
    /// Clustering factor (how well index order matches table order).
    /// Range: 1.0 (perfect) to `row_count` (random).
    pub clustering_factor: f64,
    /// Number of leaf pages.
    pub leaf_pages: u64,
    /// Number of levels (tree height).
    pub levels: u32,
    /// Average leaf density (0.0 to 1.0).
    pub avg_leaf_density: f64,
    /// Number of distinct keys.
    pub distinct_keys: u64,
}

/// Functional dependency between columns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionalDependency {
    /// Determinant columns (left side).
    pub determinant: Vec<ColumnId>,
    /// Dependent columns (right side).
    pub dependent: Vec<ColumnId>,
    /// Confidence level (0.0 to 1.0).
    /// 1.0 = exact dependency, < 1.0 = soft/approximate.
    pub confidence: OrderedFloat,
}

/// Multi-column NDV for join cardinality estimation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiColumnNdv {
    /// Columns in the combination.
    pub columns: Vec<ColumnId>,
    /// Number of distinct value combinations.
    pub ndv: u64,
}

/// Correlation statistics between columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrelationStats {
    /// Functional dependencies.
    pub functional_dependencies: Vec<FunctionalDependency>,
    /// Multi-column NDVs.
    pub multi_column_ndvs: Vec<MultiColumnNdv>,
    /// Pearson correlation coefficients for numeric columns.
    pub correlations: HashMap<(ColumnId, ColumnId), f64>,
}

/// Join cardinality statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JoinStats {
    /// Left table columns.
    pub left_columns: Vec<ColumnId>,
    /// Right table columns.
    pub right_columns: Vec<ColumnId>,
    /// Estimated join cardinality.
    pub estimated_rows: u64,
    /// Join selectivity (0.0 to 1.0).
    pub selectivity: f64,
}

/// Predicate selectivity statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredicateStats {
    /// Predicate expression (simplified representation).
    pub predicate: String,
    /// Estimated selectivity (0.0 to 1.0).
    pub selectivity: f64,
}

/// Sketch-based statistics for approximate queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Sketch {
    /// `HyperLogLog` for NDV estimation.
    HyperLogLog {
        /// Precision parameter (4-16).
        precision: u8,
        /// Registers.
        registers: Vec<u8>,
    },
    /// Count-Min Sketch for frequency estimation.
    CountMinSketch {
        /// Width of the sketch.
        width: usize,
        /// Depth (number of hash functions).
        depth: usize,
        /// Counters.
        counters: Vec<Vec<u64>>,
    },
    /// Bloom filter for membership testing.
    BloomFilter {
        /// Bit array size.
        size: usize,
        /// Number of hash functions.
        num_hashes: usize,
        /// Bit array.
        bits: Vec<bool>,
    },
}

/// Time-series statistics for temporal data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    /// Column identifier.
    pub column_id: ColumnId,
    /// Minimum timestamp.
    pub min_time: i64,
    /// Maximum timestamp.
    pub max_time: i64,
    /// Average inter-arrival time.
    pub avg_interval: f64,
    /// Seasonality period (if detected).
    pub seasonality: Option<f64>,
}

/// Ordered float for use in Hash contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OrderedFloat(u64);

impl OrderedFloat {
    /// Create from f64.
    pub fn new(value: f64) -> Self {
        Self((value * 1000.0).round() as u64)
    }

    /// Convert to f64.
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

impl std::hash::Hash for OrderedFloat {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_stats_creation() {
        let stats = TableStats {
            row_count: 1_000_000,
            page_count: 10_000,
            average_row_size: 100.0,
            table_size_bytes: 100_000_000,
            live_tuples: Some(950_000),
            dead_tuples: Some(50_000),
            last_analyzed: Some(1_234_567_890),
        };
        assert_eq!(stats.row_count, 1_000_000);
        assert_eq!(stats.page_count, 10_000);
    }

    #[test]
    fn test_column_stats_with_histogram() {
        let histogram = Histogram::EquiWidth {
            boundaries: vec![0.0, 10.0, 20.0, 30.0],
            counts: vec![100, 200, 150],
        };
        let stats = ColumnStats {
            column_id: "col1".to_string(),
            ndv: 1000,
            null_fraction: 0.05,
            avg_width: 8.0,
            mcv: None,
            histogram: Some(histogram),
            correlation: Some(0.8),
        };
        assert_eq!(stats.ndv, 1000);
        assert_eq!(stats.null_fraction, 0.05);
    }

    #[test]
    fn test_mcv_total_fraction() {
        let mcv = MostCommonValues {
            values: vec![
                ("value1".to_string(), 0.3),
                ("value2".to_string(), 0.2),
                ("value3".to_string(), 0.15),
            ],
            total_fraction: 0.65,
        };
        assert_eq!(mcv.values.len(), 3);
        assert_eq!(mcv.total_fraction, 0.65);
    }

    #[test]
    fn test_functional_dependency() {
        let fd = FunctionalDependency {
            determinant: vec!["col_a".to_string()],
            dependent: vec!["col_b".to_string(), "col_c".to_string()],
            confidence: OrderedFloat::new(1.0),
        };
        assert_eq!(fd.determinant.len(), 1);
        assert_eq!(fd.dependent.len(), 2);
        assert_eq!(fd.confidence.to_f64(), 1.0);
    }

    #[test]
    fn test_ordered_float_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(OrderedFloat::new(0.5));
        set.insert(OrderedFloat::new(0.5));
        set.insert(OrderedFloat::new(0.7));
        assert_eq!(set.len(), 2);
    }
}
