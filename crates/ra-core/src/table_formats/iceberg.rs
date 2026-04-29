//! Stub Apache Iceberg table format implementation.
//!
//! This provides a minimal [`IcebergFormat`] that satisfies the
//! [`TableFormat`] trait for testing and development. A production
//! implementation would read Iceberg metadata files (manifest lists,
//! manifests, and table metadata JSON) from the catalog.

use std::collections::HashMap;

use super::{
    DataFile, FileStats, PartitionSpec, Result, Snapshot, TableFormat, TableFormatError,
    TableFormatType,
};

/// In-memory registration for a single table.
#[derive(Debug, Clone)]
struct RegisteredTable {
    files: Vec<DataFile>,
    partition_spec: Option<PartitionSpec>,
    snapshots: Vec<Snapshot>,
}

/// Stub Iceberg format for development and testing.
///
/// Tables can be registered in-memory via [`IcebergFormat::register_table`].
/// Querying an unregistered table returns [`TableFormatError::TableNotFound`].
#[derive(Debug, Clone)]
pub struct IcebergFormat {
    tables: HashMap<String, RegisteredTable>,
}

impl IcebergFormat {
    /// Create an empty Iceberg format with no registered tables.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
        }
    }

    /// Register a table with its data files and optional partition spec.
    pub fn register_table(
        &mut self,
        name: String,
        files: Vec<DataFile>,
        partition_spec: Option<PartitionSpec>,
    ) {
        self.tables.insert(
            name,
            RegisteredTable {
                files,
                partition_spec,
                snapshots: Vec::new(),
            },
        );
    }

    /// Register a table with snapshots for time-travel testing.
    pub fn register_table_with_snapshots(
        &mut self,
        name: String,
        files: Vec<DataFile>,
        partition_spec: Option<PartitionSpec>,
        snapshots: Vec<Snapshot>,
    ) {
        self.tables.insert(
            name,
            RegisteredTable {
                files,
                partition_spec,
                snapshots,
            },
        );
    }

    fn get_table(&self, table: &str) -> Result<&RegisteredTable> {
        self.tables
            .get(table)
            .ok_or_else(|| TableFormatError::TableNotFound {
                name: table.to_owned(),
            })
    }
}

impl Default for IcebergFormat {
    fn default() -> Self {
        Self::new()
    }
}

impl TableFormat for IcebergFormat {
    fn format_type(&self) -> TableFormatType {
        TableFormatType::Iceberg
    }

    fn list_files(&self, table: &str) -> Result<Vec<DataFile>> {
        Ok(self.get_table(table)?.files.clone())
    }

    fn file_statistics(&self, _file: &DataFile) -> Option<FileStats> {
        // Stub: a real implementation would read per-file stats
        // from the Iceberg manifest entries.
        None
    }

    fn supports_time_travel(&self) -> bool {
        true
    }

    fn partition_spec(&self, table: &str) -> Result<Option<PartitionSpec>> {
        Ok(self.get_table(table)?.partition_spec.clone())
    }

    fn list_snapshots(&self, table: &str) -> Result<Vec<Snapshot>> {
        Ok(self.get_table(table)?.snapshots.clone())
    }

    fn current_snapshot_id(&self, table: &str) -> Result<Option<u64>> {
        let snapshots = &self.get_table(table)?.snapshots;
        Ok(snapshots.last().map(|s| s.snapshot_id))
    }
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_files() -> Vec<DataFile> {
        vec![
            DataFile::new(
                "data/part-0001.parquet".to_owned(),
                10 * 1024 * 1024,
                100_000,
            )
            .with_partition("date".to_owned(), "2024-01-15".to_owned()),
            DataFile::new(
                "data/part-0002.parquet".to_owned(),
                12 * 1024 * 1024,
                120_000,
            )
            .with_partition("date".to_owned(), "2024-01-16".to_owned()),
        ]
    }

    fn sample_partition_spec() -> PartitionSpec {
        PartitionSpec {
            spec_id: 0,
            fields: vec![super::super::PartitionField {
                source_column: "event_time".to_owned(),
                transform: super::super::PartitionTransform::Day,
                partition_name: "date".to_owned(),
            }],
        }
    }

    fn sample_snapshots() -> Vec<Snapshot> {
        let mut s1_summary = HashMap::new();
        s1_summary.insert("operation".to_owned(), "append".to_owned());
        let mut s2_summary = HashMap::new();
        s2_summary.insert("operation".to_owned(), "overwrite".to_owned());

        vec![
            Snapshot {
                snapshot_id: 1,
                timestamp_ms: 1_700_000_000_000,
                summary: s1_summary,
            },
            Snapshot {
                snapshot_id: 2,
                timestamp_ms: 1_700_001_000_000,
                summary: s2_summary,
            },
        ]
    }

    #[test]
    fn default_is_empty() {
        let fmt = IcebergFormat::default();
        assert!(fmt.tables.is_empty());
    }

    #[test]
    fn register_and_list_files() {
        let mut fmt = IcebergFormat::new();
        fmt.register_table("events".to_owned(), sample_files(), None);

        let files = fmt
            .list_files("events")
            .expect("should list registered files");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].record_count, 100_000);
        assert_eq!(files[1].record_count, 120_000);
    }

    #[test]
    fn partition_spec_round_trip() {
        let mut fmt = IcebergFormat::new();
        fmt.register_table(
            "events".to_owned(),
            sample_files(),
            Some(sample_partition_spec()),
        );

        let spec = fmt
            .partition_spec("events")
            .expect("should succeed")
            .expect("should have partition spec");
        assert_eq!(spec.spec_id, 0);
        assert_eq!(spec.fields.len(), 1);
        assert_eq!(spec.fields[0].source_column, "event_time");
    }

    #[test]
    fn snapshot_listing() {
        let mut fmt = IcebergFormat::new();
        fmt.register_table_with_snapshots(
            "events".to_owned(),
            sample_files(),
            None,
            sample_snapshots(),
        );

        let snapshots = fmt.list_snapshots("events").expect("should list snapshots");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].snapshot_id, 1);
        assert_eq!(snapshots[1].snapshot_id, 2);
    }

    #[test]
    fn current_snapshot_is_latest() {
        let mut fmt = IcebergFormat::new();
        fmt.register_table_with_snapshots(
            "events".to_owned(),
            sample_files(),
            None,
            sample_snapshots(),
        );

        let id = fmt
            .current_snapshot_id("events")
            .expect("should succeed")
            .expect("should have a current snapshot");
        assert_eq!(id, 2);
    }

    #[test]
    fn current_snapshot_none_when_no_snapshots() {
        let mut fmt = IcebergFormat::new();
        fmt.register_table("events".to_owned(), sample_files(), None);

        let id = fmt.current_snapshot_id("events").expect("should succeed");
        assert!(id.is_none());
    }

    #[test]
    #[expect(
        clippy::unwrap_used,
        reason = "test code intentionally checks error cases"
    )]
    fn missing_table_errors() {
        let fmt = IcebergFormat::new();

        let err = fmt.list_files("missing").unwrap_err();
        assert!(matches!(err, TableFormatError::TableNotFound { .. }));

        let err = fmt.partition_spec("missing").unwrap_err();
        assert!(matches!(err, TableFormatError::TableNotFound { .. }));

        let err = fmt.list_snapshots("missing").unwrap_err();
        assert!(matches!(err, TableFormatError::TableNotFound { .. }));
    }
}
