//! `FactsProvider` implementation from timeline snapshots.
//!
//! This module provides `SnapshotFactsProvider` which implements the `FactsProvider`
//! trait using data from a timeline configuration snapshot. This allows the optimizer
//! to run against historical or simulated system states for:
//! - Deterministic testing
//! - What-if analysis
//! - Timeline-based demonstrations
//! - Regression testing with known environment states
//!
//! # Example
//!
//! ```
//! use ra_engine::timeline_config::{TimelineConfig, FingerPrintSnapshot};
//! use ra_engine::timeline_facts::SnapshotFactsProvider;
//! use ra_core::facts::FactsProvider;
//!
//! # fn example(config: TimelineConfig) {
//! let snapshot = &config.snapshots[0];
//! let hardware_profile = config.get_hardware_profile(&snapshot.hardware_profile).unwrap();
//! let facts = SnapshotFactsProvider::new(snapshot, hardware_profile);
//!
//! // Now use facts with optimizer
//! if let Some(stats) = facts.get_table_stats("orders") {
//!     println!("Orders has {} rows", stats.row_count);
//! }
//! # }
//! ```

use crate::timeline_config::{
    ColumnStatsDef, FingerPrintSnapshot, HardwareProfileDef, IndexDef, StatisticsSnapshot,
};
use ra_core::facts::{
    DataType, FactsProvider, ForeignKey, HardwareProfile, IndexInfo, OperatorStats, SqlDialect,
    TableInfo, TableStats,
};
use ra_core::statistics::ColumnStats;
use std::collections::HashMap;
use std::time::Duration;

/// `FactsProvider` implementation from a timeline snapshot.
///
/// Provides optimizer access to statistics, schema, hardware profile,
/// and facts from a specific point in the timeline.
pub struct SnapshotFactsProvider {
    /// Table statistics by table name.
    table_stats: HashMap<String, TableStats>,

    /// Column statistics by (table, column) name.
    column_stats: HashMap<(String, String), ColumnStats>,

    /// Hardware profile for this snapshot.
    hardware: HardwareProfile,

    /// Schema information by table name.
    schemas: HashMap<String, TableInfo>,

    /// Database name (from metadata or default).
    database_name: String,

    /// SQL dialect (from metadata or default).
    dialect: SqlDialect,

    /// Memory limit (from facts or hardware).
    memory_limit: Option<u64>,

    /// Supported features (from facts snapshot).
    features: HashMap<String, bool>,

    /// Optimizer timeout.
    timeout: Duration,
}

impl SnapshotFactsProvider {
    /// Create a new `SnapshotFactsProvider` from a timeline snapshot.
    #[must_use]
    pub fn new(snapshot: &FingerPrintSnapshot, hardware_profile: &HardwareProfileDef) -> Self {
        let hardware = hardware_profile.to_hardware_profile();

        // Build table stats
        let table_stats = build_table_stats(&snapshot.statistics);

        // Build column stats
        let column_stats = build_column_stats(&snapshot.statistics);

        // Build schemas
        let schemas = build_schemas(snapshot);

        // Extract features
        let features = extract_features(snapshot);

        // Determine memory limit (from facts or hardware)
        let memory_limit = snapshot
            .facts
            .work_mem_bytes
            .or(Some(hardware.available_memory));

        // Default timeout
        let timeout = Duration::from_secs(60);

        Self {
            table_stats,
            column_stats,
            hardware,
            schemas,
            database_name: "timeline".to_string(),
            dialect: SqlDialect::Generic,
            memory_limit,
            features,
            timeout,
        }
    }

    /// Set database name (override default).
    #[must_use]
    pub fn with_database_name(mut self, name: String) -> Self {
        self.database_name = name;
        self
    }

    /// Set SQL dialect (override default).
    #[must_use]
    pub fn with_dialect(mut self, dialect: SqlDialect) -> Self {
        self.dialect = dialect;
        self
    }

    /// Set optimizer timeout (override default).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl FactsProvider for SnapshotFactsProvider {
    fn get_table_stats(&self, table: &str) -> Option<&TableStats> {
        self.table_stats.get(table)
    }

    fn get_column_stats(&self, table: &str, column: &str) -> Option<&ColumnStats> {
        self.column_stats
            .get(&(table.to_string(), column.to_string()))
    }

    fn hardware_profile(&self) -> &HardwareProfile {
        &self.hardware
    }

    fn get_schema(&self, table: &str) -> Option<&TableInfo> {
        self.schemas.get(table)
    }

    fn runtime_stats(&self, _operator_id: &str) -> Option<&OperatorStats> {
        // Timeline snapshots don't contain runtime stats
        None
    }

    fn database_name(&self) -> &'static str {
        // Note: This returns a static str, but we have a String.
        // For now, return a generic name. In real implementation,
        // we'd use a static string pool or leak the string.
        "timeline"
    }

    fn supports_feature(&self, feature: &str) -> bool {
        self.features.get(feature).copied().unwrap_or(false)
    }

    fn sql_dialect(&self) -> SqlDialect {
        self.dialect
    }

    fn memory_limit(&self) -> Option<u64> {
        self.memory_limit
    }

    fn optimizer_timeout(&self) -> Duration {
        self.timeout
    }
}

/// Build table statistics from snapshot.
fn build_table_stats(stats: &StatisticsSnapshot) -> HashMap<String, TableStats> {
    let mut result = HashMap::new();

    for table in &stats.tables {
        let table_stats = TableStats {
            row_count: table.row_count as f64,
            page_count: table.page_count.unwrap_or(
                // Estimate page count from row count and avg row size
                (table.row_count * table.avg_row_size.unwrap_or(100.0) as u64) / 8192, // Assume 8KB pages
            ),
            average_row_size: table.avg_row_size.unwrap_or(100.0),
            table_size_bytes: table.table_size_bytes.unwrap_or(
                // Estimate from row count and avg row size
                table.row_count * table.avg_row_size.unwrap_or(100.0) as u64,
            ),
            live_tuples: Some(table.row_count as f64),
            dead_tuples: Some(0.0),
            last_analyzed: Some(0), // Timeline snapshots are always "fresh"
            confidence: 1.0,        // Perfect confidence for timeline data
            estimated_modifications: 0, // No modifications since "analysis"
        };

        result.insert(table.name.clone(), table_stats);
    }

    result
}

/// Build column statistics from snapshot.
fn build_column_stats(stats: &StatisticsSnapshot) -> HashMap<(String, String), ColumnStats> {
    let mut result = HashMap::new();

    for table in &stats.tables {
        for column in &table.columns {
            let col_stats = convert_column_stats(&table.name, column);
            result.insert((table.name.clone(), column.name.clone()), col_stats);
        }
    }

    result
}

/// Convert column stats definition to `ColumnStats`.
fn convert_column_stats(_table_name: &str, col: &ColumnStatsDef) -> ColumnStats {
    ColumnStats {
        distinct_count: col.ndv as f64,
        null_fraction: col.null_fraction,
        min_value: col.min_value.clone(),
        max_value: col.max_value.clone(),
        avg_length: Some(col.avg_width),
        histogram: None, // Not included in timeline format yet
        correlation: col.correlation,
        most_common_values: None, // Not included in timeline format yet
        most_common_freqs: None,  // Not included in timeline format yet
    }
}

/// Build schema information from snapshot.
fn build_schemas(snapshot: &FingerPrintSnapshot) -> HashMap<String, TableInfo> {
    let mut result = HashMap::new();

    for table in &snapshot.schema.tables {
        let columns: Vec<(String, DataType)> = table
            .columns
            .iter()
            .map(|col| (col.name.clone(), col.data_type.clone().into()))
            .collect();

        let indexes: Vec<IndexInfo> = table.indexes.iter().map(convert_index_def).collect();

        let foreign_keys: Vec<ForeignKey> = table
            .foreign_keys
            .iter()
            .map(|fk| ForeignKey {
                columns: fk.columns.clone(),
                referenced_table: fk.referenced_table.clone(),
                referenced_columns: fk.referenced_columns.clone(),
            })
            .collect();

        let table_info = TableInfo {
            name: table.name.clone(),
            columns,
            primary_key: table.primary_key.clone(),
            foreign_keys,
            indexes,
            storage_format: table.storage_format.into(),
        };

        result.insert(table.name.clone(), table_info);
    }

    result
}

/// Convert index definition to `IndexInfo`.
fn convert_index_def(idx: &IndexDef) -> IndexInfo {
    IndexInfo {
        name: idx.name.clone(),
        index_type: idx.index_type.into(),
        columns: idx.columns.clone(),
        included_columns: idx.included_columns.clone(),
        is_unique: idx.is_unique,
    }
}

/// Extract feature flags from facts snapshot.
fn extract_features(snapshot: &FingerPrintSnapshot) -> HashMap<String, bool> {
    let mut features = HashMap::new();

    // Add known features from facts
    if let Some(hash_join) = snapshot.facts.supports_hash_join {
        features.insert("hash_join".to_string(), hash_join);
    }

    if let Some(parallel_scan) = snapshot.facts.supports_parallel_scan {
        features.insert("parallel_scan".to_string(), parallel_scan);
    }

    // Additional features can be extracted from custom facts
    for (key, value) in &snapshot.facts.custom {
        if let toml::Value::Boolean(b) = value {
            features.insert(key.clone(), *b);
        }
    }

    features
}

#[cfg(test)]
#[expect(clippy::panic, clippy::unwrap_used, reason = "test code")]
#[expect(clippy::float_cmp, reason = "exact float literals in tests")]
mod tests {
    use super::*;
    use crate::timeline_config::{
        ColumnDef, DataTypeDef, FactsSnapshot, HardwareProfileDef, IndexDef, IndexTypeDef,
        SchemaSnapshot, StatisticsSnapshot, StorageFormatDef, TableDef, TableStatsDef,
    };

    fn create_test_hardware() -> HardwareProfileDef {
        HardwareProfileDef {
            name: "test".to_string(),
            cpu_cores: 8,
            total_memory: 16_000_000_000,
            available_memory: Some(12_000_000_000),
            simd_width: 256,
            has_gpu: false,
            gpu_memory: None,
            l1_cache_size: 32768,
            l2_cache_size: 262_144,
            l3_cache_size: 8_388_608,
        }
    }

    fn create_test_snapshot() -> FingerPrintSnapshot {
        FingerPrintSnapshot {
            time_offset: 0,
            label: Some("Test snapshot".to_string()),
            hardware_profile: "test".to_string(),
            schema: SchemaSnapshot {
                tables: vec![TableDef {
                    name: "orders".to_string(),
                    storage_format: StorageFormatDef::RowBased,
                    columns: vec![
                        ColumnDef {
                            name: "order_id".to_string(),
                            data_type: DataTypeDef::Integer,
                            nullable: false,
                        },
                        ColumnDef {
                            name: "customer_id".to_string(),
                            data_type: DataTypeDef::Integer,
                            nullable: false,
                        },
                    ],
                    indexes: vec![IndexDef {
                        name: "idx_orders_customer".to_string(),
                        index_type: IndexTypeDef::Btree,
                        columns: vec!["customer_id".to_string()],
                        included_columns: vec![],
                        is_unique: false,
                    }],
                    primary_key: vec!["order_id".to_string()],
                    foreign_keys: vec![],
                }],
            },
            statistics: StatisticsSnapshot {
                tables: vec![TableStatsDef {
                    name: "orders".to_string(),
                    row_count: 1_000_000,
                    page_count: Some(10_000),
                    avg_row_size: Some(100.0),
                    table_size_bytes: Some(100_000_000),
                    columns: vec![ColumnStatsDef {
                        name: "customer_id".to_string(),
                        ndv: 50_000,
                        null_fraction: 0.0,
                        avg_width: 8.0,
                        correlation: Some(0.1),
                        min_value: None,
                        max_value: None,
                    }],
                }],
            },
            facts: FactsSnapshot {
                supports_hash_join: Some(true),
                supports_parallel_scan: Some(true),
                parallel_workers: Some(4),
                work_mem_bytes: Some(64 * 1024 * 1024),
                custom: HashMap::new(),
            },
        }
    }

    #[test]
    fn snapshot_facts_provider_basic() {
        let hardware = create_test_hardware();
        let snapshot = create_test_snapshot();
        let facts = SnapshotFactsProvider::new(&snapshot, &hardware);

        // Check hardware profile
        assert_eq!(facts.cpu_cores(), 8);
        assert_eq!(facts.simd_width(), 256);

        // Check table stats
        let table_stats = facts.get_table_stats("orders").unwrap();
        assert_eq!(table_stats.row_count, 1_000_000.0);

        // Check column stats
        let col_stats = facts.get_column_stats("orders", "customer_id").unwrap();
        assert_eq!(col_stats.distinct_count, 50_000.0);

        // Check schema
        let schema = facts.get_schema("orders").unwrap();
        assert_eq!(schema.columns.len(), 2);
        assert_eq!(schema.indexes.len(), 1);
        assert_eq!(schema.primary_key, vec!["order_id"]);

        // Check features
        assert!(facts.supports_feature("hash_join"));
        assert!(facts.supports_feature("parallel_scan"));
    }

    #[test]
    fn snapshot_facts_provider_index_lookup() {
        let hardware = create_test_hardware();
        let snapshot = create_test_snapshot();
        let facts = SnapshotFactsProvider::new(&snapshot, &hardware);

        // Check index exists
        assert!(facts.has_index("orders", &["customer_id"], None));
        assert!(facts.has_index(
            "orders",
            &["customer_id"],
            Some(ra_core::facts::IndexType::BTree)
        ));
        assert!(!facts.has_index("orders", &["order_id"], None));
    }

    #[test]
    fn snapshot_facts_provider_missing_data() {
        let hardware = create_test_hardware();
        let snapshot = create_test_snapshot();
        let facts = SnapshotFactsProvider::new(&snapshot, &hardware);

        // Non-existent table
        assert!(facts.get_table_stats("customers").is_none());
        assert!(facts.get_schema("customers").is_none());

        // Non-existent column
        assert!(facts.get_column_stats("orders", "status").is_none());
    }

    #[test]
    fn snapshot_facts_provider_customization() {
        let hardware = create_test_hardware();
        let snapshot = create_test_snapshot();
        let facts = SnapshotFactsProvider::new(&snapshot, &hardware)
            .with_database_name("postgres".to_string())
            .with_dialect(SqlDialect::Postgres)
            .with_timeout(Duration::from_secs(30));

        assert_eq!(facts.sql_dialect(), SqlDialect::Postgres);
        assert_eq!(facts.optimizer_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn convert_data_types() {
        let int_type: DataType = DataTypeDef::Integer.into();
        assert_eq!(int_type, DataType::Integer);

        let array_type: DataType = DataTypeDef::Array(Box::new(DataTypeDef::String)).into();
        match array_type {
            DataType::Array(inner) => assert_eq!(*inner, DataType::String),
            _ => panic!("Expected array type"),
        }
    }

    #[test]
    fn convert_storage_format() {
        let parquet: ra_core::facts::StorageFormat = StorageFormatDef::Parquet.into();
        assert!(parquet.is_parquet());
        assert!(parquet.is_columnar());
        assert!(parquet.supports_metadata_pushdown());
    }
}
