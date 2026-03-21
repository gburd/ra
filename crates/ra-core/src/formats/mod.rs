//! File format abstractions for columnar data.
//!
//! This module defines the [`FileFormat`] trait, which provides a
//! uniform interface for reading metadata, schema, and data from
//! columnar file formats such as Parquet, ORC, and Arrow IPC.
//!
//! The query planner uses [`FileMetadata`] to make cost-based
//! decisions about column pruning, predicate pushdown, and row
//! group filtering.

#[cfg(any(feature = "parquet", test))]
pub mod parquet;

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::facts::DataType;

/// Abstraction over columnar file formats.
///
/// Implementors provide access to schema, metadata, and scanning
/// for a specific file format. The trait is object-safe so format
/// implementations can be stored in collections.
pub trait FileFormat: Send + Sync + std::fmt::Debug {
    /// Format name (e.g., "parquet", "orc", "`arrow_ipc`").
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str;

    /// Read schema without scanning data.
    ///
    /// For Parquet this reads the footer (~4KB at end of file).
    /// Cost: O(1), typically under 1ms.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or the schema
    /// cannot be parsed.
    fn read_schema(&self, path: &Path) -> Result<Schema, FormatError>;

    /// Read file metadata including statistics and row groups.
    ///
    /// For Parquet this parses the footer metadata to extract
    /// per-row-group, per-column statistics (min/max/null count).
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or metadata
    /// cannot be parsed.
    fn read_metadata(
        &self,
        path: &Path,
    ) -> Result<FileMetadata, FormatError>;

    /// Optimization capabilities of this format.
    fn capabilities(&self) -> FormatCapabilities;

    /// File extensions this format handles (e.g., `["parquet"]`).
    fn extensions(&self) -> &[&str];
}

/// Schema describing the columns in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    /// Ordered list of fields.
    pub fields: Vec<Field>,
}

impl Schema {
    /// Create a new schema from a list of fields.
    #[must_use]
    pub fn new(fields: Vec<Field>) -> Self {
        Self { fields }
    }

    /// Number of fields in the schema.
    #[must_use]
    pub fn num_fields(&self) -> usize {
        self.fields.len()
    }

    /// Look up a field by name.
    #[must_use]
    pub fn field_by_name(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// A single field (column) in a schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Column name.
    pub name: String,
    /// Column data type.
    pub data_type: DataType,
    /// Whether the column is nullable.
    pub nullable: bool,
}

impl Field {
    /// Create a new nullable field.
    #[must_use]
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: true,
        }
    }

    /// Create a new non-nullable field.
    #[must_use]
    pub fn non_null(
        name: impl Into<String>,
        data_type: DataType,
    ) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: false,
        }
    }
}

/// Metadata extracted from a columnar file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Schema of the file.
    #[serde(skip)]
    pub schema: Schema,
    /// Total rows across all row groups.
    pub num_rows: u64,
    /// Row groups (Parquet) or stripes (ORC).
    pub row_groups: Vec<RowGroupMeta>,
    /// File-level aggregated statistics per column.
    pub file_stats: HashMap<String, FileColumnStats>,
    /// File modification time for staleness tracking.
    #[serde(with = "system_time_serde")]
    pub mtime: SystemTime,
}

/// Metadata for a single row group (or ORC stripe).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowGroupMeta {
    /// Row group index (0-based).
    pub index: usize,
    /// Byte offset in file.
    pub offset: u64,
    /// Number of rows in this group.
    pub num_rows: u64,
    /// Per-column statistics.
    pub column_stats: HashMap<String, FileColumnStats>,
    /// Compressed size in bytes.
    pub compressed_size: u64,
    /// Uncompressed size in bytes.
    pub uncompressed_size: u64,
}

/// Statistics for a single column within a file or row group.
///
/// These are the raw statistics from the file format, distinct
/// from the cost-model [`ColumnStats`](crate::statistics::ColumnStats)
/// which uses floating-point fractions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileColumnStats {
    /// Minimum value (None if all NULL or unavailable).
    pub min: Option<ScalarValue>,
    /// Maximum value (None if all NULL or unavailable).
    pub max: Option<ScalarValue>,
    /// Number of NULL values.
    pub null_count: u64,
    /// Distinct value count (not all formats provide this).
    pub distinct_count: Option<u64>,
}

impl FileColumnStats {
    /// Create empty stats (no information available).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            min: None,
            max: None,
            null_count: 0,
            distinct_count: None,
        }
    }
}

/// A scalar value from file statistics.
///
/// Represents min/max values stored in columnar file metadata.
/// Implements [`partial_cmp_value`](ScalarValue::partial_cmp_value)
/// so predicates can be evaluated against row group statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScalarValue {
    /// A null value.
    Null,
    /// A boolean value.
    Bool(bool),
    /// A 64-bit integer.
    Int64(i64),
    /// A 64-bit floating-point number.
    Float64(f64),
    /// A UTF-8 string.
    Utf8(String),
    /// Raw bytes.
    Binary(Vec<u8>),
}

impl ScalarValue {
    /// Compare two scalar values, returning `None` for incompatible
    /// types or null values.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn partial_cmp_value(
        &self,
        other: &Self,
    ) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a.partial_cmp(b),
            (Self::Int64(a), Self::Int64(b)) => a.partial_cmp(b),
            (Self::Float64(a), Self::Float64(b)) => {
                a.partial_cmp(b)
            }
            (Self::Utf8(a), Self::Utf8(b)) => a.partial_cmp(b),
            (Self::Int64(a), Self::Float64(b)) => {
                (*a as f64).partial_cmp(b)
            }
            (Self::Float64(a), Self::Int64(b)) => {
                a.partial_cmp(&(*b as f64))
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for ScalarValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Int64(v) => write!(f, "{v}"),
            Self::Float64(v) => write!(f, "{v}"),
            Self::Utf8(v) => write!(f, "'{v}'"),
            Self::Binary(v) => write!(f, "<{} bytes>", v.len()),
        }
    }
}

/// Capabilities advertised by a file format.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatCapabilities {
    /// Format supports reading a subset of columns.
    pub column_pruning: bool,
    /// Format supports filtering at the row-group level
    /// using column statistics.
    pub predicate_pushdown: bool,
    /// Format provides per-column min/max/null statistics.
    pub column_statistics: bool,
    /// Format supports bloom filters for set membership.
    pub bloom_filters: bool,
    /// Format supports nested/struct column projection.
    pub nested_columns: bool,
}

impl FormatCapabilities {
    /// No optimization capabilities.
    #[must_use]
    pub fn none() -> Self {
        Self {
            column_pruning: false,
            predicate_pushdown: false,
            column_statistics: false,
            bloom_filters: false,
            nested_columns: false,
        }
    }
}

/// Options for scanning a file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanOptions {
    /// Columns to read (empty means all columns).
    pub projection: Vec<String>,
    /// Row groups to read (None means all row groups).
    pub row_group_filter: Option<Vec<usize>>,
}

/// Errors from file format operations.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The file could not be found or opened.
    #[error("file not found: {path}")]
    FileNotFound {
        /// Path that was not found.
        path: String,
    },

    /// The file is not in the expected format.
    #[error("invalid format for {path}: {reason}")]
    InvalidFormat {
        /// Path of the invalid file.
        path: String,
        /// Description of the format problem.
        reason: String,
    },

    /// An I/O error occurred.
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// Path being read.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The operation is not supported by this format.
    #[error("unsupported operation: {operation}")]
    Unsupported {
        /// Description of the unsupported operation.
        operation: String,
    },
}

/// Serde helper for `SystemTime`.
mod system_time_serde {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Dur {
        secs: u64,
        nanos: u32,
    }

    pub fn serialize<S: Serializer>(
        time: &SystemTime,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let dur = time
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        Dur {
            secs: dur.as_secs(),
            nanos: dur.subsec_nanos(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<SystemTime, D::Error> {
        let dur = Dur::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::new(dur.secs, dur.nanos))
    }
}

/// Detect the appropriate file format from a file extension.
///
/// Returns `None` if the extension is not recognized.
#[must_use]
pub fn detect_format(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "parquet" | "pq" => Some("parquet"),
        "orc" => Some("orc"),
        "arrow" | "ipc" | "feather" => Some("arrow_ipc"),
        _ => None,
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;
    use std::path::PathBuf;

    #[test]
    fn schema_field_lookup() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Integer),
            Field::new("name", DataType::String),
            Field::non_null("active", DataType::Boolean),
        ]);

        assert_eq!(schema.num_fields(), 3);
        let id = schema
            .field_by_name("id")
            .expect("id field should exist");
        assert_eq!(id.data_type, DataType::Integer);
        assert!(id.nullable);

        let active = schema
            .field_by_name("active")
            .expect("active field should exist");
        assert!(!active.nullable);

        assert!(schema.field_by_name("missing").is_none());
    }

    #[test]
    fn file_column_stats_empty() {
        let stats = FileColumnStats::empty();
        assert!(stats.min.is_none());
        assert!(stats.max.is_none());
        assert_eq!(stats.null_count, 0);
        assert!(stats.distinct_count.is_none());
    }

    #[test]
    fn scalar_value_display() {
        assert_eq!(ScalarValue::Null.to_string(), "NULL");
        assert_eq!(ScalarValue::Bool(true).to_string(), "true");
        assert_eq!(ScalarValue::Int64(42).to_string(), "42");
        assert_eq!(ScalarValue::Float64(3.14).to_string(), "3.14");
        assert_eq!(
            ScalarValue::Utf8("hello".into()).to_string(),
            "'hello'"
        );
        assert_eq!(
            ScalarValue::Binary(vec![1, 2, 3]).to_string(),
            "<3 bytes>"
        );
    }

    #[test]
    fn scalar_value_ordering_same_type() {
        let a = ScalarValue::Int64(10);
        let b = ScalarValue::Int64(20);
        assert_eq!(
            a.partial_cmp_value(&b),
            Some(Ordering::Less)
        );
        assert_eq!(
            b.partial_cmp_value(&a),
            Some(Ordering::Greater)
        );
        assert_eq!(
            a.partial_cmp_value(&a),
            Some(Ordering::Equal)
        );
    }

    #[test]
    fn scalar_value_ordering_float() {
        let a = ScalarValue::Float64(1.5);
        let b = ScalarValue::Float64(2.5);
        assert_eq!(
            a.partial_cmp_value(&b),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn scalar_value_ordering_string() {
        let a = ScalarValue::Utf8("alice".into());
        let b = ScalarValue::Utf8("bob".into());
        assert_eq!(
            a.partial_cmp_value(&b),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn scalar_value_ordering_cross_type() {
        let a = ScalarValue::Int64(10);
        let b = ScalarValue::Float64(10.5);
        assert_eq!(
            a.partial_cmp_value(&b),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn scalar_value_ordering_null() {
        let a = ScalarValue::Null;
        let b = ScalarValue::Int64(10);
        assert!(a.partial_cmp_value(&b).is_none());
    }

    #[test]
    fn scalar_value_ordering_incompatible() {
        let a = ScalarValue::Bool(true);
        let b = ScalarValue::Int64(1);
        assert!(a.partial_cmp_value(&b).is_none());
    }

    #[test]
    fn format_capabilities_none() {
        let caps = FormatCapabilities::none();
        assert!(!caps.column_pruning);
        assert!(!caps.predicate_pushdown);
        assert!(!caps.column_statistics);
        assert!(!caps.bloom_filters);
        assert!(!caps.nested_columns);
    }

    #[test]
    fn scan_options_default() {
        let opts = ScanOptions::default();
        assert!(opts.projection.is_empty());
        assert!(opts.row_group_filter.is_none());
    }

    #[test]
    fn detect_format_parquet() {
        assert_eq!(
            detect_format(Path::new("data.parquet")),
            Some("parquet")
        );
        assert_eq!(
            detect_format(Path::new("data.pq")),
            Some("parquet")
        );
    }

    #[test]
    fn detect_format_orc() {
        assert_eq!(detect_format(Path::new("data.orc")), Some("orc"));
    }

    #[test]
    fn detect_format_arrow() {
        assert_eq!(
            detect_format(Path::new("data.arrow")),
            Some("arrow_ipc")
        );
        assert_eq!(
            detect_format(Path::new("data.ipc")),
            Some("arrow_ipc")
        );
        assert_eq!(
            detect_format(Path::new("data.feather")),
            Some("arrow_ipc")
        );
    }

    #[test]
    fn detect_format_unknown() {
        assert!(detect_format(Path::new("data.csv")).is_none());
        assert!(detect_format(Path::new("data.json")).is_none());
        assert!(detect_format(Path::new("noext")).is_none());
    }

    #[test]
    fn format_error_display() {
        let err = FormatError::FileNotFound {
            path: "/tmp/missing.parquet".into(),
        };
        assert_eq!(
            err.to_string(),
            "file not found: /tmp/missing.parquet"
        );

        let err = FormatError::InvalidFormat {
            path: "/tmp/bad.parquet".into(),
            reason: "invalid magic bytes".into(),
        };
        assert!(err.to_string().contains("invalid format"));

        let err = FormatError::Unsupported {
            operation: "bloom filter read".into(),
        };
        assert!(err.to_string().contains("unsupported operation"));
    }

    #[test]
    fn file_metadata_serde_roundtrip() {
        let meta = FileMetadata {
            schema: Schema::new(vec![
                Field::new("x", DataType::Integer),
            ]),
            num_rows: 1000,
            row_groups: vec![RowGroupMeta {
                index: 0,
                offset: 4,
                num_rows: 1000,
                column_stats: HashMap::from([(
                    "x".into(),
                    FileColumnStats {
                        min: Some(ScalarValue::Int64(1)),
                        max: Some(ScalarValue::Int64(100)),
                        null_count: 5,
                        distinct_count: Some(95),
                    },
                )]),
                compressed_size: 4096,
                uncompressed_size: 8192,
            }],
            file_stats: HashMap::new(),
            mtime: SystemTime::UNIX_EPOCH,
        };

        let json = serde_json::to_string(&meta)
            .expect("serialization should succeed");
        let roundtrip: FileMetadata = serde_json::from_str(&json)
            .expect("deserialization should succeed");

        assert_eq!(roundtrip.num_rows, 1000);
        assert_eq!(roundtrip.row_groups.len(), 1);
        assert_eq!(roundtrip.row_groups[0].num_rows, 1000);
    }

    #[test]
    fn detect_format_with_path_prefix() {
        let path = PathBuf::from("/data/warehouse/sales.parquet");
        assert_eq!(detect_format(&path), Some("parquet"));
    }

    #[test]
    fn field_constructors() {
        let nullable = Field::new("col", DataType::Integer);
        assert!(nullable.nullable);

        let non_null = Field::non_null("col", DataType::Integer);
        assert!(!non_null.nullable);
    }

    #[test]
    fn row_group_meta_stats() {
        let rg = RowGroupMeta {
            index: 0,
            offset: 0,
            num_rows: 500,
            column_stats: HashMap::from([
                (
                    "id".into(),
                    FileColumnStats {
                        min: Some(ScalarValue::Int64(1)),
                        max: Some(ScalarValue::Int64(500)),
                        null_count: 0,
                        distinct_count: Some(500),
                    },
                ),
                (
                    "name".into(),
                    FileColumnStats {
                        min: Some(ScalarValue::Utf8(
                            "alice".into(),
                        )),
                        max: Some(ScalarValue::Utf8("zara".into())),
                        null_count: 10,
                        distinct_count: None,
                    },
                ),
            ]),
            compressed_size: 2048,
            uncompressed_size: 4096,
        };

        assert_eq!(rg.column_stats.len(), 2);
        let id_stats = rg
            .column_stats
            .get("id")
            .expect("id stats should exist");
        assert_eq!(id_stats.null_count, 0);
        assert_eq!(id_stats.distinct_count, Some(500));
    }

    #[test]
    fn schema_default_is_empty() {
        let schema = Schema::default();
        assert_eq!(schema.num_fields(), 0);
    }
}
