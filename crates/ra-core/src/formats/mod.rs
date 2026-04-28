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
    /// Per-column encoding information.
    #[serde(default)]
    pub column_encodings: HashMap<String, ColumnEncodingInfo>,
}

impl RowGroupMeta {
    /// Compression ratio for this row group (uncompressed / compressed).
    /// Returns 1.0 if compressed size is zero.
    #[must_use]
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_size == 0 {
            return 1.0;
        }
        self.uncompressed_size as f64 / self.compressed_size as f64
    }

    /// Check whether a column is dictionary-encoded in this row group.
    #[must_use]
    pub fn is_dictionary_encoded(&self, column: &str) -> bool {
        self.column_encodings
            .get(column)
            .is_some_and(|e| e.encoding == ColumnEncoding::Dictionary)
    }

    /// Check whether this row group can be pruned by zone maps
    /// for the given column and scalar value bounds.
    ///
    /// Returns `true` if the row group can be skipped (predicate
    /// value falls outside the column's min/max range).
    #[must_use]
    pub fn can_prune_with_zone_map(
        &self,
        column: &str,
        value: &ScalarValue,
    ) -> bool {
        let Some(stats) = self.column_stats.get(column) else {
            return false;
        };
        let Some(ref min) = stats.min else {
            return false;
        };
        let Some(ref max) = stats.max else {
            return false;
        };
        // Prune if value < min or value > max
        let below_min = value
            .partial_cmp_value(min)
            .is_some_and(|o| o == std::cmp::Ordering::Less);
        let above_max = value
            .partial_cmp_value(max)
            .is_some_and(|o| o == std::cmp::Ordering::Greater);
        below_min || above_max
    }
}

/// Encoding information for a single column chunk within a row group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnEncodingInfo {
    /// The encoding used for this column chunk.
    pub encoding: ColumnEncoding,
    /// Compression codec applied to the column chunk.
    pub compression: CompressionCodec,
    /// Number of distinct values in the dictionary (if dictionary-encoded).
    pub dictionary_size: Option<u64>,
    /// Compressed size of this column chunk in bytes.
    pub compressed_bytes: u64,
    /// Uncompressed size of this column chunk in bytes.
    pub uncompressed_bytes: u64,
}

impl ColumnEncodingInfo {
    /// Compression ratio for this column chunk.
    /// Returns 1.0 if compressed size is zero.
    #[must_use]
    pub fn compression_ratio(&self) -> f64 {
        if self.compressed_bytes == 0 {
            return 1.0;
        }
        self.uncompressed_bytes as f64
            / self.compressed_bytes as f64
    }

    /// Width of dictionary codes in bytes (1, 2, or 4).
    /// Returns `None` if not dictionary-encoded.
    #[must_use]
    pub fn dict_code_width(&self) -> Option<u8> {
        let dict_size = self.dictionary_size?;
        if dict_size <= 256 {
            Some(1)
        } else if dict_size <= 65_536 {
            Some(2)
        } else {
            Some(4)
        }
    }
}

/// Column encoding type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum ColumnEncoding {
    /// Plain encoding (no compression beyond the codec).
    Plain,
    /// Dictionary encoding (values stored as integer codes).
    Dictionary,
    /// Run-length encoding.
    RunLength,
    /// Delta / frame-of-reference encoding.
    DeltaEncoding,
    /// Bit-packed encoding.
    BitPacked,
    /// Byte-stream split encoding (for floating point).
    ByteStreamSplit,
}

impl std::fmt::Display for ColumnEncoding {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Plain => write!(f, "PLAIN"),
            Self::Dictionary => write!(f, "DICTIONARY"),
            Self::RunLength => write!(f, "RLE"),
            Self::DeltaEncoding => write!(f, "DELTA"),
            Self::BitPacked => write!(f, "BIT_PACKED"),
            Self::ByteStreamSplit => write!(f, "BYTE_STREAM_SPLIT"),
        }
    }
}

/// Compression codec applied to column data.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    Serialize,
    Deserialize,
)]
pub enum CompressionCodec {
    /// No compression.
    #[default]
    Uncompressed,
    /// Snappy compression.
    Snappy,
    /// Gzip compression.
    Gzip,
    /// LZO compression.
    Lzo,
    /// Brotli compression.
    Brotli,
    /// LZ4 compression.
    Lz4,
    /// Zstandard compression.
    Zstd,
    /// LZ4 raw compression.
    Lz4Raw,
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
#[expect(clippy::struct_excessive_bools)]
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
    /// Format supports dictionary encoding with pushdown.
    #[serde(default)]
    pub dictionary_encoding: bool,
    /// Format supports late materialization (deferred column reads).
    #[serde(default)]
    pub late_materialization: bool,
    /// Format provides per-column encoding metadata.
    #[serde(default)]
    pub encoding_metadata: bool,
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
            dictionary_encoding: false,
            late_materialization: false,
            encoding_metadata: false,
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

impl FileMetadata {
    /// Return indices of row groups that can be pruned for an
    /// equality predicate on the given column and value.
    ///
    /// These row groups have zone maps (min/max) that prove no
    /// rows can match the predicate.
    #[must_use]
    pub fn prunable_row_groups(
        &self,
        column: &str,
        value: &ScalarValue,
    ) -> Vec<usize> {
        self.row_groups
            .iter()
            .filter(|rg| rg.can_prune_with_zone_map(column, value))
            .map(|rg| rg.index)
            .collect()
    }

    /// Return indices of row groups that survive zone map pruning
    /// (complement of `prunable_row_groups`).
    #[must_use]
    pub fn surviving_row_groups(
        &self,
        column: &str,
        value: &ScalarValue,
    ) -> Vec<usize> {
        self.row_groups
            .iter()
            .filter(|rg| {
                !rg.can_prune_with_zone_map(column, value)
            })
            .map(|rg| rg.index)
            .collect()
    }

    /// Average compression ratio across all row groups.
    /// Returns 1.0 if there are no row groups.
    #[must_use]
    pub fn avg_compression_ratio(&self) -> f64 {
        if self.row_groups.is_empty() {
            return 1.0;
        }
        let total_compressed: u64 = self
            .row_groups
            .iter()
            .map(|rg| rg.compressed_size)
            .sum();
        let total_uncompressed: u64 = self
            .row_groups
            .iter()
            .map(|rg| rg.uncompressed_size)
            .sum();
        if total_compressed == 0 {
            return 1.0;
        }
        total_uncompressed as f64 / total_compressed as f64
    }
}

/// Adjusts cost estimates based on column encoding and compression.
///
/// The optimizer uses this to produce more accurate I/O and CPU cost
/// estimates when operating on compressed columnar data.
#[derive(Debug, Clone, Copy)]
pub struct CompressionCostAdjuster {
    /// CPU cost multiplier for decompression overhead.
    /// Typical values: 1.0 (uncompressed), 1.1 (Snappy), 1.3 (Zstd).
    pub decompression_cpu_factor: f64,
    /// I/O cost multiplier based on compression ratio.
    /// Equals `1.0 / compression_ratio`.
    pub io_reduction_factor: f64,
    /// Additional CPU savings from dictionary predicate pushdown.
    /// Typical value: 0.2 (5x cheaper to compare codes than strings).
    pub dict_predicate_cpu_factor: f64,
}

impl CompressionCostAdjuster {
    /// Create an adjuster from encoding info.
    #[must_use]
    pub fn from_encoding(info: &ColumnEncodingInfo) -> Self {
        let decompression_cpu_factor = match info.compression {
            CompressionCodec::Uncompressed => 1.0,
            CompressionCodec::Snappy | CompressionCodec::Lz4
            | CompressionCodec::Lz4Raw => 1.1,
            CompressionCodec::Gzip => 1.4,
            CompressionCodec::Zstd => 1.3,
            CompressionCodec::Brotli => 1.5,
            CompressionCodec::Lzo => 1.15,
        };

        let compression_ratio = info.compression_ratio();
        let io_reduction_factor = if compression_ratio > 1.0 {
            1.0 / compression_ratio
        } else {
            1.0
        };

        let dict_predicate_cpu_factor =
            if info.encoding == ColumnEncoding::Dictionary {
                info.dict_code_width().map_or(0.5, |w| match w {
                    1 => 0.1,
                    2 => 0.15,
                    _ => 0.3,
                })
            } else {
                1.0
            };

        Self {
            decompression_cpu_factor,
            io_reduction_factor,
            dict_predicate_cpu_factor,
        }
    }

    /// Adjust an I/O cost estimate for compression.
    #[must_use]
    pub fn adjust_io(&self, base_io: f64) -> f64 {
        base_io * self.io_reduction_factor
    }

    /// Adjust a CPU cost estimate for decompression overhead.
    #[must_use]
    pub fn adjust_cpu(&self, base_cpu: f64) -> f64 {
        base_cpu * self.decompression_cpu_factor
    }

    /// Adjust a predicate evaluation cost for dictionary encoding.
    #[must_use]
    pub fn adjust_predicate_cpu(&self, base_cpu: f64) -> f64 {
        base_cpu * self.dict_predicate_cpu_factor
    }
}

impl Default for CompressionCostAdjuster {
    fn default() -> Self {
        Self {
            decompression_cpu_factor: 1.0,
            io_reduction_factor: 1.0,
            dict_predicate_cpu_factor: 1.0,
        }
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
    #[expect(clippy::approx_constant, reason = "3.14 is test data, not mathematical constant")]
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
        assert!(!caps.dictionary_encoding);
        assert!(!caps.late_materialization);
        assert!(!caps.encoding_metadata);
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
                column_encodings: HashMap::new(),
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
            column_encodings: HashMap::new(),
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

    // ---- Format-aware optimization tests ----

    #[test]
    fn column_encoding_display() {
        assert_eq!(ColumnEncoding::Plain.to_string(), "PLAIN");
        assert_eq!(
            ColumnEncoding::Dictionary.to_string(),
            "DICTIONARY"
        );
        assert_eq!(ColumnEncoding::RunLength.to_string(), "RLE");
        assert_eq!(ColumnEncoding::DeltaEncoding.to_string(), "DELTA");
        assert_eq!(
            ColumnEncoding::BitPacked.to_string(),
            "BIT_PACKED"
        );
    }

    #[test]
    fn compression_ratio() {
        let rg = RowGroupMeta {
            index: 0,
            offset: 0,
            num_rows: 1000,
            column_stats: HashMap::new(),
            compressed_size: 1000,
            uncompressed_size: 4000,
            column_encodings: HashMap::new(),
        };
        let ratio = rg.compression_ratio();
        assert!((ratio - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn compression_ratio_zero_compressed() {
        let rg = RowGroupMeta {
            index: 0,
            offset: 0,
            num_rows: 0,
            column_stats: HashMap::new(),
            compressed_size: 0,
            uncompressed_size: 0,
            column_encodings: HashMap::new(),
        };
        assert!((rg.compression_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn is_dictionary_encoded() {
        let rg = RowGroupMeta {
            index: 0,
            offset: 0,
            num_rows: 100,
            column_stats: HashMap::new(),
            compressed_size: 100,
            uncompressed_size: 200,
            column_encodings: HashMap::from([(
                "status".into(),
                ColumnEncodingInfo {
                    encoding: ColumnEncoding::Dictionary,
                    compression: CompressionCodec::Snappy,
                    dictionary_size: Some(4),
                    compressed_bytes: 50,
                    uncompressed_bytes: 100,
                },
            )]),
        };
        assert!(rg.is_dictionary_encoded("status"));
        assert!(!rg.is_dictionary_encoded("id"));
    }

    #[test]
    fn zone_map_pruning() {
        let rg = RowGroupMeta {
            index: 0,
            offset: 0,
            num_rows: 1000,
            column_stats: HashMap::from([(
                "id".into(),
                FileColumnStats {
                    min: Some(ScalarValue::Int64(100)),
                    max: Some(ScalarValue::Int64(200)),
                    null_count: 0,
                    distinct_count: None,
                },
            )]),
            compressed_size: 1000,
            uncompressed_size: 2000,
            column_encodings: HashMap::new(),
        };

        // Value within range: cannot prune
        assert!(
            !rg.can_prune_with_zone_map(
                "id",
                &ScalarValue::Int64(150)
            )
        );

        // Value below min: can prune
        assert!(
            rg.can_prune_with_zone_map(
                "id",
                &ScalarValue::Int64(50)
            )
        );

        // Value above max: can prune
        assert!(
            rg.can_prune_with_zone_map(
                "id",
                &ScalarValue::Int64(300)
            )
        );

        // Unknown column: cannot prune
        assert!(
            !rg.can_prune_with_zone_map(
                "unknown",
                &ScalarValue::Int64(150)
            )
        );
    }

    #[test]
    fn file_metadata_prunable_row_groups() {
        let meta = FileMetadata {
            schema: Schema::default(),
            num_rows: 3000,
            row_groups: vec![
                RowGroupMeta {
                    index: 0,
                    offset: 0,
                    num_rows: 1000,
                    column_stats: HashMap::from([(
                        "id".into(),
                        FileColumnStats {
                            min: Some(ScalarValue::Int64(0)),
                            max: Some(ScalarValue::Int64(999)),
                            null_count: 0,
                            distinct_count: None,
                        },
                    )]),
                    compressed_size: 1000,
                    uncompressed_size: 2000,
                    column_encodings: HashMap::new(),
                },
                RowGroupMeta {
                    index: 1,
                    offset: 1000,
                    num_rows: 1000,
                    column_stats: HashMap::from([(
                        "id".into(),
                        FileColumnStats {
                            min: Some(ScalarValue::Int64(1000)),
                            max: Some(ScalarValue::Int64(1999)),
                            null_count: 0,
                            distinct_count: None,
                        },
                    )]),
                    compressed_size: 1000,
                    uncompressed_size: 2000,
                    column_encodings: HashMap::new(),
                },
                RowGroupMeta {
                    index: 2,
                    offset: 2000,
                    num_rows: 1000,
                    column_stats: HashMap::from([(
                        "id".into(),
                        FileColumnStats {
                            min: Some(ScalarValue::Int64(2000)),
                            max: Some(ScalarValue::Int64(2999)),
                            null_count: 0,
                            distinct_count: None,
                        },
                    )]),
                    compressed_size: 1000,
                    uncompressed_size: 2000,
                    column_encodings: HashMap::new(),
                },
            ],
            file_stats: HashMap::new(),
            mtime: SystemTime::UNIX_EPOCH,
        };

        // Looking for id=500: only row group 0 survives
        let pruned = meta.prunable_row_groups(
            "id",
            &ScalarValue::Int64(500),
        );
        assert_eq!(pruned, vec![1, 2]);

        let surviving = meta.surviving_row_groups(
            "id",
            &ScalarValue::Int64(500),
        );
        assert_eq!(surviving, vec![0]);
    }

    #[test]
    fn avg_compression_ratio() {
        let meta = FileMetadata {
            schema: Schema::default(),
            num_rows: 2000,
            row_groups: vec![
                RowGroupMeta {
                    index: 0,
                    offset: 0,
                    num_rows: 1000,
                    column_stats: HashMap::new(),
                    compressed_size: 500,
                    uncompressed_size: 2000,
                    column_encodings: HashMap::new(),
                },
                RowGroupMeta {
                    index: 1,
                    offset: 500,
                    num_rows: 1000,
                    column_stats: HashMap::new(),
                    compressed_size: 500,
                    uncompressed_size: 2000,
                    column_encodings: HashMap::new(),
                },
            ],
            file_stats: HashMap::new(),
            mtime: SystemTime::UNIX_EPOCH,
        };

        // 4000 / 1000 = 4.0
        assert!(
            (meta.avg_compression_ratio() - 4.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn column_encoding_info_compression_ratio() {
        let info = ColumnEncodingInfo {
            encoding: ColumnEncoding::Dictionary,
            compression: CompressionCodec::Snappy,
            dictionary_size: Some(100),
            compressed_bytes: 500,
            uncompressed_bytes: 2000,
        };
        assert!(
            (info.compression_ratio() - 4.0).abs() < f64::EPSILON
        );
    }

    #[test]
    fn dict_code_width() {
        let small = ColumnEncodingInfo {
            encoding: ColumnEncoding::Dictionary,
            compression: CompressionCodec::Uncompressed,
            dictionary_size: Some(50),
            compressed_bytes: 100,
            uncompressed_bytes: 100,
        };
        assert_eq!(small.dict_code_width(), Some(1));

        let medium = ColumnEncodingInfo {
            encoding: ColumnEncoding::Dictionary,
            compression: CompressionCodec::Uncompressed,
            dictionary_size: Some(1000),
            compressed_bytes: 100,
            uncompressed_bytes: 100,
        };
        assert_eq!(medium.dict_code_width(), Some(2));

        let large = ColumnEncodingInfo {
            encoding: ColumnEncoding::Dictionary,
            compression: CompressionCodec::Uncompressed,
            dictionary_size: Some(100_000),
            compressed_bytes: 100,
            uncompressed_bytes: 100,
        };
        assert_eq!(large.dict_code_width(), Some(4));

        let not_dict = ColumnEncodingInfo {
            encoding: ColumnEncoding::Plain,
            compression: CompressionCodec::Uncompressed,
            dictionary_size: None,
            compressed_bytes: 100,
            uncompressed_bytes: 100,
        };
        assert_eq!(not_dict.dict_code_width(), None);
    }

    #[test]
    fn compression_cost_adjuster_snappy_dict() {
        let info = ColumnEncodingInfo {
            encoding: ColumnEncoding::Dictionary,
            compression: CompressionCodec::Snappy,
            dictionary_size: Some(100),
            compressed_bytes: 500,
            uncompressed_bytes: 2000,
        };
        let adjuster = CompressionCostAdjuster::from_encoding(&info);

        let tol = 1e-10;

        // Snappy decompression factor = 1.1
        assert!(
            (adjuster.decompression_cpu_factor - 1.1).abs() < tol
        );
        // IO reduction: 1/4 = 0.25
        assert!(
            (adjuster.io_reduction_factor - 0.25).abs() < tol
        );
        // Dictionary with 100 entries -> 1-byte codes -> factor 0.1
        assert!(
            (adjuster.dict_predicate_cpu_factor - 0.1).abs() < tol
        );

        // Base I/O of 1000 -> adjusted to 250
        assert!(
            (adjuster.adjust_io(1000.0) - 250.0).abs() < tol
        );
        // Base CPU of 100 -> adjusted to 110
        assert!(
            (adjuster.adjust_cpu(100.0) - 110.0).abs() < tol
        );
        // Predicate CPU of 100 -> adjusted to 10
        assert!(
            (adjuster.adjust_predicate_cpu(100.0) - 10.0).abs()
                < tol
        );
    }

    #[test]
    fn compression_cost_adjuster_plain_uncompressed() {
        let info = ColumnEncodingInfo {
            encoding: ColumnEncoding::Plain,
            compression: CompressionCodec::Uncompressed,
            dictionary_size: None,
            compressed_bytes: 1000,
            uncompressed_bytes: 1000,
        };
        let adjuster = CompressionCostAdjuster::from_encoding(&info);
        assert!(
            (adjuster.decompression_cpu_factor - 1.0).abs()
                < f64::EPSILON
        );
        assert!(
            (adjuster.io_reduction_factor - 1.0).abs()
                < f64::EPSILON
        );
        assert!(
            (adjuster.dict_predicate_cpu_factor - 1.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn compression_cost_adjuster_default() {
        let adjuster = CompressionCostAdjuster::default();
        assert!(
            (adjuster.adjust_io(100.0) - 100.0).abs()
                < f64::EPSILON
        );
        assert!(
            (adjuster.adjust_cpu(100.0) - 100.0).abs()
                < f64::EPSILON
        );
        assert!(
            (adjuster.adjust_predicate_cpu(100.0) - 100.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn compression_cost_adjuster_zstd() {
        let info = ColumnEncodingInfo {
            encoding: ColumnEncoding::Plain,
            compression: CompressionCodec::Zstd,
            dictionary_size: None,
            compressed_bytes: 200,
            uncompressed_bytes: 1000,
        };
        let adjuster = CompressionCostAdjuster::from_encoding(&info);
        assert!(
            (adjuster.decompression_cpu_factor - 1.3).abs()
                < f64::EPSILON
        );
        // 1/5 = 0.2
        assert!(
            (adjuster.io_reduction_factor - 0.2).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn column_encoding_serde_roundtrip() {
        let info = ColumnEncodingInfo {
            encoding: ColumnEncoding::Dictionary,
            compression: CompressionCodec::Snappy,
            dictionary_size: Some(256),
            compressed_bytes: 1024,
            uncompressed_bytes: 4096,
        };
        let json = serde_json::to_string(&info)
            .expect("serialization should succeed");
        let roundtrip: ColumnEncodingInfo =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(info, roundtrip);
    }

    #[test]
    fn row_group_meta_with_encodings_serde() {
        let rg = RowGroupMeta {
            index: 0,
            offset: 0,
            num_rows: 1000,
            column_stats: HashMap::new(),
            compressed_size: 500,
            uncompressed_size: 2000,
            column_encodings: HashMap::from([(
                "status".into(),
                ColumnEncodingInfo {
                    encoding: ColumnEncoding::Dictionary,
                    compression: CompressionCodec::Snappy,
                    dictionary_size: Some(4),
                    compressed_bytes: 200,
                    uncompressed_bytes: 800,
                },
            )]),
        };
        let json = serde_json::to_string(&rg)
            .expect("serialization should succeed");
        let roundtrip: RowGroupMeta = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert!(roundtrip.is_dictionary_encoded("status"));
    }

    #[test]
    fn backward_compat_row_group_no_encodings() {
        let json = r#"{
            "index": 0,
            "offset": 0,
            "num_rows": 100,
            "column_stats": {},
            "compressed_size": 50,
            "uncompressed_size": 100
        }"#;
        let rg: RowGroupMeta = serde_json::from_str(json)
            .expect("should deserialize legacy format");
        assert!(rg.column_encodings.is_empty());
        assert_eq!(rg.num_rows, 100);
    }

    #[test]
    fn backward_compat_capabilities_no_new_fields() {
        let json = r#"{
            "column_pruning": true,
            "predicate_pushdown": true,
            "column_statistics": true,
            "bloom_filters": false,
            "nested_columns": false
        }"#;
        let caps: FormatCapabilities = serde_json::from_str(json)
            .expect("should deserialize legacy format");
        assert!(caps.column_pruning);
        assert!(!caps.dictionary_encoding);
        assert!(!caps.late_materialization);
        assert!(!caps.encoding_metadata);
    }
}
