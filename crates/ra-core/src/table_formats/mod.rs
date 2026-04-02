//! Table format abstractions for Iceberg, Hudi, and Delta Lake.
//!
//! Modern data lakehouses use table formats that sit atop columnar
//! file formats (Parquet, ORC) and add transactional semantics,
//! schema evolution, and partition management. This module provides
//! the [`TableFormat`] trait so the optimizer can leverage metadata
//! from these formats without coupling to a specific implementation.
//!
//! # Architecture
//!
//! ```text
//! TableFormat (Iceberg / Hudi / Delta)
//!   └─ manages ─-> DataFile[]
//!       └─ each file has ─-> FileStats (min/max/null/row counts)
//!                          ─-> PartitionSpec (partition columns)
//! ```

pub mod iceberg;

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Errors that can occur when interacting with a table format.
#[derive(Debug, thiserror::Error)]
pub enum TableFormatError {
    /// The requested table was not found.
    #[error("table not found: {name}")]
    TableNotFound {
        /// Name of the missing table.
        name: String,
    },

    /// A metadata read failed.
    #[error("metadata error for {table}: {message}")]
    Metadata {
        /// Table whose metadata could not be read.
        table: String,
        /// Human-readable explanation.
        message: String,
    },

    /// The operation is not supported by this format.
    #[error("unsupported operation on {format}: {operation}")]
    Unsupported {
        /// Format name (e.g. "iceberg").
        format: String,
        /// Operation that was attempted.
        operation: String,
    },

    /// An I/O or transport error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias used throughout this module.
pub type Result<T> = std::result::Result<T, TableFormatError>;

/// Identifies the table format family.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
)]
pub enum TableFormatType {
    /// Apache Iceberg.
    Iceberg,
    /// Apache Hudi.
    Hudi,
    /// Delta Lake.
    DeltaLake,
    /// No table format layer (plain files).
    Traditional,
}

impl fmt::Display for TableFormatType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Iceberg => write!(f, "iceberg"),
            Self::Hudi => write!(f, "hudi"),
            Self::DeltaLake => write!(f, "delta_lake"),
            Self::Traditional => write!(f, "traditional"),
        }
    }
}

/// A data file managed by a table format.
///
/// Each file typically corresponds to a single Parquet or ORC file
/// produced by a write operation. The table format tracks these
/// files along with their partition values and summary statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataFile {
    /// Path to the data file (relative or absolute).
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Total number of records in the file.
    pub record_count: u64,
    /// Partition column values for this file.
    ///
    /// Keys are partition column names; values are their
    /// string-encoded representations.
    pub partition_values: HashMap<String, String>,
    /// Columnar file format (e.g. "parquet", "orc").
    pub file_format: String,
}

impl DataFile {
    /// Create a new `DataFile` with the required fields.
    #[must_use]
    pub fn new(
        path: String,
        size_bytes: u64,
        record_count: u64,
    ) -> Self {
        Self {
            path,
            size_bytes,
            record_count,
            partition_values: HashMap::new(),
            file_format: "parquet".to_owned(),
        }
    }

    /// Builder-style setter for partition values.
    #[must_use]
    pub fn with_partition(
        mut self,
        column: String,
        value: String,
    ) -> Self {
        self.partition_values.insert(column, value);
        self
    }

    /// Builder-style setter for file format.
    #[must_use]
    pub fn with_file_format(mut self, format: String) -> Self {
        self.file_format = format;
        self
    }
}

/// Per-column statistics for a data file or row group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileColumnStats {
    /// Minimum value (string-encoded for portability).
    pub min: Option<String>,
    /// Maximum value (string-encoded for portability).
    pub max: Option<String>,
    /// Number of NULL values.
    pub null_count: u64,
    /// Distinct value count (when available).
    pub distinct_count: Option<u64>,
}

/// Aggregate statistics for an entire data file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileStats {
    /// Total record count.
    pub record_count: u64,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Per-column statistics, keyed by column name.
    pub column_stats: HashMap<String, FileColumnStats>,
}

/// Describes how a table is partitioned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionSpec {
    /// Unique identifier for this partition spec.
    pub spec_id: u32,
    /// The partition fields, in order.
    pub fields: Vec<PartitionField>,
}

/// A single field within a [`PartitionSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartitionField {
    /// Source column name.
    pub source_column: String,
    /// Transform applied to the source column.
    pub transform: PartitionTransform,
    /// Name of the resulting partition column.
    pub partition_name: String,
}

/// Transforms that can be applied to a source column to produce
/// partition values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionTransform {
    /// Use the source value directly.
    Identity,
    /// Truncate to the given width.
    Truncate(u32),
    /// Hash into the given number of buckets.
    Bucket(u32),
    /// Extract the year from a date/timestamp.
    Year,
    /// Extract year and month.
    Month,
    /// Extract the date (year-month-day).
    Day,
    /// Extract date and hour.
    Hour,
}

impl fmt::Display for PartitionTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity => write!(f, "identity"),
            Self::Truncate(w) => write!(f, "truncate({w})"),
            Self::Bucket(n) => write!(f, "bucket({n})"),
            Self::Year => write!(f, "year"),
            Self::Month => write!(f, "month"),
            Self::Day => write!(f, "day"),
            Self::Hour => write!(f, "hour"),
        }
    }
}

/// Snapshot metadata for time-travel queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Unique snapshot identifier.
    pub snapshot_id: u64,
    /// Millisecond timestamp when the snapshot was created.
    pub timestamp_ms: i64,
    /// Summary of the operation that produced this snapshot.
    pub summary: HashMap<String, String>,
}

/// Abstraction over table formats (Iceberg, Hudi, Delta Lake).
///
/// Implementations provide access to file listings, statistics,
/// partition specs, and snapshot history so the optimizer can
/// make format-aware decisions without depending on a specific
/// table format library at the core level.
pub trait TableFormat: Send + Sync + fmt::Debug {
    /// Return which format family this implementation belongs to.
    fn format_type(&self) -> TableFormatType;

    /// List all data files for the given table.
    ///
    /// # Errors
    ///
    /// Returns [`TableFormatError::TableNotFound`] when the table
    /// does not exist, or [`TableFormatError::Metadata`] on read
    /// failures.
    fn list_files(&self, table: &str) -> Result<Vec<DataFile>>;

    /// Return per-file statistics when available.
    fn file_statistics(
        &self,
        file: &DataFile,
    ) -> Option<FileStats>;

    /// Whether the format supports time-travel queries.
    fn supports_time_travel(&self) -> bool;

    /// Return the partition spec for the given table, if any.
    ///
    /// # Errors
    ///
    /// Returns [`TableFormatError::TableNotFound`] when the table
    /// does not exist.
    fn partition_spec(
        &self,
        table: &str,
    ) -> Result<Option<PartitionSpec>>;

    /// List available snapshots for time-travel.
    ///
    /// Returns an empty vec when time travel is not supported.
    ///
    /// # Errors
    ///
    /// Returns [`TableFormatError::TableNotFound`] when the table
    /// does not exist.
    fn list_snapshots(
        &self,
        table: &str,
    ) -> Result<Vec<Snapshot>>;

    /// Return the current snapshot id, if applicable.
    ///
    /// # Errors
    ///
    /// Returns [`TableFormatError::TableNotFound`] when the table
    /// does not exist.
    fn current_snapshot_id(
        &self,
        table: &str,
    ) -> Result<Option<u64>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::iceberg::IcebergFormat;

    #[test]
    fn table_format_type_display() {
        assert_eq!(TableFormatType::Iceberg.to_string(), "iceberg");
        assert_eq!(TableFormatType::Hudi.to_string(), "hudi");
        assert_eq!(
            TableFormatType::DeltaLake.to_string(),
            "delta_lake"
        );
        assert_eq!(
            TableFormatType::Traditional.to_string(),
            "traditional"
        );
    }

    #[test]
    fn data_file_builder() {
        let file = DataFile::new(
            "data/part-0001.parquet".to_owned(),
            1024 * 1024,
            50_000,
        )
        .with_partition("date".to_owned(), "2024-01-15".to_owned())
        .with_file_format("parquet".to_owned());

        assert_eq!(file.path, "data/part-0001.parquet");
        assert_eq!(file.size_bytes, 1024 * 1024);
        assert_eq!(file.record_count, 50_000);
        assert_eq!(
            file.partition_values.get("date"),
            Some(&"2024-01-15".to_owned())
        );
        assert_eq!(file.file_format, "parquet");
    }

    #[test]
    fn partition_transform_display() {
        assert_eq!(
            PartitionTransform::Identity.to_string(),
            "identity"
        );
        assert_eq!(
            PartitionTransform::Truncate(16).to_string(),
            "truncate(16)"
        );
        assert_eq!(
            PartitionTransform::Bucket(256).to_string(),
            "bucket(256)"
        );
        assert_eq!(PartitionTransform::Year.to_string(), "year");
        assert_eq!(PartitionTransform::Month.to_string(), "month");
        assert_eq!(PartitionTransform::Day.to_string(), "day");
        assert_eq!(PartitionTransform::Hour.to_string(), "hour");
    }

    #[test]
    fn serialize_roundtrip_data_file() {
        let file = DataFile::new(
            "s3://bucket/table/part-0001.parquet".to_owned(),
            2 * 1024 * 1024,
            100_000,
        )
        .with_partition(
            "region".to_owned(),
            "us-east-1".to_owned(),
        );

        let json = serde_json::to_string(&file)
            .expect("serialization should succeed");
        let deserialized: DataFile = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(file, deserialized);
    }

    #[test]
    fn serialize_roundtrip_partition_spec() {
        let spec = PartitionSpec {
            spec_id: 0,
            fields: vec![
                PartitionField {
                    source_column: "event_time".to_owned(),
                    transform: PartitionTransform::Day,
                    partition_name: "event_day".to_owned(),
                },
                PartitionField {
                    source_column: "region".to_owned(),
                    transform: PartitionTransform::Identity,
                    partition_name: "region".to_owned(),
                },
            ],
        };

        let json = serde_json::to_string(&spec)
            .expect("serialization should succeed");
        let deserialized: PartitionSpec =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(spec, deserialized);
    }

    #[test]
    fn serialize_roundtrip_snapshot() {
        let mut summary = HashMap::new();
        summary.insert(
            "operation".to_owned(),
            "append".to_owned(),
        );
        summary.insert(
            "added-records".to_owned(),
            "50000".to_owned(),
        );

        let snap = Snapshot {
            snapshot_id: 42,
            timestamp_ms: 1_700_000_000_000,
            summary,
        };

        let json = serde_json::to_string(&snap)
            .expect("serialization should succeed");
        let deserialized: Snapshot =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(snap, deserialized);
    }

    #[test]
    fn iceberg_stub_format_type() {
        let iceberg = IcebergFormat::new();
        assert_eq!(
            iceberg.format_type(),
            TableFormatType::Iceberg
        );
    }

    #[test]
    fn iceberg_stub_supports_time_travel() {
        let iceberg = IcebergFormat::new();
        assert!(iceberg.supports_time_travel());
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test code intentionally checks error case")]
    fn iceberg_stub_list_files_not_found() {
        let iceberg = IcebergFormat::new();
        let result = iceberg.list_files("nonexistent_table");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, TableFormatError::TableNotFound { .. })
        );
    }

    #[test]
    fn iceberg_stub_partition_spec_not_found() {
        let iceberg = IcebergFormat::new();
        let result = iceberg.partition_spec("nonexistent_table");
        assert!(result.is_err());
    }

    #[test]
    fn iceberg_stub_list_snapshots_not_found() {
        let iceberg = IcebergFormat::new();
        let result =
            iceberg.list_snapshots("nonexistent_table");
        assert!(result.is_err());
    }

    #[test]
    fn iceberg_stub_current_snapshot_not_found() {
        let iceberg = IcebergFormat::new();
        let result =
            iceberg.current_snapshot_id("nonexistent_table");
        assert!(result.is_err());
    }

    #[test]
    fn iceberg_with_registered_table() {
        let mut iceberg = IcebergFormat::new();
        let files = vec![
            DataFile::new(
                "data/part-0001.parquet".to_owned(),
                1024,
                500,
            ),
            DataFile::new(
                "data/part-0002.parquet".to_owned(),
                2048,
                1000,
            ),
        ];
        let spec = PartitionSpec {
            spec_id: 0,
            fields: vec![PartitionField {
                source_column: "date".to_owned(),
                transform: PartitionTransform::Day,
                partition_name: "date_day".to_owned(),
            }],
        };

        iceberg.register_table(
            "sales".to_owned(),
            files.clone(),
            Some(spec.clone()),
        );

        let listed = iceberg
            .list_files("sales")
            .expect("registered table should be listable");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].path, "data/part-0001.parquet");

        let part = iceberg
            .partition_spec("sales")
            .expect("partition spec lookup should succeed");
        assert!(part.is_some());
        let part = part.expect("already checked is_some");
        assert_eq!(part.fields.len(), 1);
    }

    #[test]
    fn iceberg_file_statistics_returns_none_for_stub() {
        let iceberg = IcebergFormat::new();
        let file = DataFile::new(
            "data/part-0001.parquet".to_owned(),
            1024,
            500,
        );
        assert!(iceberg.file_statistics(&file).is_none());
    }

    #[test]
    fn trait_object_usage() {
        let iceberg = IcebergFormat::new();
        let format: &dyn TableFormat = &iceberg;
        assert_eq!(
            format.format_type(),
            TableFormatType::Iceberg
        );
        assert!(format.supports_time_travel());
    }

    #[test]
    fn file_column_stats_construction() {
        let stats = FileColumnStats {
            min: Some("10".to_owned()),
            max: Some("999".to_owned()),
            null_count: 5,
            distinct_count: Some(100),
        };
        assert_eq!(stats.min.as_deref(), Some("10"));
        assert_eq!(stats.max.as_deref(), Some("999"));
        assert_eq!(stats.null_count, 5);
        assert_eq!(stats.distinct_count, Some(100));
    }

    #[test]
    fn file_stats_construction() {
        let mut column_stats = HashMap::new();
        column_stats.insert(
            "amount".to_owned(),
            FileColumnStats {
                min: Some("0.0".to_owned()),
                max: Some("9999.99".to_owned()),
                null_count: 0,
                distinct_count: None,
            },
        );

        let stats = FileStats {
            record_count: 50_000,
            size_bytes: 10 * 1024 * 1024,
            column_stats,
        };

        assert_eq!(stats.record_count, 50_000);
        assert!(stats.column_stats.contains_key("amount"));
    }

    #[test]
    fn table_format_error_display() {
        let err = TableFormatError::TableNotFound {
            name: "orders".to_owned(),
        };
        assert_eq!(err.to_string(), "table not found: orders");

        let err = TableFormatError::Metadata {
            table: "sales".to_owned(),
            message: "corrupt manifest".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "metadata error for sales: corrupt manifest"
        );

        let err = TableFormatError::Unsupported {
            format: "traditional".to_owned(),
            operation: "time_travel".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "unsupported operation on traditional: time_travel"
        );
    }
}
