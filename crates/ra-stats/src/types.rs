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

/// Workload statistics for adaptive optimization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadStats {
    /// Query template frequencies (template hash -> count).
    pub query_frequencies: HashMap<String, u64>,
    /// Table access pattern counts (table name -> access count).
    pub table_access_patterns: HashMap<String, AccessPattern>,
    /// Columns frequently referenced in predicates or joins.
    pub hot_columns: Vec<HotColumn>,
}

/// Access pattern for a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessPattern {
    /// Number of sequential scans.
    pub seq_scans: u64,
    /// Number of index scans.
    pub index_scans: u64,
    /// Number of inserts.
    pub inserts: u64,
    /// Number of updates.
    pub updates: u64,
    /// Number of deletes.
    pub deletes: u64,
}

impl AccessPattern {
    /// Total read accesses.
    pub fn total_reads(&self) -> u64 {
        self.seq_scans + self.index_scans
    }

    /// Total write accesses.
    pub fn total_writes(&self) -> u64 {
        self.inserts + self.updates + self.deletes
    }

    /// Write ratio (0.0 = read-only, 1.0 = write-only).
    pub fn write_ratio(&self) -> f64 {
        let total = self.total_reads() + self.total_writes();
        if total == 0 {
            return 0.0;
        }
        self.total_writes() as f64 / total as f64
    }
}

/// A frequently-referenced column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotColumn {
    /// Table name.
    pub table: String,
    /// Column identifier.
    pub column_id: ColumnId,
    /// Number of times referenced in predicates.
    pub predicate_refs: u64,
    /// Number of times referenced in joins.
    pub join_refs: u64,
    /// Number of times referenced in group-by clauses.
    pub group_by_refs: u64,
}

impl TableStats {
    /// Dead tuple ratio (0.0 to 1.0).
    pub fn dead_tuple_ratio(&self) -> f64 {
        match (self.live_tuples, self.dead_tuples) {
            (Some(live), Some(dead)) if live + dead > 0 => {
                dead as f64 / (live + dead) as f64
            }
            _ => 0.0,
        }
    }

    /// Whether the table likely needs vacuuming.
    pub fn needs_vacuum(&self, threshold: f64) -> bool {
        self.dead_tuple_ratio() > threshold
    }

    /// Average bytes per page.
    pub fn fill_factor(&self) -> f64 {
        if self.page_count == 0 {
            return 0.0;
        }
        self.table_size_bytes as f64 / self.page_count as f64
    }
}

impl ColumnStats {
    /// Selectivity for an equality predicate (1/NDV adjusted for NULLs).
    pub fn equality_selectivity(&self) -> f64 {
        if self.ndv == 0 {
            return 1.0;
        }
        (1.0 - self.null_fraction) / self.ndv as f64
    }

    /// Selectivity estimate for a range predicate spanning `fraction`
    /// of the value domain.
    pub fn range_selectivity(&self, fraction: f64) -> f64 {
        (1.0 - self.null_fraction) * fraction.clamp(0.0, 1.0)
    }

    /// Whether the column is highly selective (many distinct values).
    pub fn is_high_cardinality(&self, row_count: u64) -> bool {
        if row_count == 0 {
            return false;
        }
        self.ndv as f64 / row_count as f64 > 0.9
    }
}

impl IndexStats {
    /// Whether the index has good clustering (factor close to 1.0).
    pub fn is_well_clustered(&self, row_count: u64) -> bool {
        if row_count == 0 {
            return true;
        }
        self.clustering_factor / (row_count as f64) < 0.1
    }

    /// Estimated pages to read for a range scan returning `selectivity`
    /// fraction of rows.
    pub fn range_scan_pages(&self, selectivity: f64, total_pages: u64) -> u64 {
        let selectivity = selectivity.clamp(0.0, 1.0);
        let min_pages = (total_pages as f64 * selectivity) as u64;
        let max_pages = total_pages;
        let factor = self.clustering_factor
            / (self.distinct_keys.max(1) as f64);
        let pages = min_pages as f64
            + (max_pages - min_pages) as f64 * factor.min(1.0);
        (pages as u64).min(max_pages).max(1)
    }
}

impl Histogram {
    /// Number of buckets in the histogram.
    pub fn bucket_count(&self) -> usize {
        match self {
            Self::EquiWidth { counts, .. } => counts.len(),
            Self::EquiDepth { boundaries, .. } => {
                boundaries.len().saturating_sub(1)
            }
            Self::EndBiased { boundaries, .. } => boundaries.len(),
            Self::TDigest { centroids, .. } => centroids.len(),
        }
    }

    /// Total row count represented by this histogram.
    pub fn total_rows(&self) -> u64 {
        match self {
            Self::EquiWidth { counts, .. } => counts.iter().sum(),
            Self::EquiDepth {
                boundaries,
                rows_per_bucket,
            } => {
                let buckets = boundaries.len().saturating_sub(1) as u64;
                buckets * rows_per_bucket
            }
            Self::EndBiased { .. } => 0,
            Self::TDigest { centroids, .. } => {
                centroids.iter().map(|(_, w)| w).sum()
            }
        }
    }
}

impl MostCommonValues {
    /// Get the frequency of a specific value, if present.
    pub fn frequency(&self, value: &str) -> Option<f64> {
        self.values
            .iter()
            .find(|(v, _)| v == value)
            .map(|(_, f)| *f)
    }

    /// Number of MCV entries.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the MCV list is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl WorkloadStats {
    /// Create empty workload stats.
    pub fn new() -> Self {
        Self {
            query_frequencies: HashMap::new(),
            table_access_patterns: HashMap::new(),
            hot_columns: Vec::new(),
        }
    }

    /// Record a query template occurrence.
    pub fn record_query(&mut self, template: &str) {
        *self
            .query_frequencies
            .entry(template.to_string())
            .or_insert(0) += 1;
    }

    /// Get the top-N most frequent query templates.
    pub fn top_queries(&self, n: usize) -> Vec<(&str, u64)> {
        let mut entries: Vec<(&str, u64)> = self
            .query_frequencies
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries.truncate(n);
        entries
    }
}

impl Default for WorkloadStats {
    fn default() -> Self {
        Self::new()
    }
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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    // ---- TableStats ----

    fn sample_table() -> TableStats {
        TableStats {
            row_count: 1_000_000,
            page_count: 10_000,
            average_row_size: 100.0,
            table_size_bytes: 100_000_000,
            live_tuples: Some(950_000),
            dead_tuples: Some(50_000),
            last_analyzed: Some(1_234_567_890),
        }
    }

    #[test]
    fn table_stats_fields() {
        let s = sample_table();
        assert_eq!(s.row_count, 1_000_000);
        assert_eq!(s.page_count, 10_000);
        assert_eq!(s.average_row_size, 100.0);
        assert_eq!(s.table_size_bytes, 100_000_000);
    }

    #[test]
    fn table_stats_live_dead_tuples() {
        let s = sample_table();
        assert_eq!(s.live_tuples, Some(950_000));
        assert_eq!(s.dead_tuples, Some(50_000));
    }

    #[test]
    fn table_dead_tuple_ratio() {
        let s = sample_table();
        let ratio = s.dead_tuple_ratio();
        assert!((ratio - 0.05).abs() < 0.001);
    }

    #[test]
    fn table_dead_tuple_ratio_no_tuples() {
        let s = TableStats {
            live_tuples: None,
            dead_tuples: None,
            ..sample_table()
        };
        assert_eq!(s.dead_tuple_ratio(), 0.0);
    }

    #[test]
    fn table_dead_tuple_ratio_zero_total() {
        let s = TableStats {
            live_tuples: Some(0),
            dead_tuples: Some(0),
            ..sample_table()
        };
        assert_eq!(s.dead_tuple_ratio(), 0.0);
    }

    #[test]
    fn table_needs_vacuum_true() {
        let s = sample_table();
        assert!(s.needs_vacuum(0.01));
    }

    #[test]
    fn table_needs_vacuum_false() {
        let s = sample_table();
        assert!(!s.needs_vacuum(0.1));
    }

    #[test]
    fn table_fill_factor() {
        let s = sample_table();
        assert_eq!(s.fill_factor(), 10_000.0);
    }

    #[test]
    fn table_fill_factor_zero_pages() {
        let s = TableStats {
            page_count: 0,
            ..sample_table()
        };
        assert_eq!(s.fill_factor(), 0.0);
    }

    #[test]
    fn table_stats_serialize_roundtrip() {
        let s = sample_table();
        let json = serde_json::to_string(&s)
            .expect("serialize");
        let d: TableStats = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(s, d);
    }

    // ---- ColumnStats ----

    fn sample_column() -> ColumnStats {
        ColumnStats {
            column_id: "col1".to_string(),
            ndv: 1000,
            null_fraction: 0.05,
            avg_width: 8.0,
            mcv: None,
            histogram: None,
            correlation: Some(0.8),
        }
    }

    #[test]
    fn column_stats_fields() {
        let c = sample_column();
        assert_eq!(c.ndv, 1000);
        assert_eq!(c.null_fraction, 0.05);
        assert_eq!(c.avg_width, 8.0);
    }

    #[test]
    fn column_equality_selectivity() {
        let c = sample_column();
        let sel = c.equality_selectivity();
        assert!((sel - 0.00095).abs() < 0.0001);
    }

    #[test]
    fn column_equality_selectivity_zero_ndv() {
        let c = ColumnStats { ndv: 0, ..sample_column() };
        assert_eq!(c.equality_selectivity(), 1.0);
    }

    #[test]
    fn column_equality_selectivity_all_null() {
        let c = ColumnStats {
            ndv: 10,
            null_fraction: 1.0,
            ..sample_column()
        };
        assert_eq!(c.equality_selectivity(), 0.0);
    }

    #[test]
    fn column_range_selectivity_full() {
        let c = sample_column();
        let sel = c.range_selectivity(1.0);
        assert!((sel - 0.95).abs() < 0.001);
    }

    #[test]
    fn column_range_selectivity_half() {
        let c = sample_column();
        let sel = c.range_selectivity(0.5);
        assert!((sel - 0.475).abs() < 0.001);
    }

    #[test]
    fn column_range_selectivity_clamps() {
        let c = sample_column();
        assert_eq!(c.range_selectivity(-0.5), 0.0);
        assert_eq!(c.range_selectivity(1.5), c.range_selectivity(1.0));
    }

    #[test]
    fn column_is_high_cardinality() {
        let c = ColumnStats { ndv: 950, ..sample_column() };
        assert!(c.is_high_cardinality(1000));
    }

    #[test]
    fn column_is_not_high_cardinality() {
        let c = ColumnStats { ndv: 10, ..sample_column() };
        assert!(!c.is_high_cardinality(1000));
    }

    #[test]
    fn column_high_cardinality_zero_rows() {
        let c = sample_column();
        assert!(!c.is_high_cardinality(0));
    }

    #[test]
    fn column_stats_with_histogram() {
        let histogram = Histogram::EquiWidth {
            boundaries: vec![0.0, 10.0, 20.0, 30.0],
            counts: vec![100, 200, 150],
        };
        let c = ColumnStats {
            histogram: Some(histogram),
            ..sample_column()
        };
        assert!(c.histogram.is_some());
    }

    #[test]
    fn column_stats_serialize_roundtrip() {
        let c = sample_column();
        let json = serde_json::to_string(&c).expect("serialize");
        let d: ColumnStats = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(c, d);
    }

    // ---- MostCommonValues ----

    fn sample_mcv() -> MostCommonValues {
        MostCommonValues {
            values: vec![
                ("value1".to_string(), 0.3),
                ("value2".to_string(), 0.2),
                ("value3".to_string(), 0.15),
            ],
            total_fraction: 0.65,
        }
    }

    #[test]
    fn mcv_total_fraction() {
        let mcv = sample_mcv();
        assert_eq!(mcv.total_fraction, 0.65);
    }

    #[test]
    fn mcv_len() {
        assert_eq!(sample_mcv().len(), 3);
    }

    #[test]
    fn mcv_is_empty() {
        assert!(!sample_mcv().is_empty());
        let empty = MostCommonValues {
            values: vec![],
            total_fraction: 0.0,
        };
        assert!(empty.is_empty());
    }

    #[test]
    fn mcv_frequency_found() {
        assert_eq!(sample_mcv().frequency("value1"), Some(0.3));
    }

    #[test]
    fn mcv_frequency_not_found() {
        assert_eq!(sample_mcv().frequency("missing"), None);
    }

    // ---- Histogram ----

    #[test]
    fn histogram_equi_width_bucket_count() {
        let h = Histogram::EquiWidth {
            boundaries: vec![0.0, 10.0, 20.0],
            counts: vec![100, 200],
        };
        assert_eq!(h.bucket_count(), 2);
    }

    #[test]
    fn histogram_equi_width_total_rows() {
        let h = Histogram::EquiWidth {
            boundaries: vec![0.0, 10.0, 20.0],
            counts: vec![100, 200],
        };
        assert_eq!(h.total_rows(), 300);
    }

    #[test]
    fn histogram_equi_depth_bucket_count() {
        let h = Histogram::EquiDepth {
            boundaries: vec![0.0, 10.0, 20.0, 30.0],
            rows_per_bucket: 100,
        };
        assert_eq!(h.bucket_count(), 3);
    }

    #[test]
    fn histogram_equi_depth_total_rows() {
        let h = Histogram::EquiDepth {
            boundaries: vec![0.0, 10.0, 20.0, 30.0],
            rows_per_bucket: 100,
        };
        assert_eq!(h.total_rows(), 300);
    }

    #[test]
    fn histogram_end_biased_bucket_count() {
        let h = Histogram::EndBiased {
            mcv_fraction: 0.3,
            boundaries: vec![10.0, 20.0, 30.0, 40.0, 50.0],
        };
        assert_eq!(h.bucket_count(), 5);
    }

    #[test]
    fn histogram_tdigest_bucket_count() {
        let h = Histogram::TDigest {
            centroids: vec![(5.0, 100), (15.0, 200)],
        };
        assert_eq!(h.bucket_count(), 2);
    }

    #[test]
    fn histogram_tdigest_total_rows() {
        let h = Histogram::TDigest {
            centroids: vec![(5.0, 100), (15.0, 200)],
        };
        assert_eq!(h.total_rows(), 300);
    }

    // ---- IndexStats ----

    fn sample_index() -> IndexStats {
        IndexStats {
            index_id: "idx_pk".to_string(),
            clustering_factor: 100.0,
            leaf_pages: 500,
            levels: 3,
            avg_leaf_density: 0.7,
            distinct_keys: 100_000,
        }
    }

    #[test]
    fn index_stats_fields() {
        let idx = sample_index();
        assert_eq!(idx.levels, 3);
        assert_eq!(idx.leaf_pages, 500);
    }

    #[test]
    fn index_well_clustered() {
        let idx = sample_index();
        assert!(idx.is_well_clustered(100_000));
    }

    #[test]
    fn index_poorly_clustered() {
        let idx = IndexStats {
            clustering_factor: 50_000.0,
            ..sample_index()
        };
        assert!(!idx.is_well_clustered(100_000));
    }

    #[test]
    fn index_well_clustered_zero_rows() {
        let idx = sample_index();
        assert!(idx.is_well_clustered(0));
    }

    #[test]
    fn index_range_scan_pages_full() {
        let idx = sample_index();
        let pages = idx.range_scan_pages(1.0, 10_000);
        assert!(pages <= 10_000);
        assert!(pages > 0);
    }

    #[test]
    fn index_range_scan_pages_small_selectivity() {
        let idx = sample_index();
        let pages = idx.range_scan_pages(0.01, 10_000);
        assert!(pages < 10_000);
        assert!(pages >= 1);
    }

    #[test]
    fn index_range_scan_pages_clamps() {
        let idx = sample_index();
        let p1 = idx.range_scan_pages(-0.5, 10_000);
        let p2 = idx.range_scan_pages(0.0, 10_000);
        assert_eq!(p1, p2);
    }

    // ---- CorrelationStats ----

    #[test]
    fn correlation_stats_creation() {
        let cs = CorrelationStats {
            functional_dependencies: vec![],
            multi_column_ndvs: vec![],
            correlations: HashMap::new(),
        };
        assert!(cs.functional_dependencies.is_empty());
    }

    #[test]
    fn correlation_stats_with_fd() {
        let cs = CorrelationStats {
            functional_dependencies: vec![FunctionalDependency {
                determinant: vec!["a".to_string()],
                dependent: vec!["b".to_string()],
                confidence: OrderedFloat::new(0.95),
            }],
            multi_column_ndvs: vec![],
            correlations: HashMap::new(),
        };
        assert_eq!(cs.functional_dependencies.len(), 1);
    }

    // ---- FunctionalDependency ----

    #[test]
    fn functional_dependency_exact() {
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
    fn functional_dependency_approximate() {
        let fd = FunctionalDependency {
            determinant: vec!["zip".to_string()],
            dependent: vec!["city".to_string()],
            confidence: OrderedFloat::new(0.85),
        };
        assert!((fd.confidence.to_f64() - 0.85).abs() < 0.001);
    }

    // ---- MultiColumnNdv ----

    #[test]
    fn multi_column_ndv_creation() {
        let mc = MultiColumnNdv {
            columns: vec!["a".to_string(), "b".to_string()],
            ndv: 5000,
        };
        assert_eq!(mc.columns.len(), 2);
        assert_eq!(mc.ndv, 5000);
    }

    // ---- JoinStats ----

    #[test]
    fn join_stats_creation() {
        let js = JoinStats {
            left_columns: vec!["id".to_string()],
            right_columns: vec!["user_id".to_string()],
            estimated_rows: 100_000,
            selectivity: 0.01,
        };
        assert_eq!(js.estimated_rows, 100_000);
        assert_eq!(js.selectivity, 0.01);
    }

    // ---- PredicateStats ----

    #[test]
    fn predicate_stats_creation() {
        let ps = PredicateStats {
            predicate: "age > 30".to_string(),
            selectivity: 0.4,
        };
        assert_eq!(ps.selectivity, 0.4);
    }

    // ---- Sketch ----

    #[test]
    fn sketch_hll() {
        let s = Sketch::HyperLogLog {
            precision: 14,
            registers: vec![0; 16384],
        };
        if let Sketch::HyperLogLog { precision, registers } = &s {
            assert_eq!(*precision, 14);
            assert_eq!(registers.len(), 16384);
        }
    }

    #[test]
    fn sketch_count_min() {
        let s = Sketch::CountMinSketch {
            width: 1000,
            depth: 5,
            counters: vec![vec![0; 1000]; 5],
        };
        if let Sketch::CountMinSketch { width, depth, .. } = &s {
            assert_eq!(*width, 1000);
            assert_eq!(*depth, 5);
        }
    }

    #[test]
    fn sketch_bloom_filter() {
        let s = Sketch::BloomFilter {
            size: 10_000,
            num_hashes: 7,
            bits: vec![false; 10_000],
        };
        if let Sketch::BloomFilter { size, num_hashes, .. } = &s {
            assert_eq!(*size, 10_000);
            assert_eq!(*num_hashes, 7);
        }
    }

    // ---- TimeSeriesStats ----

    #[test]
    fn time_series_stats_creation() {
        let ts = TimeSeriesStats {
            column_id: "created_at".to_string(),
            min_time: 1_000_000,
            max_time: 2_000_000,
            avg_interval: 60.0,
            seasonality: Some(86400.0),
        };
        assert_eq!(ts.avg_interval, 60.0);
        assert_eq!(ts.seasonality, Some(86400.0));
    }

    #[test]
    fn time_series_no_seasonality() {
        let ts = TimeSeriesStats {
            column_id: "ts".to_string(),
            min_time: 0,
            max_time: 1000,
            avg_interval: 1.0,
            seasonality: None,
        };
        assert!(ts.seasonality.is_none());
    }

    // ---- WorkloadStats ----

    #[test]
    fn workload_stats_new_empty() {
        let ws = WorkloadStats::new();
        assert!(ws.query_frequencies.is_empty());
        assert!(ws.table_access_patterns.is_empty());
        assert!(ws.hot_columns.is_empty());
    }

    #[test]
    fn workload_stats_default() {
        let ws = WorkloadStats::default();
        assert!(ws.query_frequencies.is_empty());
    }

    #[test]
    fn workload_record_query() {
        let mut ws = WorkloadStats::new();
        ws.record_query("SELECT * FROM users");
        ws.record_query("SELECT * FROM users");
        ws.record_query("SELECT * FROM orders");
        assert_eq!(
            ws.query_frequencies.get("SELECT * FROM users"),
            Some(&2)
        );
        assert_eq!(
            ws.query_frequencies.get("SELECT * FROM orders"),
            Some(&1)
        );
    }

    #[test]
    fn workload_top_queries() {
        let mut ws = WorkloadStats::new();
        for _ in 0..10 {
            ws.record_query("q1");
        }
        for _ in 0..5 {
            ws.record_query("q2");
        }
        ws.record_query("q3");
        let top = ws.top_queries(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "q1");
        assert_eq!(top[0].1, 10);
        assert_eq!(top[1].0, "q2");
        assert_eq!(top[1].1, 5);
    }

    #[test]
    fn workload_top_queries_empty() {
        let ws = WorkloadStats::new();
        let top = ws.top_queries(5);
        assert!(top.is_empty());
    }

    // ---- AccessPattern ----

    #[test]
    fn access_pattern_total_reads() {
        let ap = AccessPattern {
            seq_scans: 100,
            index_scans: 200,
            inserts: 10,
            updates: 5,
            deletes: 2,
        };
        assert_eq!(ap.total_reads(), 300);
    }

    #[test]
    fn access_pattern_total_writes() {
        let ap = AccessPattern {
            seq_scans: 100,
            index_scans: 200,
            inserts: 10,
            updates: 5,
            deletes: 2,
        };
        assert_eq!(ap.total_writes(), 17);
    }

    #[test]
    fn access_pattern_write_ratio_zero() {
        let ap = AccessPattern {
            seq_scans: 0,
            index_scans: 0,
            inserts: 0,
            updates: 0,
            deletes: 0,
        };
        assert_eq!(ap.write_ratio(), 0.0);
    }

    #[test]
    fn access_pattern_write_ratio_all_reads() {
        let ap = AccessPattern {
            seq_scans: 100,
            index_scans: 100,
            inserts: 0,
            updates: 0,
            deletes: 0,
        };
        assert_eq!(ap.write_ratio(), 0.0);
    }

    #[test]
    fn access_pattern_write_ratio_mixed() {
        let ap = AccessPattern {
            seq_scans: 50,
            index_scans: 50,
            inserts: 50,
            updates: 25,
            deletes: 25,
        };
        assert_eq!(ap.write_ratio(), 0.5);
    }

    // ---- HotColumn ----

    #[test]
    fn hot_column_creation() {
        let hc = HotColumn {
            table: "users".to_string(),
            column_id: "email".to_string(),
            predicate_refs: 100,
            join_refs: 50,
            group_by_refs: 10,
        };
        assert_eq!(hc.predicate_refs, 100);
    }

    // ---- OrderedFloat ----

    #[test]
    fn ordered_float_roundtrip() {
        let of = OrderedFloat::new(0.123);
        assert!((of.to_f64() - 0.123).abs() < 0.001);
    }

    #[test]
    fn ordered_float_hash_dedup() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(OrderedFloat::new(0.5));
        set.insert(OrderedFloat::new(0.5));
        set.insert(OrderedFloat::new(0.7));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn ordered_float_ordering() {
        let a = OrderedFloat::new(0.3);
        let b = OrderedFloat::new(0.7);
        assert!(a < b);
    }

    #[test]
    fn ordered_float_zero() {
        let z = OrderedFloat::new(0.0);
        assert_eq!(z.to_f64(), 0.0);
    }
}
