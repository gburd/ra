//! Parquet file format implementation.
//!
//! Reads Apache Parquet file metadata (schema, row group statistics)
//! from the file footer without scanning row data. The optimizer
//! uses this information for predicate pushdown, row group filtering,
//! and cost estimation.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::time::SystemTime;

use parquet::basic::{Encoding, Repetition, Type as PhysicalType};
use parquet::file::metadata::RowGroupMetaData;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::file::statistics::Statistics as ParquetStatistics;
use parquet::schema::types::Type as SchemaType;

use crate::facts::DataType;

use super::{
    ColumnEncoding, ColumnEncodingInfo, CompressionCodec, Field, FileColumnStats, FileFormat,
    FileMetadata, FormatCapabilities, FormatError, RowGroupMeta, ScalarValue, Schema,
};

/// Parquet file format implementation.
///
/// Reads Apache Parquet file metadata (schema, row group stats)
/// from the file footer without scanning data.
#[derive(Debug, Clone)]
pub struct ParquetFormat {
    /// Default row group size hint for cost estimation.
    pub default_row_group_size: u64,
}

impl ParquetFormat {
    /// Create a new `ParquetFormat` with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            default_row_group_size: 1_000_000,
        }
    }
}

impl Default for ParquetFormat {
    fn default() -> Self {
        Self::new()
    }
}

impl FileFormat for ParquetFormat {
    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "parquet"
    }

    fn read_schema(&self, path: &Path) -> Result<Schema, FormatError> {
        let reader = open_parquet(path)?;
        let parquet_schema = reader.metadata().file_metadata().schema();
        Ok(convert_schema(parquet_schema))
    }

    fn read_metadata(&self, path: &Path) -> Result<FileMetadata, FormatError> {
        let reader = open_parquet(path)?;
        let parquet_meta = reader.metadata();
        let file_meta = parquet_meta.file_metadata();

        let schema = convert_schema(file_meta.schema());
        let num_rows = u64::try_from(file_meta.num_rows()).unwrap_or(0);

        let row_groups: Vec<RowGroupMeta> = parquet_meta
            .row_groups()
            .iter()
            .enumerate()
            .map(|(i, rg)| extract_row_group_meta(i, rg))
            .collect();

        let file_stats = aggregate_stats(&row_groups);

        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        Ok(FileMetadata {
            schema,
            num_rows,
            row_groups,
            file_stats,
            mtime,
        })
    }

    fn capabilities(&self) -> FormatCapabilities {
        FormatCapabilities {
            column_pruning: true,
            predicate_pushdown: true,
            column_statistics: true,
            bloom_filters: true,
            nested_columns: true,
            dictionary_encoding: true,
            late_materialization: true,
            encoding_metadata: true,
        }
    }

    fn extensions(&self) -> &[&str] {
        &["parquet", "pq"]
    }
}

fn open_parquet(path: &Path) -> Result<SerializedFileReader<File>, FormatError> {
    let file = File::open(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            FormatError::FileNotFound {
                path: path.display().to_string(),
            }
        } else {
            FormatError::Io {
                path: path.display().to_string(),
                source,
            }
        }
    })?;

    SerializedFileReader::new(file).map_err(|e| FormatError::InvalidFormat {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn convert_schema(schema: &SchemaType) -> Schema {
    let fields = match schema {
        SchemaType::GroupType { fields, .. } => fields.iter().map(|f| convert_field(f)).collect(),
        SchemaType::PrimitiveType { .. } => vec![],
    };
    Schema::new(fields)
}

fn convert_field(field: &SchemaType) -> Field {
    match field {
        SchemaType::PrimitiveType {
            basic_info,
            physical_type,
            ..
        } => {
            let data_type = map_physical_type(*physical_type);
            let name = basic_info.name().to_string();
            let nullable =
                !basic_info.has_repetition() || basic_info.repetition() == Repetition::OPTIONAL;
            if nullable {
                Field::new(name, data_type)
            } else {
                Field::non_null(name, data_type)
            }
        }
        SchemaType::GroupType { basic_info, .. } => {
            let name = basic_info.name().to_string();
            Field::new(name, DataType::Other("struct".into()))
        }
    }
}

fn map_physical_type(pt: PhysicalType) -> DataType {
    match pt {
        PhysicalType::BOOLEAN => DataType::Boolean,
        PhysicalType::INT32 | PhysicalType::INT64 => DataType::Integer,
        PhysicalType::FLOAT | PhysicalType::DOUBLE => DataType::Float,
        PhysicalType::BYTE_ARRAY | PhysicalType::FIXED_LEN_BYTE_ARRAY => DataType::Binary,
        PhysicalType::INT96 => DataType::Timestamp,
    }
}

fn extract_row_group_meta(index: usize, rg: &RowGroupMetaData) -> RowGroupMeta {
    let mut column_stats = HashMap::new();
    let mut column_encodings = HashMap::new();

    for col in rg.columns() {
        let col_path = col.column_path().string();
        if let Some(stats) = col.statistics() {
            column_stats.insert(col_path.clone(), convert_statistics(stats));
        }

        let encoding_info = extract_column_encoding(col);
        column_encodings.insert(col_path, encoding_info);
    }

    let num_rows = u64::try_from(rg.num_rows()).unwrap_or(0);
    let compressed_size = u64::try_from(rg.compressed_size()).unwrap_or(0);

    // Calculate offset from the first column chunk
    let offset = rg
        .columns()
        .first()
        .map_or(0, |c| u64::try_from(c.file_offset()).unwrap_or(0));

    // Sum uncompressed sizes across columns
    let uncompressed_size: u64 = rg
        .columns()
        .iter()
        .map(|c| u64::try_from(c.uncompressed_size()).unwrap_or(0))
        .sum();

    RowGroupMeta {
        index,
        offset,
        num_rows,
        column_stats,
        compressed_size,
        uncompressed_size,
        column_encodings,
    }
}

fn extract_column_encoding(
    col: &parquet::file::metadata::ColumnChunkMetaData,
) -> ColumnEncodingInfo {
    let encodings = col.encodings();
    let is_dict = encodings
        .iter()
        .any(|e| *e == Encoding::PLAIN_DICTIONARY || *e == Encoding::RLE_DICTIONARY);

    let encoding = if is_dict {
        ColumnEncoding::Dictionary
    } else if encodings.contains(&Encoding::DELTA_BINARY_PACKED)
        || encodings.contains(&Encoding::DELTA_LENGTH_BYTE_ARRAY)
        || encodings.contains(&Encoding::DELTA_BYTE_ARRAY)
    {
        ColumnEncoding::DeltaEncoding
    } else if encodings.contains(&Encoding::RLE) {
        ColumnEncoding::RunLength
    } else if encodings.contains(&Encoding::BYTE_STREAM_SPLIT) {
        ColumnEncoding::ByteStreamSplit
    } else {
        // Note: BIT_PACKED is deprecated - RLE hybrid handles bit-packing
        ColumnEncoding::Plain
    };

    let compression = convert_compression(col.compression());

    let compressed_bytes = u64::try_from(col.compressed_size()).unwrap_or(0);
    let uncompressed_bytes = u64::try_from(col.uncompressed_size()).unwrap_or(0);

    // Parquet doesn't directly expose dictionary size in column
    // chunk metadata, but we can estimate from distinct_count
    // in statistics if available.
    let dictionary_size = if is_dict {
        col.statistics()
            .and_then(parquet::file::statistics::Statistics::distinct_count_opt)
    } else {
        None
    };

    ColumnEncodingInfo {
        encoding,
        compression,
        dictionary_size,
        compressed_bytes,
        uncompressed_bytes,
    }
}

fn convert_compression(c: parquet::basic::Compression) -> CompressionCodec {
    match c {
        parquet::basic::Compression::UNCOMPRESSED => CompressionCodec::Uncompressed,
        parquet::basic::Compression::SNAPPY => CompressionCodec::Snappy,
        parquet::basic::Compression::GZIP(_) => CompressionCodec::Gzip,
        parquet::basic::Compression::LZO => CompressionCodec::Lzo,
        parquet::basic::Compression::BROTLI(_) => CompressionCodec::Brotli,
        parquet::basic::Compression::LZ4 => CompressionCodec::Lz4,
        parquet::basic::Compression::ZSTD(_) => CompressionCodec::Zstd,
        parquet::basic::Compression::LZ4_RAW => CompressionCodec::Lz4Raw,
    }
}

fn convert_statistics(stats: &ParquetStatistics) -> FileColumnStats {
    let null_count = stats.null_count_opt().unwrap_or(0);
    let distinct_count = stats.distinct_count_opt();

    let (min, max) = extract_min_max(stats);

    FileColumnStats {
        min,
        max,
        null_count,
        distinct_count,
    }
}

fn extract_min_max(stats: &ParquetStatistics) -> (Option<ScalarValue>, Option<ScalarValue>) {
    match stats {
        ParquetStatistics::Boolean(s) => (
            s.min_opt().map(|v| ScalarValue::Bool(*v)),
            s.max_opt().map(|v| ScalarValue::Bool(*v)),
        ),
        ParquetStatistics::Int32(s) => (
            s.min_opt().map(|v| ScalarValue::Int64(i64::from(*v))),
            s.max_opt().map(|v| ScalarValue::Int64(i64::from(*v))),
        ),
        ParquetStatistics::Int64(s) => (
            s.min_opt().map(|v| ScalarValue::Int64(*v)),
            s.max_opt().map(|v| ScalarValue::Int64(*v)),
        ),
        ParquetStatistics::Float(s) => (
            s.min_opt().map(|v| ScalarValue::Float64(f64::from(*v))),
            s.max_opt().map(|v| ScalarValue::Float64(f64::from(*v))),
        ),
        ParquetStatistics::Double(s) => (
            s.min_opt().map(|v| ScalarValue::Float64(*v)),
            s.max_opt().map(|v| ScalarValue::Float64(*v)),
        ),
        ParquetStatistics::ByteArray(s) => (
            s.min_opt().map(|v| {
                String::from_utf8(v.data().to_vec()).map_or_else(
                    |_| ScalarValue::Binary(v.data().to_vec()),
                    ScalarValue::Utf8,
                )
            }),
            s.max_opt().map(|v| {
                String::from_utf8(v.data().to_vec()).map_or_else(
                    |_| ScalarValue::Binary(v.data().to_vec()),
                    ScalarValue::Utf8,
                )
            }),
        ),
        ParquetStatistics::FixedLenByteArray(s) => (
            s.min_opt().map(|v| ScalarValue::Binary(v.data().to_vec())),
            s.max_opt().map(|v| ScalarValue::Binary(v.data().to_vec())),
        ),
        ParquetStatistics::Int96(_) => (None, None),
    }
}

fn aggregate_stats(row_groups: &[RowGroupMeta]) -> HashMap<String, FileColumnStats> {
    let mut result: HashMap<String, FileColumnStats> = HashMap::new();

    for rg in row_groups {
        for (col_name, col_stats) in &rg.column_stats {
            let entry = result
                .entry(col_name.clone())
                .or_insert_with(FileColumnStats::empty);

            entry.null_count += col_stats.null_count;

            merge_min(&mut entry.min, col_stats.min.as_ref());
            merge_max(&mut entry.max, col_stats.max.as_ref());

            match (entry.distinct_count, col_stats.distinct_count) {
                (Some(a), Some(b)) => {
                    entry.distinct_count = Some(a.max(b));
                }
                (None, Some(b)) => {
                    entry.distinct_count = Some(b);
                }
                _ => {}
            }
        }
    }

    result
}

fn merge_min(current: &mut Option<ScalarValue>, candidate: Option<&ScalarValue>) {
    let Some(cand) = candidate else { return };
    match current {
        None => *current = Some(cand.clone()),
        Some(cur) => {
            if scalar_lt(cand, cur) {
                *current = Some(cand.clone());
            }
        }
    }
}

fn merge_max(current: &mut Option<ScalarValue>, candidate: Option<&ScalarValue>) {
    let Some(cand) = candidate else { return };
    match current {
        None => *current = Some(cand.clone()),
        Some(cur) => {
            if scalar_lt(cur, cand) {
                *current = Some(cand.clone());
            }
        }
    }
}

fn scalar_lt(a: &ScalarValue, b: &ScalarValue) -> bool {
    match (a, b) {
        (ScalarValue::Bool(a), ScalarValue::Bool(b)) => a < b,
        (ScalarValue::Int64(a), ScalarValue::Int64(b)) => a < b,
        (ScalarValue::Float64(a), ScalarValue::Float64(b)) => a < b,
        (ScalarValue::Utf8(a), ScalarValue::Utf8(b)) => a < b,
        (ScalarValue::Binary(a), ScalarValue::Binary(b)) => a < b,
        _ => false,
    }
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use parquet::basic::Compression;
    use parquet::data_type::ByteArray;
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::parser::parse_message_type;

    use super::*;

    fn write_test_parquet(path: &Path, num_rows: usize) {
        let schema_str = "
            message test_schema {
                REQUIRED INT64 id;
                OPTIONAL BYTE_ARRAY name (UTF8);
                REQUIRED DOUBLE score;
                OPTIONAL BOOLEAN active;
            }
        ";
        let schema = Arc::new(parse_message_type(schema_str).expect("should parse schema"));

        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();

        let file = File::create(path).expect("should create file");
        let mut writer =
            SerializedFileWriter::new(file, schema, Arc::new(props)).expect("should create writer");

        let mut rg_writer = writer.next_row_group().expect("should create row group");

        // Write id column
        {
            let mut col_writer = rg_writer
                .next_column()
                .expect("should get column writer")
                .expect("column writer should exist");
            let values: Vec<i64> = (0..num_rows as i64).collect();
            col_writer
                .typed::<parquet::data_type::Int64Type>()
                .write_batch(&values, None, None)
                .expect("should write id column");
            col_writer.close().expect("should close column writer");
        }

        // Write name column
        {
            let mut col_writer = rg_writer
                .next_column()
                .expect("should get column writer")
                .expect("column writer should exist");
            let values: Vec<ByteArray> = (0..num_rows)
                .map(|i| ByteArray::from(format!("name_{i}").as_str()))
                .collect();
            let def_levels: Vec<i16> = (0..num_rows).map(|_| 1).collect();
            col_writer
                .typed::<parquet::data_type::ByteArrayType>()
                .write_batch(&values, Some(&def_levels), None)
                .expect("should write name column");
            col_writer.close().expect("should close column writer");
        }

        // Write score column
        {
            let mut col_writer = rg_writer
                .next_column()
                .expect("should get column writer")
                .expect("column writer should exist");
            let values: Vec<f64> = (0..num_rows).map(|i| i as f64 * 1.5).collect();
            col_writer
                .typed::<parquet::data_type::DoubleType>()
                .write_batch(&values, None, None)
                .expect("should write score column");
            col_writer.close().expect("should close column writer");
        }

        // Write active column
        {
            let mut col_writer = rg_writer
                .next_column()
                .expect("should get column writer")
                .expect("column writer should exist");
            let values: Vec<bool> = (0..num_rows).map(|i| i % 2 == 0).collect();
            let def_levels: Vec<i16> = (0..num_rows).map(|_| 1).collect();
            col_writer
                .typed::<parquet::data_type::BoolType>()
                .write_batch(&values, Some(&def_levels), None)
                .expect("should write active column");
            col_writer.close().expect("should close column writer");
        }

        rg_writer.close().expect("should close row group");
        writer.close().expect("should close writer");
    }

    fn write_multi_row_group_parquet(path: &Path, rows_per_group: usize, num_groups: usize) {
        let schema_str = "
            message multi_schema {
                REQUIRED INT64 id;
                OPTIONAL BYTE_ARRAY label (UTF8);
            }
        ";
        let schema = Arc::new(parse_message_type(schema_str).expect("should parse schema"));

        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();

        let file = File::create(path).expect("should create file");
        let mut writer =
            SerializedFileWriter::new(file, schema, Arc::new(props)).expect("should create writer");

        for g in 0..num_groups {
            let base = (g * rows_per_group) as i64;
            let mut rg_writer = writer.next_row_group().expect("should create row group");

            // id column
            {
                let mut col = rg_writer
                    .next_column()
                    .expect("should get column writer")
                    .expect("column writer should exist");
                let values: Vec<i64> = (base..base + rows_per_group as i64).collect();
                col.typed::<parquet::data_type::Int64Type>()
                    .write_batch(&values, None, None)
                    .expect("should write id");
                col.close().expect("should close");
            }

            // label column
            {
                let mut col = rg_writer
                    .next_column()
                    .expect("should get column writer")
                    .expect("column writer should exist");
                let values: Vec<ByteArray> = (0..rows_per_group)
                    .map(|i| ByteArray::from(format!("label_{}", base + i as i64).as_str()))
                    .collect();
                let def_levels: Vec<i16> = vec![1; rows_per_group];
                col.typed::<parquet::data_type::ByteArrayType>()
                    .write_batch(&values, Some(&def_levels), None)
                    .expect("should write label");
                col.close().expect("should close");
            }

            rg_writer.close().expect("should close row group");
        }

        writer.close().expect("should close writer");
    }

    #[test]
    fn parquet_format_name() {
        let fmt = ParquetFormat::new();
        assert_eq!(fmt.name(), "parquet");
    }

    #[test]
    fn parquet_format_capabilities() {
        let fmt = ParquetFormat::new();
        let caps = fmt.capabilities();
        assert!(caps.column_pruning);
        assert!(caps.predicate_pushdown);
        assert!(caps.column_statistics);
        assert!(caps.bloom_filters);
        assert!(caps.nested_columns);
    }

    #[test]
    fn parquet_format_extensions() {
        let fmt = ParquetFormat::new();
        let exts = fmt.extensions();
        assert!(exts.contains(&"parquet"));
        assert!(exts.contains(&"pq"));
    }

    #[test]
    fn parquet_format_default() {
        let fmt = ParquetFormat::default();
        assert_eq!(fmt.default_row_group_size, 1_000_000);
    }

    #[test]
    fn read_schema_missing_file() {
        let fmt = ParquetFormat::new();
        let result = fmt.read_schema(Path::new("/nonexistent.parquet"));
        assert!(result.is_err());
        let err = result.expect_err("should fail on missing file");
        assert!(
            matches!(err, FormatError::FileNotFound { .. }),
            "expected FileNotFound, got: {err}"
        );
    }

    #[test]
    fn read_schema_invalid_file() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("bad.parquet");
        std::fs::write(&path, b"not a parquet file").expect("should write file");

        let fmt = ParquetFormat::new();
        let result = fmt.read_schema(&path);
        assert!(result.is_err());
        let err = result.expect_err("should fail on invalid file");
        assert!(
            matches!(err, FormatError::InvalidFormat { .. }),
            "expected InvalidFormat, got: {err}"
        );
    }

    #[test]
    fn read_schema_real_parquet() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("test.parquet");
        write_test_parquet(&path, 10);

        let fmt = ParquetFormat::new();
        let schema = fmt.read_schema(&path).expect("should read schema");

        assert_eq!(schema.num_fields(), 4);

        let id = schema.field_by_name("id").expect("id field should exist");
        assert_eq!(id.data_type, DataType::Integer);
        assert!(!id.nullable);

        let name = schema
            .field_by_name("name")
            .expect("name field should exist");
        assert_eq!(name.data_type, DataType::Binary);
        assert!(name.nullable);

        let score = schema
            .field_by_name("score")
            .expect("score field should exist");
        assert_eq!(score.data_type, DataType::Float);
        assert!(!score.nullable);

        let active = schema
            .field_by_name("active")
            .expect("active field should exist");
        assert_eq!(active.data_type, DataType::Boolean);
        assert!(active.nullable);
    }

    #[test]
    fn read_metadata_row_count() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("test.parquet");
        write_test_parquet(&path, 100);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        assert_eq!(meta.num_rows, 100);
        assert_eq!(meta.row_groups.len(), 1);
        assert_eq!(meta.row_groups[0].num_rows, 100);
        assert_eq!(meta.schema.num_fields(), 4);
    }

    #[test]
    fn read_metadata_column_statistics() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("stats.parquet");
        write_test_parquet(&path, 50);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        let rg = &meta.row_groups[0];

        let id_stats = rg.column_stats.get("id").expect("id stats should exist");
        assert_eq!(id_stats.min, Some(ScalarValue::Int64(0)));
        assert_eq!(id_stats.max, Some(ScalarValue::Int64(49)));
        assert_eq!(id_stats.null_count, 0);

        let score_stats = rg
            .column_stats
            .get("score")
            .expect("score stats should exist");
        assert_eq!(score_stats.min, Some(ScalarValue::Float64(0.0)));
        assert_eq!(score_stats.max, Some(ScalarValue::Float64(49.0 * 1.5)));

        let active_stats = rg
            .column_stats
            .get("active")
            .expect("active stats should exist");
        assert_eq!(active_stats.min, Some(ScalarValue::Bool(false)));
        assert_eq!(active_stats.max, Some(ScalarValue::Bool(true)));
    }

    #[test]
    fn read_metadata_file_level_stats() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("file_stats.parquet");
        write_test_parquet(&path, 30);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        let id_file_stats = meta
            .file_stats
            .get("id")
            .expect("file-level id stats should exist");
        assert_eq!(id_file_stats.min, Some(ScalarValue::Int64(0)));
        assert_eq!(id_file_stats.max, Some(ScalarValue::Int64(29)));
    }

    #[test]
    fn read_metadata_multiple_row_groups() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("multi_rg.parquet");
        write_multi_row_group_parquet(&path, 100, 3);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        assert_eq!(meta.num_rows, 300);
        assert_eq!(meta.row_groups.len(), 3);

        for (i, rg) in meta.row_groups.iter().enumerate() {
            assert_eq!(rg.index, i);
            assert_eq!(rg.num_rows, 100);
            assert!(rg.compressed_size > 0);
        }

        // Verify aggregated stats span all row groups
        let id_stats = meta
            .file_stats
            .get("id")
            .expect("file-level id stats should exist");
        assert_eq!(id_stats.min, Some(ScalarValue::Int64(0)));
        assert_eq!(id_stats.max, Some(ScalarValue::Int64(299)));
    }

    #[test]
    fn read_metadata_row_group_boundaries() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("boundaries.parquet");
        write_multi_row_group_parquet(&path, 50, 2);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        // First row group: ids 0..49
        let rg0_id = meta.row_groups[0]
            .column_stats
            .get("id")
            .expect("rg0 id stats should exist");
        assert_eq!(rg0_id.min, Some(ScalarValue::Int64(0)));
        assert_eq!(rg0_id.max, Some(ScalarValue::Int64(49)));

        // Second row group: ids 50..99
        let rg1_id = meta.row_groups[1]
            .column_stats
            .get("id")
            .expect("rg1 id stats should exist");
        assert_eq!(rg1_id.min, Some(ScalarValue::Int64(50)));
        assert_eq!(rg1_id.max, Some(ScalarValue::Int64(99)));
    }

    #[test]
    fn read_metadata_has_mtime() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("mtime.parquet");
        write_test_parquet(&path, 5);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        assert!(meta.mtime > SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn read_metadata_compressed_size_nonzero() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("compressed.parquet");
        write_test_parquet(&path, 100);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        assert!(meta.row_groups[0].compressed_size > 0);
        assert!(meta.row_groups[0].uncompressed_size > 0);
    }

    #[test]
    fn trait_object_usage() {
        let fmt: Box<dyn FileFormat> = Box::new(ParquetFormat::new());
        assert_eq!(fmt.name(), "parquet");
        assert!(fmt.capabilities().column_pruning);
    }

    #[test]
    fn scalar_comparison() {
        assert!(scalar_lt(&ScalarValue::Int64(1), &ScalarValue::Int64(2)));
        assert!(!scalar_lt(&ScalarValue::Int64(2), &ScalarValue::Int64(1)));
        assert!(!scalar_lt(&ScalarValue::Int64(1), &ScalarValue::Int64(1)));

        assert!(scalar_lt(
            &ScalarValue::Utf8("a".into()),
            &ScalarValue::Utf8("b".into())
        ));
        assert!(!scalar_lt(
            &ScalarValue::Int64(1),
            &ScalarValue::Utf8("b".into())
        ));
    }

    #[test]
    fn aggregate_stats_merges_correctly() {
        let rg1 = RowGroupMeta {
            index: 0,
            offset: 0,
            num_rows: 10,
            column_stats: HashMap::from([(
                "x".into(),
                FileColumnStats {
                    min: Some(ScalarValue::Int64(5)),
                    max: Some(ScalarValue::Int64(15)),
                    null_count: 2,
                    distinct_count: Some(10),
                },
            )]),
            compressed_size: 100,
            uncompressed_size: 200,
            column_encodings: HashMap::new(),
        };

        let rg2 = RowGroupMeta {
            index: 1,
            offset: 200,
            num_rows: 10,
            column_stats: HashMap::from([(
                "x".into(),
                FileColumnStats {
                    min: Some(ScalarValue::Int64(1)),
                    max: Some(ScalarValue::Int64(20)),
                    null_count: 3,
                    distinct_count: Some(8),
                },
            )]),
            compressed_size: 100,
            uncompressed_size: 200,
            column_encodings: HashMap::new(),
        };

        let agg = aggregate_stats(&[rg1, rg2]);
        let x = agg.get("x").expect("x should exist");
        assert_eq!(x.min, Some(ScalarValue::Int64(1)));
        assert_eq!(x.max, Some(ScalarValue::Int64(20)));
        assert_eq!(x.null_count, 5);
        assert_eq!(x.distinct_count, Some(10));
    }

    // ---- Format-aware: encoding extraction tests ----

    #[test]
    fn parquet_format_new_capabilities() {
        let fmt = ParquetFormat::new();
        let caps = fmt.capabilities();
        assert!(caps.dictionary_encoding);
        assert!(caps.late_materialization);
        assert!(caps.encoding_metadata);
    }

    #[test]
    fn read_metadata_has_encoding_info() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("encoding.parquet");
        write_test_parquet(&path, 50);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        let rg = &meta.row_groups[0];
        // Every column should have encoding info
        assert!(
            !rg.column_encodings.is_empty(),
            "should have encoding info for columns"
        );

        // id column should have encoding info
        let id_enc = rg
            .column_encodings
            .get("id")
            .expect("id encoding should exist");
        assert_eq!(id_enc.compression, CompressionCodec::Snappy);
        assert!(id_enc.compressed_bytes > 0);
        assert!(id_enc.uncompressed_bytes > 0);
    }

    #[test]
    fn read_metadata_compression_ratio() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("ratio.parquet");
        write_test_parquet(&path, 100);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        let ratio = meta.avg_compression_ratio();
        assert!(
            ratio >= 1.0,
            "compression ratio should be >= 1.0, got {ratio}"
        );
    }

    #[test]
    fn zone_map_pruning_on_parquet() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("zonemap.parquet");
        write_multi_row_group_parquet(&path, 100, 3);

        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        // Row groups: [0..99], [100..199], [200..299]
        // Looking for id=50: only row group 0 survives
        let surviving = meta.surviving_row_groups("id", &ScalarValue::Int64(50));
        assert_eq!(surviving, vec![0]);

        let pruned = meta.prunable_row_groups("id", &ScalarValue::Int64(50));
        assert_eq!(pruned, vec![1, 2]);
    }

    #[test]
    fn write_dict_encoded_parquet_and_read_encoding() {
        let dir = tempfile::tempdir().expect("should create temp dir");
        let path = dir.path().join("dict.parquet");

        // Write a parquet file with dictionary encoding enabled
        let schema_str = "
            message dict_schema {
                REQUIRED BYTE_ARRAY status (UTF8);
            }
        ";
        let schema = Arc::new(parse_message_type(schema_str).expect("should parse schema"));

        let props = WriterProperties::builder()
            .set_dictionary_enabled(true)
            .set_compression(Compression::SNAPPY)
            .build();

        let file = File::create(&path).expect("should create file");
        let mut writer =
            SerializedFileWriter::new(file, schema, Arc::new(props)).expect("should create writer");

        let mut rg_writer = writer.next_row_group().expect("should create row group");
        {
            let mut col = rg_writer
                .next_column()
                .expect("should get column writer")
                .expect("column writer should exist");
            // Write low-cardinality values to trigger dictionary encoding
            let values: Vec<ByteArray> = (0..1000)
                .map(|i| {
                    let status = match i % 4 {
                        0 => "pending",
                        1 => "shipped",
                        2 => "delivered",
                        _ => "cancelled",
                    };
                    ByteArray::from(status)
                })
                .collect();
            col.typed::<parquet::data_type::ByteArrayType>()
                .write_batch(&values, None, None)
                .expect("should write status");
            col.close().expect("should close");
        }
        rg_writer.close().expect("should close row group");
        writer.close().expect("should close writer");

        // Read back and verify encoding
        let fmt = ParquetFormat::new();
        let meta = fmt.read_metadata(&path).expect("should read metadata");

        let rg = &meta.row_groups[0];
        let status_enc = rg
            .column_encodings
            .get("status")
            .expect("status encoding should exist");
        assert_eq!(
            status_enc.encoding,
            ColumnEncoding::Dictionary,
            "low-cardinality column should be dictionary-encoded"
        );
        assert!(rg.is_dictionary_encoded("status"));
    }
}
