//! Time-series database optimization rules.
//!
//! Provides operators and cost models for time-series query patterns
//! used by databases like `InfluxDB`, `TimescaleDB`, `QuestDB`, and
//! `ClickHouse`. Key optimizations include time-range pruning,
//! downsampling pushdown, and last-point queries.

use serde::{Deserialize, Serialize};

use ra_core::cost::Cost;

/// Time-series-specific operators that extend the relational algebra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimeSeriesOp {
    /// Scan a hypertable with time-range pruning.
    ChunkScan {
        /// Table name.
        table: String,
        /// Time column name.
        time_column: String,
        /// Start of the time range (inclusive).
        range_start: String,
        /// End of the time range (exclusive).
        range_end: String,
    },

    /// Scan a continuous aggregate (pre-computed rollup).
    ContinuousAggregateScan {
        /// Base table name.
        table: String,
        /// Bucket interval (e.g., "1 hour").
        interval: String,
    },

    /// Last-point query: most recent row per series.
    LastPoint {
        /// Table name.
        table: String,
        /// Series (grouping) column.
        series_column: String,
        /// Time column name.
        time_column: String,
    },

    /// Gap-filled aggregation.
    GapFilledAggregate {
        /// Table name.
        table: String,
        /// Time column.
        time_column: String,
        /// Bucket interval.
        interval: String,
        /// Gap fill method.
        fill_method: GapFillMethod,
    },

    /// Tag-based series filter scan.
    TagScan {
        /// Table name.
        table: String,
        /// Tag column name.
        tag: String,
        /// Tag value to filter on.
        value: String,
    },

    /// Delta-encoded column direct scan.
    DeltaScan {
        /// Table name.
        table: String,
        /// Column to read deltas from.
        column: String,
    },

    /// Aligned aggregation with chunk-parallel execution.
    AlignedAggregate {
        /// Table name.
        table: String,
        /// Bucket interval.
        interval: String,
        /// Aggregate functions.
        functions: Vec<String>,
    },
}

/// Method for filling gaps in time-series data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GapFillMethod {
    /// Fill with NULL values.
    Null,
    /// Last observation carried forward.
    Locf,
    /// Linear interpolation between known points.
    Linear,
    /// Fill with a constant value.
    Constant,
}

/// Statistics about a time-series table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    /// Total number of rows.
    pub row_count: f64,
    /// Number of distinct time series.
    pub series_count: f64,
    /// Average rows per series.
    pub avg_rows_per_series: f64,
    /// Number of time-based chunks.
    pub chunk_count: u32,
    /// Chunk interval (e.g., "1 day", "1 hour").
    pub chunk_interval: String,
    /// Retention policy duration, if set.
    pub retention: Option<String>,
    /// Available continuous aggregates.
    pub continuous_aggregates: Vec<CaggInfo>,
    /// Tag columns.
    pub tag_columns: Vec<String>,
    /// Field (metric) columns.
    pub field_columns: Vec<String>,
}

/// Metadata about a continuous aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaggInfo {
    /// The bucket interval of this aggregate.
    pub interval: String,
    /// Aggregate functions available.
    pub functions: Vec<String>,
    /// Whether the aggregate is up to date.
    pub is_refreshed: bool,
}

/// Convert a non-negative f64 to u64 for memory estimates, clamping
/// negative and overflow values.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn f64_to_mem(val: f64) -> u64 {
    if val <= 0.0 {
        0
    } else if val >= u64::MAX as f64 {
        u64::MAX
    } else {
        val as u64
    }
}

/// Estimate cost for a chunk scan with time-range pruning.
///
/// The `total_chunks` parameter accounts for the metadata lookup cost
/// of checking each chunk's boundaries against the query range.
#[must_use]
pub fn estimate_chunk_scan_cost(
    total_chunks: u32,
    matching_chunks: u32,
    rows_per_chunk: f64,
) -> Cost {
    let rows = f64::from(matching_chunks) * rows_per_chunk;
    let metadata_cost = f64::from(total_chunks) * 0.001;
    Cost::new(
        rows * 0.05 + metadata_cost,
        rows * 0.01,
        0.0,
        f64_to_mem(rows * 32.0),
    )
}

/// Estimate cost for a continuous aggregate scan.
#[must_use]
pub fn estimate_cagg_scan_cost(bucket_count: f64) -> Cost {
    Cost::new(
        bucket_count * 0.02,
        bucket_count * 0.005,
        0.0,
        f64_to_mem(bucket_count * 64.0),
    )
}

/// Estimate cost for a last-point query.
#[must_use]
pub fn estimate_last_point_cost(series_count: f64) -> Cost {
    Cost::new(
        series_count * 0.1,
        series_count * 0.02,
        0.0,
        f64_to_mem(series_count * 128.0),
    )
}

/// Estimate cost for a tag-based scan.
#[must_use]
pub fn estimate_tag_scan_cost(matching_series: f64, rows_per_series: f64) -> Cost {
    let rows = matching_series * rows_per_series;
    Cost::new(rows * 0.05, rows * 0.01, 0.0, f64_to_mem(rows * 32.0))
}

/// Estimate cost for an aligned (chunk-parallel) aggregation.
#[must_use]
pub fn estimate_aligned_agg_cost(total_rows: f64, chunk_count: u32, parallelism: u32) -> Cost {
    let parallel_rows = total_rows / f64::from(parallelism);
    let merge_cost = f64::from(chunk_count);
    Cost::new(
        parallel_rows * 0.08 + merge_cost,
        parallel_rows * 0.01,
        0.0,
        f64_to_mem(parallel_rows * 16.0),
    )
}

/// Estimate cost for a gap-filled aggregation.
#[must_use]
pub fn estimate_gap_fill_cost(data_buckets: f64, total_buckets: f64) -> Cost {
    Cost::new(
        total_buckets * 0.03,
        data_buckets * 0.01,
        0.0,
        f64_to_mem(total_buckets * 48.0),
    )
}

/// Compare chunk scan vs. full scan cost to determine pruning benefit.
#[must_use]
pub fn pruning_benefit(total_chunks: u32, matching_chunks: u32) -> f64 {
    if total_chunks == 0 {
        return 0.0;
    }
    let pruned = total_chunks.saturating_sub(matching_chunks);
    f64::from(pruned) / f64::from(total_chunks)
}

impl std::fmt::Display for GapFillMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Locf => write!(f, "LOCF"),
            Self::Linear => write!(f, "LINEAR"),
            Self::Constant => write!(f, "CONSTANT"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn ts_op_chunk_scan_roundtrip() {
        let op = TimeSeriesOp::ChunkScan {
            table: "sensor_data".into(),
            time_column: "time".into(),
            range_start: "2025-03-01".into(),
            range_end: "2025-03-15".into(),
        };
        let json = serde_json::to_string(&op).expect("serialization should succeed");
        let deserialized: TimeSeriesOp =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(op, deserialized);
    }

    #[test]
    fn ts_op_last_point_roundtrip() {
        let op = TimeSeriesOp::LastPoint {
            table: "metrics".into(),
            series_column: "sensor_id".into(),
            time_column: "time".into(),
        };
        let json = serde_json::to_string(&op).expect("serialization should succeed");
        let deserialized: TimeSeriesOp =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(op, deserialized);
    }

    #[test]
    fn ts_op_gap_fill() {
        let op = TimeSeriesOp::GapFilledAggregate {
            table: "sensors".into(),
            time_column: "time".into(),
            interval: "1 hour".into(),
            fill_method: GapFillMethod::Locf,
        };
        if let TimeSeriesOp::GapFilledAggregate { fill_method, .. } = &op {
            assert_eq!(*fill_method, GapFillMethod::Locf);
        } else {
            panic!("expected GapFilledAggregate");
        }
    }

    #[test]
    fn gap_fill_method_display() {
        assert_eq!(GapFillMethod::Null.to_string(), "NULL");
        assert_eq!(GapFillMethod::Locf.to_string(), "LOCF");
        assert_eq!(GapFillMethod::Linear.to_string(), "LINEAR");
        assert_eq!(GapFillMethod::Constant.to_string(), "CONSTANT");
    }

    #[test]
    fn ts_stats_roundtrip() {
        let stats = TimeSeriesStats {
            row_count: 100_000_000.0,
            series_count: 10_000.0,
            avg_rows_per_series: 10_000.0,
            chunk_count: 365,
            chunk_interval: "1 day".into(),
            retention: Some("90 days".into()),
            continuous_aggregates: vec![CaggInfo {
                interval: "1 hour".into(),
                functions: vec!["avg".into(), "min".into(), "max".into()],
                is_refreshed: true,
            }],
            tag_columns: vec!["sensor_id".into(), "region".into()],
            field_columns: vec!["temperature".into(), "humidity".into()],
        };
        let json = serde_json::to_string(&stats).expect("serialization should succeed");
        let deserialized: TimeSeriesStats =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(stats, deserialized);
    }

    #[test]
    fn chunk_scan_cost_proportional() {
        let few = estimate_chunk_scan_cost(365, 7, 10_000.0);
        let many = estimate_chunk_scan_cost(365, 365, 10_000.0);
        assert!(many.total() > few.total());
    }

    #[test]
    fn pruning_benefit_all_pruned() {
        let benefit = pruning_benefit(100, 1);
        assert!((benefit - 0.99).abs() < f64::EPSILON);
    }

    #[test]
    fn pruning_benefit_none_pruned() {
        let benefit = pruning_benefit(100, 100);
        assert!((benefit - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn pruning_benefit_zero_chunks() {
        let benefit = pruning_benefit(0, 0);
        assert!((benefit - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cagg_much_cheaper_than_raw() {
        let raw = estimate_chunk_scan_cost(365, 365, 1_000_000.0);
        let cagg = estimate_cagg_scan_cost(8760.0);
        assert!(cagg.total() < raw.total());
    }

    #[test]
    fn last_point_cheaper_than_full_scan() {
        let full = estimate_chunk_scan_cost(365, 365, 100_000.0);
        let last_point = estimate_last_point_cost(10_000.0);
        assert!(last_point.total() < full.total());
    }

    #[test]
    fn tag_scan_selective() {
        let full = estimate_chunk_scan_cost(100, 100, 100_000.0);
        let tagged = estimate_tag_scan_cost(10.0, 100_000.0);
        assert!(tagged.total() < full.total());
    }

    #[test]
    fn aligned_agg_scales_with_parallelism() {
        let serial = estimate_aligned_agg_cost(1_000_000.0, 100, 1);
        let parallel = estimate_aligned_agg_cost(1_000_000.0, 100, 8);
        assert!(parallel.total() < serial.total());
    }

    #[test]
    fn gap_fill_cost_proportional_to_buckets() {
        let sparse = estimate_gap_fill_cost(10.0, 1000.0);
        let dense = estimate_gap_fill_cost(900.0, 1000.0);
        assert!(dense.io > sparse.io);
    }
}
