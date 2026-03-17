//! Statistics types for cost estimation.
//!
//! Statistics describe the data distribution and cardinality of
//! tables and columns, providing the information cost models need
//! to estimate operator costs.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Statistics for a table or intermediate relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Statistics {
    /// Estimated number of rows.
    pub row_count: f64,
    /// Average row size in bytes.
    pub avg_row_size: u64,
    /// Total size on disk in bytes.
    pub total_size: u64,
    /// Per-column statistics, keyed by column name.
    pub columns: HashMap<String, ColumnStats>,
}

impl Statistics {
    /// Create statistics for a table with the given row count.
    #[must_use]
    pub fn new(row_count: f64) -> Self {
        Self {
            row_count,
            avg_row_size: 0,
            total_size: 0,
            columns: HashMap::new(),
        }
    }

    /// Estimate selectivity for a predicate, defaulting to a
    /// heuristic when column statistics are not available.
    ///
    /// Returns a value in `[0.0, 1.0]`.
    #[must_use]
    pub fn default_selectivity() -> f64 {
        0.1
    }
}

/// Statistics for a single column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnStats {
    /// Number of distinct values (NDV).
    pub distinct_count: f64,
    /// Fraction of NULL values in `[0.0, 1.0]`.
    pub null_fraction: f64,
    /// Minimum value (as a sortable string representation).
    pub min_value: Option<String>,
    /// Maximum value (as a sortable string representation).
    pub max_value: Option<String>,
    /// Average length in bytes (for variable-length columns).
    pub avg_length: Option<f64>,
    /// Optional histogram for value distribution.
    pub histogram: Option<Histogram>,
}

impl ColumnStats {
    /// Create column statistics with the given distinct count.
    #[must_use]
    pub fn new(distinct_count: f64) -> Self {
        Self {
            distinct_count,
            null_fraction: 0.0,
            min_value: None,
            max_value: None,
            avg_length: None,
            histogram: None,
        }
    }

    /// Estimate the selectivity of an equality predicate on this column.
    ///
    /// Uses `1 / distinct_count` when available, otherwise falls
    /// back to the default.
    #[must_use]
    pub fn equality_selectivity(&self) -> f64 {
        if self.distinct_count > 0.0 {
            1.0 / self.distinct_count
        } else {
            Statistics::default_selectivity()
        }
    }
}

/// A histogram describing value distribution for a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Histogram {
    /// An equi-width histogram with fixed-width buckets.
    EquiWidth(EquiWidthHistogram),
    /// An equi-depth histogram where each bucket has roughly the
    /// same number of rows.
    EquiDepth(EquiDepthHistogram),
}

/// An equi-width histogram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquiWidthHistogram {
    /// The buckets, each with a boundary and count.
    pub buckets: Vec<HistogramBucket>,
}

/// An equi-depth histogram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquiDepthHistogram {
    /// The buckets, each with a boundary and count.
    pub buckets: Vec<HistogramBucket>,
    /// The target number of rows per bucket.
    pub rows_per_bucket: f64,
}

/// A single histogram bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramBucket {
    /// The upper bound of this bucket (as a sortable string).
    pub upper_bound: String,
    /// The number of rows in this bucket.
    pub row_count: f64,
    /// The number of distinct values in this bucket.
    pub distinct_count: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_new() {
        let stats = Statistics::new(1000.0);
        assert_eq!(stats.row_count, 1000.0);
        assert!(stats.columns.is_empty());
    }

    #[test]
    fn default_selectivity() {
        let sel = Statistics::default_selectivity();
        assert!((sel - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn column_stats_equality_selectivity() {
        let cs = ColumnStats::new(100.0);
        let sel = cs.equality_selectivity();
        assert!((sel - 0.01).abs() < f64::EPSILON);
    }

    #[test]
    fn column_stats_zero_distinct() {
        let cs = ColumnStats::new(0.0);
        let sel = cs.equality_selectivity();
        assert!((sel - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn histogram_bucket_fields() {
        let bucket = HistogramBucket {
            upper_bound: "100".into(),
            row_count: 50.0,
            distinct_count: 25.0,
        };
        assert_eq!(bucket.upper_bound, "100");
        assert_eq!(bucket.row_count, 50.0);
        assert_eq!(bucket.distinct_count, 25.0);
    }

    #[test]
    fn equi_width_histogram() {
        let hist = Histogram::EquiWidth(EquiWidthHistogram {
            buckets: vec![
                HistogramBucket {
                    upper_bound: "50".into(),
                    row_count: 100.0,
                    distinct_count: 50.0,
                },
                HistogramBucket {
                    upper_bound: "100".into(),
                    row_count: 100.0,
                    distinct_count: 50.0,
                },
            ],
        });

        if let Histogram::EquiWidth(h) = &hist {
            assert_eq!(h.buckets.len(), 2);
        } else {
            panic!("expected EquiWidth variant");
        }
    }

    #[test]
    fn statistics_with_columns() {
        let mut stats = Statistics::new(500.0);
        stats
            .columns
            .insert("id".into(), ColumnStats::new(500.0));
        stats
            .columns
            .insert("name".into(), ColumnStats::new(200.0));

        assert_eq!(stats.columns.len(), 2);
        let id_stats =
            stats.columns.get("id").expect("id column should exist");
        assert_eq!(id_stats.distinct_count, 500.0);
    }

    #[test]
    fn serialize_roundtrip() {
        let mut stats = Statistics::new(100.0);
        stats.avg_row_size = 64;
        stats.total_size = 6400;
        stats
            .columns
            .insert("col".into(), ColumnStats::new(50.0));

        let json = serde_json::to_string(&stats)
            .expect("serialization should succeed");
        let deserialized: Statistics = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(stats, deserialized);
    }
}
