//! `SQLite` database adapter implementation with FTS5 and sqlite-vec support.

#[cfg(feature = "sqlite")]
use crate::{
    AdapterError, ColumnInfo, DatabaseAdapter, DatabaseCapabilities, ForeignKeyInfo, IndexInfo,
    SchemaInfo, TableInfo,
};
#[cfg(feature = "sqlite")]
use ra_core::{FactsProvider, SqlDialect};
#[cfg(feature = "sqlite")]
use ra_stats::types::{ColumnStats, TableStats};
#[cfg(feature = "sqlite")]
use std::collections::HashMap;
#[cfg(feature = "sqlite")]
use std::sync::Mutex;
#[cfg(feature = "sqlite")]
use std::time::Instant;

#[cfg(feature = "sqlite")]
use r2d2::Pool;
#[cfg(feature = "sqlite")]
use r2d2_sqlite::SqliteConnectionManager;
#[cfg(feature = "sqlite")]
use rusqlite::{OpenFlags, Row};
#[cfg(feature = "sqlite")]
use std::path::Path;

/// `SQLite` database adapter.
///
/// Connects to `SQLite` databases to gather schema information, statistics,
/// and detect FTS5 and sqlite-vec extensions for hybrid search capabilities.
///
/// # Features
///
/// - Connection pooling via r2d2
/// - FTS5 full-text search detection
/// - sqlite-vec vector search detection
/// - Schema introspection via `sqlite_master`
/// - Statistics from ANALYZE tables
#[cfg(feature = "sqlite")]
pub struct SQLiteAdapter {
    connection_string: Option<String>,
    pool: Option<Pool<SqliteConnectionManager>>,
    facts: Mutex<SQLiteFacts>,
}

#[cfg(feature = "sqlite")]
impl std::fmt::Debug for SQLiteAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SQLiteAdapter")
            .field("connection_string", &self.connection_string)
            .field("pool", &self.pool.as_ref().map(|_| "<connected>"))
            .finish_non_exhaustive()
    }
}

/// Internal storage for gathered facts.
#[cfg(feature = "sqlite")]
#[derive(Debug, Clone)]
struct SQLiteFacts {
    #[expect(dead_code, reason = "adapter scaffolding")]
    table_stats: HashMap<String, ra_core::facts::TableStats>,
    #[expect(dead_code, reason = "adapter scaffolding")]
    column_stats: HashMap<(String, String), ra_core::statistics::ColumnStats>,
    #[expect(dead_code, reason = "adapter scaffolding")]
    schemas: HashMap<String, ra_core::facts::TableInfo>,
    #[expect(dead_code, reason = "adapter scaffolding")]
    hardware: ra_core::facts::HardwareProfile,
    features: HashMap<String, bool>,
}

#[cfg(feature = "sqlite")]
impl SQLiteFacts {
    fn new() -> Self {
        Self {
            table_stats: HashMap::new(),
            column_stats: HashMap::new(),
            schemas: HashMap::new(),
            hardware: ra_core::facts::HardwareProfile {
                cpu_cores: 8,
                available_memory: 8 * 1024 * 1024 * 1024,
                total_memory: 8 * 1024 * 1024 * 1024,
                simd_width: 256,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 32 * 1024,
                l2_cache_size: 256 * 1024,
                l3_cache_size: 8 * 1024 * 1024,
            },
            features: HashMap::new(),
        }
    }
}

#[cfg(feature = "sqlite")]
impl SQLiteAdapter {
    /// Create a new `SQLite` adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            connection_string: None,
            pool: None,
            facts: Mutex::new(SQLiteFacts::new()),
        }
    }

    /// Check if FTS5 extension is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn check_fts5(&self) -> Result<bool, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        // Check if fts5 is in the compile options
        let result = conn
            .query_row(
                "SELECT 1 FROM pragma_compile_options WHERE compile_options LIKE '%FTS5%'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        Ok(result)
    }

    /// Check if sqlite-vec extension is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn check_sqlite_vec(&self) -> Result<bool, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        // Try to use vec_distance_l2 function - if it exists, sqlite-vec is loaded
        let result = conn
            .query_row("SELECT vec_distance_l2('[1,2,3]', '[4,5,6]')", [], |_| {
                Ok(true)
            })
            .is_ok();

        Ok(result)
    }

    /// Get all FTS5 virtual tables in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn get_fts5_tables(&self) -> Result<Vec<String>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type='table' AND sql LIKE '%USING fts5%'",
            )
            .map_err(|e| AdapterError::QueryError(format!("Failed to prepare statement: {e}")))?;

        let tables = stmt
            .query_map([], |row: &Row| row.get::<_, String>(0))
            .map_err(|e| AdapterError::QueryError(format!("Failed to query FTS5 tables: {e}")))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| AdapterError::QueryError(format!("Failed to collect results: {e}")))?;

        Ok(tables)
    }

    /// Get all tables with vector columns (sqlite-vec).
    ///
    /// This detects tables that have BLOB columns used for vector storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn get_sqlite_vec_tables(&self) -> Result<Vec<String>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        // Look for tables with 'embedding' or 'vector' in column names (convention)
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT m.name
                 FROM sqlite_master m
                 JOIN pragma_table_info(m.name) p
                 WHERE m.type='table'
                 AND (p.name LIKE '%embedding%' OR p.name LIKE '%vector%')
                 AND p.type='BLOB'",
            )
            .map_err(|e| AdapterError::QueryError(format!("Failed to prepare statement: {e}")))?;

        let tables = stmt
            .query_map([], |row: &Row| row.get::<_, String>(0))
            .map_err(|e| AdapterError::QueryError(format!("Failed to query vector tables: {e}")))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| AdapterError::QueryError(format!("Failed to collect results: {e}")))?;

        Ok(tables)
    }

    /// Execute a query and return results compatible with ra-web API.
    ///
    /// This method returns a structure compatible with the `PostgreSQL` adapter's
    /// `execute()` method, including timing information.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn execute(&self, query: &str) -> Result<super::postgres::ExecutionResult, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let start = Instant::now();

        let mut stmt = conn
            .prepare(query)
            .map_err(|e| AdapterError::QueryError(format!("Failed to prepare statement: {e}")))?;

        let column_count = stmt.column_count();
        let column_names: Vec<String> = (0..column_count)
            .map(|i| stmt.column_name(i).unwrap_or("").to_string())
            .collect();

        let result_rows = stmt
            .query_map([], |row: &Row| {
                let mut map = serde_json::Map::new();
                for (i, name) in column_names.iter().enumerate() {
                    let value = row_value_to_json(row, i)?;
                    map.insert(name.clone(), value);
                }
                Ok(serde_json::Value::Object(map))
            })
            .map_err(|e| AdapterError::QueryError(format!("Failed to query: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AdapterError::QueryError(format!("Failed to collect results: {e}")))?;

        let duration = start.elapsed();
        let row_count = result_rows.len();

        Ok(super::postgres::ExecutionResult {
            rows: result_rows,
            row_count,
            execution_time_ms: duration.as_millis() as u64,
            plan: None,
        })
    }

    /// Get connection from pool for raw access.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or pool is exhausted.
    pub fn get_connection(
        &self,
    ) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        pool.get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))
    }
}

/// Convert a `SQLite` row value to JSON.
#[cfg(feature = "sqlite")]
fn row_value_to_json(row: &Row, idx: usize) -> Result<serde_json::Value, rusqlite::Error> {
    use rusqlite::types::ValueRef;

    match row.get_ref(idx)? {
        ValueRef::Null => Ok(serde_json::Value::Null),
        ValueRef::Integer(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::Real(f) => Ok(serde_json::Number::from_f64(f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number)),
        ValueRef::Text(s) => {
            let text = std::str::from_utf8(s).unwrap_or("");
            Ok(serde_json::Value::String(text.to_string()))
        }
        ValueRef::Blob(b) => {
            // Encode blob as base64
            Ok(serde_json::Value::String(base64_encode(b)))
        }
    }
}

/// Simple base64 encoding without external dependency.
#[cfg(feature = "sqlite")]
fn base64_encode(data: &[u8]) -> String {
    const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    let mut i = 0;

    while i < data.len() {
        let b1 = data[i];
        let b2 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b3 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        result.push(BASE64_CHARS[(b1 >> 2) as usize] as char);
        result.push(BASE64_CHARS[(((b1 & 0x03) << 4) | (b2 >> 4)) as usize] as char);
        result.push(if i + 1 < data.len() {
            BASE64_CHARS[(((b2 & 0x0f) << 2) | (b3 >> 6)) as usize] as char
        } else {
            '='
        });
        result.push(if i + 2 < data.len() {
            BASE64_CHARS[(b3 & 0x3f) as usize] as char
        } else {
            '='
        });

        i += 3;
    }

    result
}

#[cfg(feature = "sqlite")]
impl Default for SQLiteAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "sqlite")]
impl DatabaseAdapter for SQLiteAdapter {
    fn connect(&mut self, connection_string: &str) -> Result<(), AdapterError> {
        // Parse connection string - can be file path or :memory:
        let manager = if connection_string == ":memory:" {
            SqliteConnectionManager::memory()
        } else {
            let path = Path::new(connection_string);
            if !path.exists() {
                return Err(AdapterError::ConnectionError(format!(
                    "Database file does not exist: {connection_string}"
                )));
            }
            SqliteConnectionManager::file(connection_string)
                .with_flags(OpenFlags::SQLITE_OPEN_READ_WRITE)
        };

        let pool = Pool::builder()
            .max_size(20)
            .min_idle(Some(5))
            .connection_timeout(std::time::Duration::from_secs(5))
            .idle_timeout(Some(std::time::Duration::from_secs(300)))
            .max_lifetime(Some(std::time::Duration::from_secs(1800)))
            .build(manager)
            .map_err(|e| AdapterError::ConnectionError(format!("Failed to create pool: {e}")))?;

        // Test connection
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Failed to get connection: {e}")))?;

        // Verify it's a valid SQLite database
        conn.query_row("SELECT sqlite_version()", [], |row: &Row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| AdapterError::ConnectionError(format!("Invalid SQLite database: {e}")))?;

        self.connection_string = Some(connection_string.to_string());
        self.pool = Some(pool);

        // Detect capabilities
        let fts5_available = self.check_fts5().unwrap_or(false);
        let vec_available = self.check_sqlite_vec().unwrap_or(false);

        let mut facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;
        facts.features.insert("fts5".to_string(), fts5_available);
        facts
            .features
            .insert("sqlite-vec".to_string(), vec_available);

        Ok(())
    }

    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let mut stats = HashMap::new();

        // Get all regular tables (not views or virtual tables)
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?;

        let table_names: Vec<String> = stmt
            .query_map([], |row: &Row| row.get(0))
            .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AdapterError::QueryError(format!("Failed to collect tables: {e}")))?;

        for table_name in table_names {
            // Get row count
            let row_count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM \"{table_name}\""),
                    [],
                    |row: &Row| row.get(0),
                )
                .unwrap_or(0);

            stats.insert(
                table_name.clone(),
                TableStats {
                    row_count: row_count as u64,
                    page_count: 0, // SQLite doesn't expose page-level stats easily
                    average_row_size: 0.0,
                    table_size_bytes: 0,
                    live_tuples: None,
                    dead_tuples: None,
                    last_analyzed: None,
                },
            );
        }

        Ok(stats)
    }

    fn gather_column_stats(
        &self,
        table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let mut stats = HashMap::new();

        // Get column names
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{table}\")"))
            .map_err(|e| AdapterError::QueryError(format!("Failed to query table info: {e}")))?;

        let columns: Vec<String> = stmt
            .query_map([], |row: &Row| row.get::<_, String>(1))
            .map_err(|e| AdapterError::QueryError(format!("Failed to query columns: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AdapterError::QueryError(format!("Failed to collect columns: {e}")))?;

        // Get total rows for null fraction calculation
        let total_rows: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM \"{table}\""),
                [],
                |row: &Row| row.get(0),
            )
            .unwrap_or(0);

        for column in columns {
            // Get distinct count
            let distinct_count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(DISTINCT \"{column}\") FROM \"{table}\""),
                    [],
                    |row: &Row| row.get(0),
                )
                .unwrap_or(0);

            // Get null count
            let null_count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM \"{table}\" WHERE \"{column}\" IS NULL"),
                    [],
                    |row: &Row| row.get(0),
                )
                .unwrap_or(0);

            let null_fraction = if total_rows > 0 {
                null_count as f64 / total_rows as f64
            } else {
                0.0
            };

            stats.insert(
                column.clone(),
                ColumnStats {
                    column_id: column,
                    ndv: distinct_count as u64,
                    null_fraction,
                    avg_width: 0.0,
                    mcv: None,
                    histogram: None,
                    correlation: None,
                },
            );
        }

        Ok(stats)
    }

    #[expect(clippy::too_many_lines, reason = "schema query assembly")]
    fn get_schema_info(&self) -> Result<SchemaInfo, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let mut tables = HashMap::new();

        // Get all tables
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?;

        let table_names: Vec<String> = stmt
            .query_map([], |row: &Row| row.get(0))
            .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AdapterError::QueryError(format!("Failed to collect tables: {e}")))?;

        for table_name in table_names {
            // Get columns
            let mut col_stmt = conn
                .prepare(&format!("PRAGMA table_info(\"{table_name}\")"))
                .map_err(|e| {
                    AdapterError::QueryError(format!("Failed to query table info: {e}"))
                })?;

            let columns: Vec<ColumnInfo> = col_stmt
                .query_map([], |row: &Row| {
                    Ok(ColumnInfo {
                        name: row.get(1)?,
                        data_type: row.get(2)?,
                        nullable: row.get::<_, i32>(3)? == 0,
                        default_value: row.get(4).ok(),
                    })
                })
                .map_err(|e| AdapterError::QueryError(format!("Failed to query columns: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AdapterError::QueryError(format!("Failed to collect columns: {e}")))?;

            // Get primary key - need to check pk column in table_info
            let mut pk_stmt = conn
                .prepare(&format!("PRAGMA table_info(\"{table_name}\")"))
                .map_err(|e| {
                    AdapterError::QueryError(format!("Failed to query table info: {e}"))
                })?;
            let primary_key: Vec<String> = pk_stmt
                .query_map([], |row: &Row| {
                    let pk_flag: i32 = row.get(5)?;
                    if pk_flag > 0 {
                        row.get::<_, String>(1)
                    } else {
                        Err(rusqlite::Error::InvalidQuery)
                    }
                })
                .map_err(|e| {
                    AdapterError::QueryError(format!("Failed to query primary keys: {e}"))
                })?
                .filter_map(Result::ok)
                .collect();

            // Get foreign keys
            let mut fk_stmt = conn
                .prepare(&format!("PRAGMA foreign_key_list(\"{table_name}\")"))
                .map_err(|e| {
                    AdapterError::QueryError(format!("Failed to query foreign keys: {e}"))
                })?;

            let foreign_keys: Vec<ForeignKeyInfo> = fk_stmt
                .query_map([], |row: &Row| {
                    Ok(ForeignKeyInfo {
                        name: format!("fk_{}", row.get::<_, i32>(0)?),
                        columns: vec![row.get(3)?],
                        referenced_table: row.get(2)?,
                        referenced_columns: vec![row.get(4)?],
                    })
                })
                .map_err(|e| {
                    AdapterError::QueryError(format!("Failed to query foreign keys: {e}"))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| {
                    AdapterError::QueryError(format!("Failed to collect foreign keys: {e}"))
                })?;

            // Get indexes
            let mut idx_stmt = conn
                .prepare(&format!("PRAGMA index_list(\"{table_name}\")"))
                .map_err(|e| AdapterError::QueryError(format!("Failed to query indexes: {e}")))?;

            let indexes: Vec<IndexInfo> = idx_stmt
                .query_map([], |row: &Row| {
                    let index_name: String = row.get(1)?;
                    let unique: bool = row.get::<_, i32>(2)? == 1;

                    // Get index columns
                    let mut idx_col_stmt =
                        conn.prepare(&format!("PRAGMA index_info(\"{index_name}\")"))?;
                    let index_columns: Vec<String> = idx_col_stmt
                        .query_map([], |row: &Row| row.get(2))?
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap_or_default();

                    Ok(IndexInfo {
                        name: index_name,
                        columns: index_columns,
                        unique,
                        index_type: "btree".to_string(), // SQLite uses B-trees
                    })
                })
                .map_err(|e| AdapterError::QueryError(format!("Failed to query indexes: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AdapterError::QueryError(format!("Failed to collect indexes: {e}")))?;

            tables.insert(
                table_name.clone(),
                TableInfo {
                    name: table_name,
                    columns,
                    primary_key,
                    foreign_keys,
                    indexes,
                },
            );
        }

        Ok(SchemaInfo { tables })
    }

    fn get_capabilities(&self) -> Result<DatabaseCapabilities, AdapterError> {
        let facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;
        Ok(DatabaseCapabilities {
            database_name: "SQLite".to_string(),
            dialect: SqlDialect::Sqlite,
            features: facts.features.clone(),
            index_types: vec!["btree".to_string()],
            max_identifier_length: 64,
        })
    }

    fn supports_feature(&self, feature: &str) -> Result<bool, AdapterError> {
        let facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;
        Ok(facts.features.get(feature).copied().unwrap_or(false))
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Sqlite
    }

    fn database_name(&self) -> &str {
        "SQLite"
    }

    fn as_facts_provider(&self) -> &dyn FactsProvider {
        self
    }
}

#[cfg(feature = "sqlite")]
impl FactsProvider for SQLiteAdapter {
    fn get_table_stats(&self, _table_name: &str) -> Option<&ra_core::facts::TableStats> {
        // Cannot return reference from Mutex - need to redesign
        // For now, return None - this would need Arc or other approach
        None
    }

    fn get_column_stats(
        &self,
        _table_name: &str,
        _column_name: &str,
    ) -> Option<&ra_core::statistics::ColumnStats> {
        // Cannot return reference from Mutex - need to redesign
        None
    }

    fn hardware_profile(&self) -> &ra_core::facts::HardwareProfile {
        // Cannot return reference from Mutex - need to redesign
        // Return a static default for now
        &DEFAULT_HARDWARE
    }

    fn get_schema(&self, _table_name: &str) -> Option<&ra_core::facts::TableInfo> {
        // Cannot return reference from Mutex
        None
    }

    fn runtime_stats(&self, _operator_id: &str) -> Option<&ra_core::facts::OperatorStats> {
        None
    }

    fn database_name(&self) -> &'static str {
        "sqlite"
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        let Ok(facts) = self.facts.lock() else {
            return false;
        };
        facts.features.get(feature_name).copied().unwrap_or(false)
    }

    fn sql_dialect(&self) -> ra_core::SqlDialect {
        ra_core::SqlDialect::Sqlite
    }

    fn memory_limit(&self) -> Option<u64> {
        None
    }

    fn optimizer_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }
}

// Default hardware profile for when we can't return a reference
#[cfg(feature = "sqlite")]
static DEFAULT_HARDWARE: ra_core::facts::HardwareProfile = ra_core::facts::HardwareProfile {
    cpu_cores: 8,
    available_memory: 8 * 1024 * 1024 * 1024,
    total_memory: 8 * 1024 * 1024 * 1024,
    simd_width: 256,
    has_gpu: false,
    gpu_memory: None,
    l1_cache_size: 32 * 1024,
    l2_cache_size: 256 * 1024,
    l3_cache_size: 8 * 1024 * 1024,
};
