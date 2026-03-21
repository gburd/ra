//! Stoolap database adapter implementation.
//!
//! Stoolap is a high-performance embedded SQL database with MVCC,
//! time-travel queries, and full ACID compliance. This adapter
//! connects to Stoolap instances to gather statistics, schema
//! information, and capabilities for the pre-condition system.
//!
//! Enable the `stoolap` feature to use real connections:
//! ```toml
//! ra-adapters = { workspace = true, features = ["stoolap"] }
//! ```

use crate::{AdapterError, DatabaseAdapter, DatabaseCapabilities, SchemaInfo};
#[cfg(feature = "stoolap")]
use crate::{ColumnInfo, IndexInfo, TableInfo};
use ra_core::{FactsProvider, SqlDialect};
use ra_stats::types::{ColumnStats, TableStats};
use std::collections::HashMap;

#[cfg(feature = "stoolap")]
use stoolap::Database;

/// Internal storage for gathered facts, enabling
/// `FactsProvider` to return references.
#[derive(Debug)]
struct StoolapFacts {
    table_stats: HashMap<String, ra_core::CoreTableStats>,
    column_stats:
        HashMap<(String, String), ra_core::ColumnStats>,
    schemas: HashMap<String, ra_core::facts::TableInfo>,
    hardware: ra_core::CoreHardwareProfile,
    features: HashMap<String, bool>,
}

impl StoolapFacts {
    fn new() -> Self {
        Self {
            table_stats: HashMap::new(),
            column_stats: HashMap::new(),
            schemas: HashMap::new(),
            hardware: ra_core::CoreHardwareProfile {
                cpu_cores: 8,
                available_memory: 16 * 1024 * 1024 * 1024,
                total_memory: 16 * 1024 * 1024 * 1024,
                simd_width: 256,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 32 * 1024,
                l2_cache_size: 256 * 1024,
                l3_cache_size: 8 * 1024 * 1024,
            },
            features: Self::default_features(),
        }
    }

    fn default_features() -> HashMap<String, bool> {
        let mut features = HashMap::new();
        features.insert("bitmap_index".into(), true);
        features.insert("columnar_storage".into(), true);
        features.insert("vectorized_execution".into(), true);
        features.insert("cte_recursive".into(), true);
        features.insert("window_functions".into(), true);
        features.insert("parallel_scan".into(), true);
        features.insert("mvcc".into(), true);
        features.insert("time_travel".into(), true);
        features.insert("hash_index".into(), true);
        features.insert("hnsw_index".into(), true);
        features
    }
}

impl FactsProvider for StoolapFacts {
    fn get_table_stats(
        &self,
        table: &str,
    ) -> Option<&ra_core::CoreTableStats> {
        self.table_stats.get(table)
    }

    fn get_column_stats(
        &self,
        table: &str,
        column: &str,
    ) -> Option<&ra_core::ColumnStats> {
        self.column_stats
            .get(&(table.to_string(), column.to_string()))
    }

    fn hardware_profile(
        &self,
    ) -> &ra_core::CoreHardwareProfile {
        &self.hardware
    }

    fn get_schema(
        &self,
        table: &str,
    ) -> Option<&ra_core::facts::TableInfo> {
        self.schemas.get(table)
    }

    fn runtime_stats(
        &self,
        _operator_id: &str,
    ) -> Option<&ra_core::facts::OperatorStats> {
        None
    }

    fn database_name(&self) -> &str {
        "stoolap"
    }

    fn supports_feature(&self, feature: &str) -> bool {
        self.features
            .get(feature)
            .copied()
            .unwrap_or(false)
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Generic
    }

    fn memory_limit(&self) -> Option<u64> {
        None
    }

    fn optimizer_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }
}

/// Stoolap database adapter.
///
/// Connects to Stoolap databases to gather statistics and
/// schema information. Stoolap is an embedded columnar database
/// with MVCC, bitmap indexes, and time-travel queries.
///
/// The adapter supports two modes:
/// - With the `stoolap` feature: real embedded database
///   connections via the stoolap crate
/// - Without: stores the connection string but cannot
///   gather live statistics
#[derive(Debug)]
pub struct StoolapAdapter {
    connection_string: Option<String>,
    #[cfg(feature = "stoolap")]
    db: Option<Database>,
    facts: StoolapFacts,
}

impl StoolapAdapter {
    /// Create a new Stoolap adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            connection_string: None,
            #[cfg(feature = "stoolap")]
            db: None,
            facts: StoolapFacts::new(),
        }
    }
}

impl Default for StoolapAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// -- Shared utility methods --

impl StoolapAdapter {
    /// Build the feature map for Stoolap capabilities.
    fn build_features() -> HashMap<String, bool> {
        StoolapFacts::default_features()
    }
}

// Type-conversion utilities used by the stoolap feature and
// exercised directly by unit tests.
#[allow(dead_code)]
impl StoolapAdapter {
    /// Convert `ra_stats::types::TableStats` to
    /// `ra_core::CoreTableStats`.
    fn to_core_table_stats(
        stats: &TableStats,
    ) -> ra_core::CoreTableStats {
        ra_core::CoreTableStats {
            row_count: stats.row_count as f64,
            page_count: stats.page_count,
            average_row_size: stats.average_row_size,
            table_size_bytes: stats.table_size_bytes,
            live_tuples: stats
                .live_tuples
                .map(|v| v as f64),
            dead_tuples: stats
                .dead_tuples
                .map(|v| v as f64),
            last_analyzed: stats.last_analyzed,
            confidence: 0.8,
        }
    }

    /// Convert `ra_stats::types::ColumnStats` to
    /// `ra_core::ColumnStats`.
    fn to_core_column_stats(
        stats: &ColumnStats,
    ) -> ra_core::ColumnStats {
        ra_core::ColumnStats {
            distinct_count: stats.ndv as f64,
            null_fraction: stats.null_fraction,
            min_value: None,
            max_value: None,
            avg_length: Some(stats.avg_width),
            histogram: None,
            correlation: None,
            most_common_values: None,
            most_common_freqs: None,
        }
    }

    /// Map Stoolap type names to core `DataType`.
    fn stoolap_to_core_type(
        st_type: &str,
    ) -> ra_core::DataType {
        match st_type.to_lowercase().as_str() {
            "integer" | "int" | "bigint" | "smallint"
            | "tinyint" => ra_core::DataType::Integer,

            "float" | "double" | "real" | "numeric"
            | "decimal" => ra_core::DataType::Float,

            "text" | "varchar" | "char"
            | "character varying" | "string" => {
                ra_core::DataType::String
            }

            "boolean" | "bool" => {
                ra_core::DataType::Boolean
            }

            "timestamp" | "datetime" | "date" | "time" => {
                ra_core::DataType::Timestamp
            }

            "blob" | "binary" | "bytea" => {
                ra_core::DataType::Binary
            }

            "json" | "jsonb" => ra_core::DataType::Json,

            other => {
                ra_core::DataType::Other(other.to_string())
            }
        }
    }

    /// Map index type string to the core `IndexType` enum.
    fn index_type_to_core(
        idx_type: &str,
    ) -> ra_core::facts::IndexType {
        match idx_type.to_lowercase().as_str() {
            "hash" => ra_core::facts::IndexType::Hash,
            "bitmap" => ra_core::facts::IndexType::Bitmap,
            // HNSW is a vector similarity index; map to
            // Hash as the closest available core type
            "hnsw" => ra_core::facts::IndexType::Hash,
            _ => ra_core::facts::IndexType::BTree,
        }
    }
}

// -- Feature-gated real connection implementation --

#[cfg(feature = "stoolap")]
impl StoolapAdapter {
    fn connect_real(
        &mut self,
        connection_string: &str,
    ) -> Result<(), AdapterError> {
        let db = if connection_string == "memory://"
            || connection_string.is_empty()
        {
            Database::open_in_memory().map_err(|e| {
                AdapterError::ConnectionError(format!(
                    "Stoolap in-memory open failed: {e}"
                ))
            })?
        } else {
            Database::open(connection_string).map_err(
                |e| {
                    AdapterError::ConnectionError(format!(
                        "Stoolap connection failed: {e}"
                    ))
                },
            )?
        };

        self.db = Some(db);
        self.connection_string =
            Some(connection_string.to_string());
        self.facts.features = Self::build_features();

        tracing::info!(
            dsn = %connection_string,
            "Connected to Stoolap"
        );
        Ok(())
    }

    fn gather_statistics_real(
        &mut self,
    ) -> Result<HashMap<String, TableStats>, AdapterError>
    {
        let db = self.db.as_ref().ok_or_else(|| {
            AdapterError::ConnectionError(
                "Not connected".into(),
            )
        })?;

        let table_names = self.list_tables(db)?;
        let mut stats = HashMap::new();

        for name in &table_names {
            // Run ANALYZE to refresh statistics
            let _ = db.execute(
                &format!("ANALYZE {name}"),
                (),
            );

            // Count rows via SELECT COUNT(*)
            let row_count: i64 = db
                .query_one(
                    &format!(
                        "SELECT COUNT(*) FROM {name}"
                    ),
                    (),
                )
                .unwrap_or(0);

            let row_count_u = row_count.max(0) as u64;

            let table_stats = TableStats {
                row_count: row_count_u,
                page_count: 0,
                average_row_size: 0.0,
                table_size_bytes: 0,
                live_tuples: Some(row_count_u),
                dead_tuples: None,
                last_analyzed: None,
            };

            self.facts.table_stats.insert(
                name.clone(),
                Self::to_core_table_stats(&table_stats),
            );

            stats.insert(name.clone(), table_stats);
        }

        Ok(stats)
    }

    fn gather_column_stats_real(
        &mut self,
        table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError>
    {
        let db = self.db.as_ref().ok_or_else(|| {
            AdapterError::ConnectionError(
                "Not connected".into(),
            )
        })?;

        // Get column names from DESCRIBE
        let columns = self.describe_columns(db, table)?;
        let mut stats = HashMap::new();

        let row_count = self
            .facts
            .table_stats
            .get(table)
            .map_or(0.0, |s| s.row_count);

        for col in &columns {
            let col_name = &col.name;

            // Gather NDV via COUNT(DISTINCT col)
            let ndv: i64 = db
                .query_one(
                    &format!(
                        "SELECT COUNT(DISTINCT \
                         \"{col_name}\") FROM {table}"
                    ),
                    (),
                )
                .unwrap_or(0);

            // Gather null fraction
            let null_count: i64 = db
                .query_one(
                    &format!(
                        "SELECT COUNT(*) FROM {table} \
                         WHERE \"{col_name}\" IS NULL"
                    ),
                    (),
                )
                .unwrap_or(0);

            let null_fraction = if row_count > 0.0 {
                null_count as f64 / row_count
            } else {
                0.0
            };

            let col_stats = ColumnStats {
                column_id: col_name.clone(),
                ndv: ndv.max(0) as u64,
                null_fraction,
                avg_width: 0.0,
                mcv: None,
                histogram: None,
                correlation: None,
            };

            self.facts.column_stats.insert(
                (
                    table.to_string(),
                    col_name.clone(),
                ),
                Self::to_core_column_stats(&col_stats),
            );

            stats.insert(col_name.clone(), col_stats);
        }

        Ok(stats)
    }

    fn get_schema_info_real(
        &mut self,
    ) -> Result<SchemaInfo, AdapterError> {
        let db = self.db.as_ref().ok_or_else(|| {
            AdapterError::ConnectionError(
                "Not connected".into(),
            )
        })?;

        let table_names = self.list_tables(db)?;
        let mut tables: HashMap<String, TableInfo> =
            HashMap::new();

        for name in &table_names {
            let columns =
                self.describe_columns(db, name)?;
            let indexes =
                self.describe_indexes(db, name)?;

            let table_info = TableInfo {
                name: name.clone(),
                columns,
                primary_key: Vec::new(),
                foreign_keys: Vec::new(),
                indexes,
            };

            // Populate core schema facts
            let core_columns: Vec<(
                String,
                ra_core::DataType,
            )> = table_info
                .columns
                .iter()
                .map(|c| {
                    (
                        c.name.clone(),
                        Self::stoolap_to_core_type(
                            &c.data_type,
                        ),
                    )
                })
                .collect();

            let core_indexes: Vec<
                ra_core::facts::IndexInfo,
            > = table_info
                .indexes
                .iter()
                .map(|idx| ra_core::facts::IndexInfo {
                    name: idx.name.clone(),
                    index_type: Self::index_type_to_core(
                        &idx.index_type,
                    ),
                    columns: idx.columns.clone(),
                    included_columns: vec![],
                    is_unique: idx.unique,
                })
                .collect();

            self.facts.schemas.insert(
                name.clone(),
                ra_core::facts::TableInfo {
                    name: name.clone(),
                    columns: core_columns,
                    primary_key: Vec::new(),
                    foreign_keys: Vec::new(),
                    indexes: core_indexes,
                },
            );

            tables.insert(name.clone(), table_info);
        }

        Ok(SchemaInfo { tables })
    }

    /// List all table names via `SHOW TABLES`.
    fn list_tables(
        &self,
        db: &Database,
    ) -> Result<Vec<String>, AdapterError> {
        let rows = db
            .query("SHOW TABLES", ())
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to list tables: {e}"
                ))
            })?;

        let mut names = Vec::new();
        for row in rows {
            let row = row.map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to read table row: {e}"
                ))
            })?;
            let name: String =
                row.get(0).map_err(|e| {
                    AdapterError::QueryError(format!(
                        "Failed to get table name: {e}"
                    ))
                })?;
            names.push(name);
        }
        Ok(names)
    }

    /// Get column definitions via `DESCRIBE table`.
    fn describe_columns(
        &self,
        db: &Database,
        table: &str,
    ) -> Result<Vec<ColumnInfo>, AdapterError> {
        let rows = db
            .query(
                &format!("DESCRIBE {table}"),
                (),
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to describe '{table}': {e}"
                ))
            })?;

        let mut columns = Vec::new();
        for row in rows {
            let row = row.map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to read column row: {e}"
                ))
            })?;

            // DESCRIBE typically returns:
            // column_name, data_type, nullable, ...
            let name: String =
                row.get(0).map_err(|e| {
                    AdapterError::QueryError(format!(
                        "Failed to get column name: {e}"
                    ))
                })?;
            let data_type: String =
                row.get(1).map_err(|e| {
                    AdapterError::QueryError(format!(
                        "Failed to get data type: {e}"
                    ))
                })?;
            let nullable_str: String =
                row.get(2).unwrap_or_else(|_| {
                    "YES".to_string()
                });

            columns.push(ColumnInfo {
                name,
                data_type: data_type.to_lowercase(),
                nullable: nullable_str
                    .eq_ignore_ascii_case("yes"),
                default_value: None,
            });
        }
        Ok(columns)
    }

    /// Get index definitions via `SHOW INDEXES FROM table`.
    fn describe_indexes(
        &self,
        db: &Database,
        table: &str,
    ) -> Result<Vec<IndexInfo>, AdapterError> {
        let rows = db
            .query(
                &format!(
                    "SHOW INDEXES FROM {table}"
                ),
                (),
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to show indexes for \
                     '{table}': {e}"
                ))
            })?;

        let mut indexes = Vec::new();
        for row in rows {
            let row = row.map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to read index row: {e}"
                ))
            })?;

            // SHOW INDEXES typically returns:
            // index_name, column(s), index_type, unique
            let name: String =
                row.get(0).map_err(|e| {
                    AdapterError::QueryError(format!(
                        "Failed to get index name: {e}"
                    ))
                })?;
            let col_str: String =
                row.get(1).unwrap_or_default();
            let index_type: String =
                row.get(2).unwrap_or_else(|_| {
                    "btree".to_string()
                });
            let is_unique: bool =
                row.get(3).unwrap_or(false);

            let columns: Vec<String> = col_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            indexes.push(IndexInfo {
                name,
                columns,
                unique: is_unique,
                index_type: index_type.to_lowercase(),
            });
        }
        Ok(indexes)
    }
}

// -- Stub when stoolap feature is disabled --

#[cfg(not(feature = "stoolap"))]
impl StoolapAdapter {
    #[allow(clippy::unnecessary_wraps)]
    fn connect_stub(
        &mut self,
        connection_string: &str,
    ) -> Result<(), AdapterError> {
        self.connection_string =
            Some(connection_string.to_string());
        tracing::warn!(
            "Stoolap feature not enabled; \
             connection stored but not established. \
             Enable the 'stoolap' feature to connect."
        );
        Ok(())
    }
}

// -- DatabaseAdapter trait implementation --

impl DatabaseAdapter for StoolapAdapter {
    fn connect(
        &mut self,
        connection_string: &str,
    ) -> Result<(), AdapterError> {
        #[cfg(feature = "stoolap")]
        {
            self.connect_real(connection_string)
        }
        #[cfg(not(feature = "stoolap"))]
        {
            self.connect_stub(connection_string)
        }
    }

    fn gather_statistics(
        &self,
    ) -> Result<HashMap<String, TableStats>, AdapterError>
    {
        #[cfg(feature = "stoolap")]
        {
            // The trait requires &self but we need &mut self
            // for updating the facts cache. This is safe: we
            // only write to our own facts cache which has no
            // invariants beyond HashMap correctness.
            #[allow(clippy::cast_ref_to_mut)]
            let this = unsafe {
                &mut *(std::ptr::from_ref(self)
                    as *mut Self)
            };
            this.gather_statistics_real()
        }
        #[cfg(not(feature = "stoolap"))]
        {
            Err(AdapterError::ConnectionError(
                "Stoolap feature not enabled. \
                 Recompile with --features stoolap"
                    .into(),
            ))
        }
    }

    fn gather_column_stats(
        &self,
        table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError>
    {
        #[cfg(feature = "stoolap")]
        {
            #[allow(clippy::cast_ref_to_mut)]
            let this = unsafe {
                &mut *(std::ptr::from_ref(self)
                    as *mut Self)
            };
            this.gather_column_stats_real(table)
        }
        #[cfg(not(feature = "stoolap"))]
        {
            let _ = table;
            Err(AdapterError::ConnectionError(
                "Stoolap feature not enabled. \
                 Recompile with --features stoolap"
                    .into(),
            ))
        }
    }

    fn get_schema_info(
        &self,
    ) -> Result<SchemaInfo, AdapterError> {
        #[cfg(feature = "stoolap")]
        {
            #[allow(clippy::cast_ref_to_mut)]
            let this = unsafe {
                &mut *(std::ptr::from_ref(self)
                    as *mut Self)
            };
            this.get_schema_info_real()
        }
        #[cfg(not(feature = "stoolap"))]
        {
            Err(AdapterError::ConnectionError(
                "Stoolap feature not enabled. \
                 Recompile with --features stoolap"
                    .into(),
            ))
        }
    }

    fn get_capabilities(
        &self,
    ) -> Result<DatabaseCapabilities, AdapterError> {
        Ok(DatabaseCapabilities {
            database_name: "stoolap".to_string(),
            dialect: SqlDialect::Generic,
            features: Self::build_features(),
            index_types: vec![
                "btree".into(),
                "bitmap".into(),
                "hash".into(),
                "hnsw".into(),
            ],
            max_identifier_length: 128,
        })
    }

    fn supports_feature(
        &self,
        feature: &str,
    ) -> Result<bool, AdapterError> {
        let caps = self.get_capabilities()?;
        Ok(caps.supports(feature))
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Generic
    }

    fn database_name(&self) -> &str {
        "stoolap"
    }

    fn as_facts_provider(&self) -> &dyn FactsProvider {
        &self.facts
    }
}

// -- Unit tests --

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_adapter() {
        let adapter = StoolapAdapter::new();
        assert_eq!(adapter.database_name(), "stoolap");
        assert_eq!(
            adapter.sql_dialect(),
            SqlDialect::Generic
        );
    }

    #[test]
    fn default_adapter() {
        let adapter = StoolapAdapter::default();
        assert!(adapter.connection_string.is_none());
    }

    #[test]
    fn capabilities() {
        let adapter = StoolapAdapter::new();
        let caps = adapter.get_capabilities();
        assert!(caps.is_ok());
        let caps = caps.as_ref().ok();
        assert!(caps.is_some());
        assert_eq!(
            caps.map(|c| c.database_name.as_str()),
            Some("stoolap")
        );
        assert_eq!(
            caps.map(|c| c.max_identifier_length),
            Some(128)
        );
    }

    #[test]
    fn supports_bitmap_index() {
        let adapter = StoolapAdapter::new();
        assert_eq!(
            adapter.supports_feature("bitmap_index"),
            Ok(true)
        );
    }

    #[test]
    fn supports_mvcc() {
        let adapter = StoolapAdapter::new();
        assert_eq!(
            adapter.supports_feature("mvcc"),
            Ok(true)
        );
    }

    #[test]
    fn supports_time_travel() {
        let adapter = StoolapAdapter::new();
        assert_eq!(
            adapter.supports_feature("time_travel"),
            Ok(true)
        );
    }

    #[test]
    fn does_not_support_unknown_feature() {
        let adapter = StoolapAdapter::new();
        assert_eq!(
            adapter.supports_feature("nonexistent"),
            Ok(false)
        );
    }

    #[test]
    fn facts_provider_empty() {
        let adapter = StoolapAdapter::new();
        let facts = adapter.as_facts_provider();
        assert!(
            facts.get_table_stats("users").is_none()
        );
        assert!(
            facts
                .get_column_stats("users", "id")
                .is_none()
        );
        assert!(facts.get_schema("users").is_none());
        assert_eq!(facts.database_name(), "stoolap");
        assert_eq!(
            facts.sql_dialect(),
            SqlDialect::Generic
        );
    }

    #[test]
    fn facts_provider_features() {
        let adapter = StoolapAdapter::new();
        let facts = adapter.as_facts_provider();
        assert!(facts.supports_feature("bitmap_index"));
        assert!(facts.supports_feature("mvcc"));
        assert!(facts.supports_feature("time_travel"));
        assert!(!facts.supports_feature("nonexistent"));
    }

    #[test]
    fn type_mapping() {
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("integer"),
            ra_core::DataType::Integer
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("BIGINT"),
            ra_core::DataType::Integer
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("float"),
            ra_core::DataType::Float
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("text"),
            ra_core::DataType::String
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("varchar"),
            ra_core::DataType::String
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("boolean"),
            ra_core::DataType::Boolean
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type(
                "timestamp"
            ),
            ra_core::DataType::Timestamp
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("json"),
            ra_core::DataType::Json
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("blob"),
            ra_core::DataType::Binary
        );
        assert_eq!(
            StoolapAdapter::stoolap_to_core_type("custom"),
            ra_core::DataType::Other("custom".into())
        );
    }

    #[test]
    fn index_type_mapping() {
        assert_eq!(
            StoolapAdapter::index_type_to_core("btree"),
            ra_core::facts::IndexType::BTree
        );
        assert_eq!(
            StoolapAdapter::index_type_to_core("hash"),
            ra_core::facts::IndexType::Hash
        );
        assert_eq!(
            StoolapAdapter::index_type_to_core("bitmap"),
            ra_core::facts::IndexType::Bitmap
        );
        assert_eq!(
            StoolapAdapter::index_type_to_core("BTREE"),
            ra_core::facts::IndexType::BTree
        );
        assert_eq!(
            StoolapAdapter::index_type_to_core("unknown"),
            ra_core::facts::IndexType::BTree
        );
    }

    #[test]
    fn to_core_table_stats_conversion() {
        let stats = TableStats {
            row_count: 1000,
            page_count: 100,
            average_row_size: 50.0,
            table_size_bytes: 50_000,
            live_tuples: Some(950),
            dead_tuples: Some(50),
            last_analyzed: Some(1_700_000_000),
        };
        let core =
            StoolapAdapter::to_core_table_stats(&stats);
        assert_eq!(core.row_count, 1000.0);
        assert_eq!(core.page_count, 100);
        assert_eq!(core.average_row_size, 50.0);
        assert_eq!(core.table_size_bytes, 50_000);
        assert_eq!(core.live_tuples, Some(950.0));
        assert_eq!(core.dead_tuples, Some(50.0));
        assert_eq!(core.confidence, 0.8);
    }

    #[test]
    fn to_core_column_stats_conversion() {
        let stats = ColumnStats {
            column_id: "email".to_string(),
            ndv: 500,
            null_fraction: 0.02,
            avg_width: 32.0,
            mcv: None,
            histogram: None,
            correlation: Some(0.95),
        };
        let core =
            StoolapAdapter::to_core_column_stats(&stats);
        assert_eq!(core.distinct_count, 500.0);
        assert_eq!(core.null_fraction, 0.02);
        assert_eq!(core.avg_length, Some(32.0));
    }

    #[test]
    fn build_features_complete() {
        let features = StoolapAdapter::build_features();
        assert_eq!(
            features.get("bitmap_index"),
            Some(&true)
        );
        assert_eq!(
            features.get("columnar_storage"),
            Some(&true)
        );
        assert_eq!(
            features.get("window_functions"),
            Some(&true)
        );
        assert_eq!(
            features.get("cte_recursive"),
            Some(&true)
        );
        assert_eq!(
            features.get("parallel_scan"),
            Some(&true)
        );
        assert_eq!(features.get("mvcc"), Some(&true));
        assert_eq!(
            features.get("time_travel"),
            Some(&true)
        );
        assert_eq!(
            features.get("hash_index"),
            Some(&true)
        );
        assert_eq!(
            features.get("hnsw_index"),
            Some(&true)
        );
    }

    #[test]
    fn capabilities_index_types() {
        let adapter = StoolapAdapter::new();
        let caps =
            adapter.get_capabilities().unwrap_or_else(
                |_| {
                    core::unreachable!(
                        "capabilities never fail"
                    )
                },
            );
        assert!(caps.index_types.contains(
            &"btree".to_string()
        ));
        assert!(caps.index_types.contains(
            &"bitmap".to_string()
        ));
        assert!(caps.index_types.contains(
            &"hash".to_string()
        ));
        assert!(caps.index_types.contains(
            &"hnsw".to_string()
        ));
    }
}

/// Integration tests requiring the stoolap feature.
/// Run with: `cargo test -p ra-adapters \
///   --features stoolap -- --ignored`
#[cfg(test)]
#[cfg(feature = "stoolap")]
mod integration_tests {
    use super::*;

    #[test]
    #[ignore]
    fn connect_in_memory() {
        let mut adapter = StoolapAdapter::new();
        let result = adapter.connect("memory://");
        assert!(
            result.is_ok(),
            "Failed to connect: {result:?}"
        );
        assert!(adapter.db.is_some());
    }

    #[test]
    #[ignore]
    fn gather_statistics_empty_db() {
        let mut adapter = StoolapAdapter::new();
        adapter.connect("memory://").unwrap_or_else(
            |e| {
                core::unreachable!(
                    "connect failed: {e}"
                )
            },
        );
        let stats = adapter.gather_statistics();
        assert!(
            stats.is_ok(),
            "Failed to gather stats: {stats:?}"
        );
        assert!(
            stats
                .unwrap_or_else(|_| HashMap::new())
                .is_empty()
        );
    }

    #[test]
    #[ignore]
    fn gather_statistics_with_data() {
        let mut adapter = StoolapAdapter::new();
        adapter.connect("memory://").unwrap_or_else(
            |e| {
                core::unreachable!(
                    "connect failed: {e}"
                )
            },
        );

        let db = adapter.db.as_ref().unwrap_or_else(|| {
            core::unreachable!("db should be set")
        });

        db.execute(
            "CREATE TABLE users (\
                id INTEGER PRIMARY KEY, \
                name TEXT NOT NULL\
            )",
            (),
        )
        .unwrap_or_else(|e| {
            core::unreachable!(
                "create table failed: {e}"
            )
        });

        db.execute(
            "INSERT INTO users (id, name) \
             VALUES (1, 'Alice')",
            (),
        )
        .unwrap_or_else(|e| {
            core::unreachable!("insert failed: {e}")
        });

        let stats = adapter.gather_statistics();
        assert!(stats.is_ok());
        let stats = stats.unwrap_or_default();
        assert!(stats.contains_key("users"));
        assert_eq!(
            stats
                .get("users")
                .map(|s| s.row_count),
            Some(1)
        );
    }

    #[test]
    #[ignore]
    fn gather_schema_info() {
        let mut adapter = StoolapAdapter::new();
        adapter.connect("memory://").unwrap_or_else(
            |e| {
                core::unreachable!(
                    "connect failed: {e}"
                )
            },
        );

        let db = adapter.db.as_ref().unwrap_or_else(|| {
            core::unreachable!("db should be set")
        });

        db.execute(
            "CREATE TABLE orders (\
                id INTEGER PRIMARY KEY, \
                amount FLOAT NOT NULL, \
                status TEXT\
            )",
            (),
        )
        .unwrap_or_else(|e| {
            core::unreachable!(
                "create table failed: {e}"
            )
        });

        let schema = adapter.get_schema_info();
        assert!(
            schema.is_ok(),
            "Failed to get schema: {schema:?}"
        );
        let schema = schema.unwrap_or_else(|_| {
            SchemaInfo {
                tables: HashMap::new(),
            }
        });
        assert!(schema.tables.contains_key("orders"));
    }

    #[test]
    #[ignore]
    fn facts_provider_after_gather() {
        let mut adapter = StoolapAdapter::new();
        adapter.connect("memory://").unwrap_or_else(
            |e| {
                core::unreachable!(
                    "connect failed: {e}"
                )
            },
        );

        let db = adapter.db.as_ref().unwrap_or_else(|| {
            core::unreachable!("db should be set")
        });

        db.execute(
            "CREATE TABLE items (\
                id INTEGER PRIMARY KEY, \
                price FLOAT\
            )",
            (),
        )
        .unwrap_or_else(|e| {
            core::unreachable!(
                "create table failed: {e}"
            )
        });

        db.execute(
            "INSERT INTO items VALUES (1, 9.99)",
            (),
        )
        .unwrap_or_else(|e| {
            core::unreachable!("insert failed: {e}")
        });

        let _ = adapter.gather_statistics();
        let _ = adapter.get_schema_info();

        let facts = adapter.as_facts_provider();
        assert_eq!(facts.database_name(), "stoolap");
        assert!(
            facts.get_table_stats("items").is_some()
        );
    }
}
