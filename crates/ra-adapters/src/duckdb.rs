//! `DuckDB` database adapter implementation with analytics benchmark capabilities.
//!
//! `DuckDB` is an embedded analytical database designed for OLAP workloads. This adapter
//! provides both standard database adapter capabilities and specialized benchmarking
//! features for comparing native `DuckDB` execution with Ra-optimized execution.

#[cfg_attr(not(feature = "duckdb"), expect(unused_imports))]
use crate::{
    AdapterError, ColumnInfo, DatabaseAdapter, DatabaseCapabilities, SchemaInfo, TableInfo,
};
#[cfg_attr(not(feature = "duckdb"), expect(unused_imports))]
use ra_core::{FactsProvider, SqlDialect};
#[cfg_attr(not(feature = "duckdb"), expect(unused_imports))]
use ra_stats::types::{ColumnStats, TableStats};
#[cfg_attr(not(feature = "duckdb"), expect(unused_imports))]
use std::time::Instant;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

#[cfg(feature = "duckdb")]
use duckdb::{Connection, Row};

/// `DuckDB` database adapter with benchmark capabilities.
///
/// Provides schema introspection, statistics gathering, and execution timing
/// for comparing native `DuckDB` execution with Ra-optimized execution.
///
/// # Features
///
/// - Embedded database (no connection pooling needed)
/// - Native support for Parquet, CSV, Arrow files
/// - Columnar storage optimization
/// - Parallel query execution
/// - Vectorized execution engine
/// - Benchmark comparison capabilities
pub struct DuckDBAdapter {
    connection_string: Option<String>,
    #[cfg(feature = "duckdb")]
    connection: Option<Mutex<Connection>>,
    #[cfg_attr(not(feature = "duckdb"), expect(dead_code))]
    facts: Mutex<DuckDBFacts>,
}

impl std::fmt::Debug for DuckDBAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("DuckDBAdapter");
        debug_struct.field("connection_string", &self.connection_string);
        #[cfg(feature = "duckdb")]
        {
            debug_struct.field(
                "connection",
                &self.connection.as_ref().map(|_| "<connected>"),
            );
        }
        debug_struct.finish_non_exhaustive()
    }
}

/// Internal storage for gathered facts.
#[derive(Debug, Clone)]
struct DuckDBFacts {
    #[expect(dead_code, reason = "adapter scaffolding")]
    table_stats: HashMap<String, ra_core::facts::TableStats>,
    #[expect(dead_code, reason = "adapter scaffolding")]
    column_stats: HashMap<(String, String), ra_core::statistics::ColumnStats>,
    #[expect(dead_code, reason = "adapter scaffolding")]
    schemas: HashMap<String, ra_core::facts::TableInfo>,
    #[expect(dead_code, reason = "adapter scaffolding")]
    hardware: ra_core::facts::HardwareProfile,
    #[cfg_attr(not(feature = "duckdb"), expect(dead_code))]
    features: HashMap<String, bool>,
}

impl DuckDBFacts {
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

/// Query execution result with timing information.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Execution duration
    pub duration: Duration,
    /// Number of rows returned
    pub row_count: usize,
    /// Result rows as JSON values
    pub rows: Vec<HashMap<String, serde_json::Value>>,
}

/// Explain plan information.
#[derive(Debug, Clone)]
pub struct ExplainPlan {
    /// Raw explain output
    pub plan_text: String,
    /// Estimated cost (if available)
    pub estimated_cost: Option<f64>,
    /// Estimated rows (if available)
    pub estimated_rows: Option<u64>,
}

/// Comparison metrics between native and Ra-optimized execution.
#[derive(Debug, Clone)]
pub struct ComparisonMetrics {
    /// Native `DuckDB` execution time
    pub native_duration: Duration,
    /// Ra-optimized execution time
    pub ra_duration: Duration,
    /// Speedup factor (`native_duration` / `ra_duration`)
    pub speedup: f64,
    /// Native `DuckDB` explain plan
    pub native_plan: ExplainPlan,
    /// Ra explain plan
    pub ra_plan: ExplainPlan,
    /// Number of rows returned (should be same for both)
    pub row_count: usize,
}

impl DuckDBAdapter {
    /// Create a new `DuckDB` adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            connection_string: None,
            #[cfg(feature = "duckdb")]
            connection: None,
            facts: Mutex::new(DuckDBFacts::new()),
        }
    }

    /// Open a `DuckDB` database file or in-memory database.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to database file, or ":memory:" for in-memory database
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened.
    #[cfg(feature = "duckdb")]
    pub fn open(&mut self, path: &str) -> Result<(), AdapterError> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()
        } else {
            Connection::open(path)
        }
        .map_err(|e| AdapterError::ConnectionError(format!("Failed to open DuckDB: {e}")))?;

        // Configure for optimal analytical performance
        conn.execute_batch(
            "SET threads TO 8;
             SET enable_optimizer TO true;
             SET enable_profiling TO false;
             SET enable_progress_bar TO false;",
        )
        .map_err(|e| AdapterError::ConnectionError(format!("Failed to configure DuckDB: {e}")))?;

        self.connection_string = Some(path.to_string());
        self.connection = Some(Mutex::new(conn));

        // Detect capabilities
        self.detect_capabilities()?;

        Ok(())
    }

    /// Execute a query and return results with timing.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    #[cfg(feature = "duckdb")]
    pub fn execute(&self, query: &str) -> Result<QueryResult, AdapterError> {
        let conn_mutex = self
            .connection
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let conn = conn_mutex
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex error: {e}")))?;

        let start = Instant::now();

        let mut stmt = conn
            .prepare(query)
            .map_err(|e| AdapterError::QueryError(format!("Failed to prepare statement: {e}")))?;

        let column_count = stmt.column_count();
        let column_names: Vec<String> = (0..column_count)
            .map(|i| {
                stmt.column_name(i)
                    .map_or(String::new(), ToString::to_string)
            })
            .collect();

        let rows = stmt
            .query_map([], |row| {
                let mut map = HashMap::new();
                for (i, name) in column_names.iter().enumerate() {
                    let value = row_value_to_json(row, i)?;
                    map.insert(name.clone(), value);
                }
                Ok(map)
            })
            .map_err(|e| AdapterError::QueryError(format!("Failed to execute query: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AdapterError::QueryError(format!("Failed to collect results: {e}")))?;

        let duration = start.elapsed();
        let row_count = rows.len();

        Ok(QueryResult {
            duration,
            row_count,
            rows,
        })
    }

    /// Execute query using native `DuckDB` optimizer.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    #[cfg(feature = "duckdb")]
    pub fn execute_native(&self, query: &str) -> Result<QueryResult, AdapterError> {
        self.execute(query)
    }

    /// Execute query with Ra optimization.
    ///
    /// This is a placeholder that currently executes the query normally.
    /// In a full implementation, this would:
    /// 1. Parse the SQL query
    /// 2. Optimize with Ra optimizer
    /// 3. Generate optimized plan
    /// 4. Execute the optimized plan
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    #[cfg(feature = "duckdb")]
    pub fn execute_with_ra(&self, query: &str) -> Result<QueryResult, AdapterError> {
        // TODO: Integrate with Ra optimizer
        // For now, just execute normally
        self.execute(query)
    }

    /// Get EXPLAIN plan for a query.
    ///
    /// # Errors
    ///
    /// Returns an error if the explain query fails.
    #[cfg(feature = "duckdb")]
    pub fn get_explain_plan(&self, query: &str) -> Result<ExplainPlan, AdapterError> {
        let explain_query = format!("EXPLAIN {query}");
        let result = self.execute(&explain_query)?;

        let mut plan_text = String::new();
        for row in &result.rows {
            if let Some(serde_json::Value::String(line)) = row.get("explain_value") {
                plan_text.push_str(line);
                plan_text.push('\n');
            }
        }

        Ok(ExplainPlan {
            plan_text,
            estimated_cost: None,
            estimated_rows: None,
        })
    }

    /// Get table statistics from `DuckDB`.
    ///
    /// # Errors
    ///
    /// Returns an error if statistics query fails.
    #[cfg(feature = "duckdb")]
    pub fn get_stats(&self, table: &str) -> Result<HashMap<String, String>, AdapterError> {
        let query = format!("SELECT * FROM duckdb_tables() WHERE table_name = '{table}'");
        let result = self.execute(&query)?;

        let mut stats = HashMap::new();
        if let Some(row) = result.rows.first() {
            for (key, value) in row {
                stats.insert(key.clone(), format!("{value:?}"));
            }
        }

        Ok(stats)
    }

    /// Compare native `DuckDB` execution with Ra-optimized execution.
    ///
    /// # Errors
    ///
    /// Returns an error if either execution fails.
    #[cfg(feature = "duckdb")]
    pub fn compare_execution(&self, query: &str) -> Result<ComparisonMetrics, AdapterError> {
        let native_result = self.execute_native(query)?;
        let native_plan = self.get_explain_plan(query)?;

        let ra_result = self.execute_with_ra(query)?;
        let ra_plan = self.get_explain_plan(query)?;

        let speedup = if ra_result.duration.as_secs_f64() > 0.0 {
            native_result.duration.as_secs_f64() / ra_result.duration.as_secs_f64()
        } else {
            1.0
        };

        Ok(ComparisonMetrics {
            native_duration: native_result.duration,
            ra_duration: ra_result.duration,
            speedup,
            native_plan,
            ra_plan,
            row_count: native_result.row_count,
        })
    }

    /// Load Parquet file into `DuckDB`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be loaded.
    #[cfg(feature = "duckdb")]
    pub fn load_parquet(&self, table_name: &str, file_path: &str) -> Result<(), AdapterError> {
        let query =
            format!("CREATE TABLE {table_name} AS SELECT * FROM read_parquet('{file_path}')");
        self.execute(&query)?;
        Ok(())
    }

    /// Load CSV file into `DuckDB`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be loaded.
    #[cfg(feature = "duckdb")]
    pub fn load_csv(&self, table_name: &str, file_path: &str) -> Result<(), AdapterError> {
        let query =
            format!("CREATE TABLE {table_name} AS SELECT * FROM read_csv_auto('{file_path}')");
        self.execute(&query)?;
        Ok(())
    }

    /// Detect `DuckDB` capabilities and features.
    #[cfg(feature = "duckdb")]
    fn detect_capabilities(&self) -> Result<(), AdapterError> {
        let mut facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;

        // DuckDB always has these features
        facts.features.insert("window_functions".to_string(), true);
        facts.features.insert("cte_recursive".to_string(), true);
        facts.features.insert("lateral_join".to_string(), true);
        facts.features.insert("parallel_query".to_string(), true);
        facts.features.insert("columnar_storage".to_string(), true);
        facts
            .features
            .insert("vectorized_execution".to_string(), true);
        facts.features.insert("parquet_support".to_string(), true);
        facts.features.insert("csv_support".to_string(), true);
        facts.features.insert("arrow_support".to_string(), true);
        facts
            .features
            .insert("aggregate_pushdown".to_string(), true);
        facts.features.insert("filter_pushdown".to_string(), true);

        Ok(())
    }

    /// Get connection for raw access.
    #[cfg(feature = "duckdb")]
    pub fn get_connection(&self) -> Result<&Mutex<Connection>, AdapterError> {
        self.connection
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))
    }
}

/// Convert a `DuckDB` row value to JSON.
#[cfg(feature = "duckdb")]
fn row_value_to_json(row: &Row, idx: usize) -> Result<serde_json::Value, duckdb::Error> {
    use duckdb::types::ValueRef;

    match row.get_ref(idx)? {
        ValueRef::Null => Ok(serde_json::Value::Null),
        ValueRef::Boolean(b) => Ok(serde_json::Value::Bool(b)),
        ValueRef::TinyInt(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::SmallInt(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::Int(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::BigInt(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::HugeInt(i) => Ok(serde_json::Value::String(i.to_string())), // i128 doesn't fit in JSON number
        ValueRef::UTinyInt(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::USmallInt(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::UInt(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::UBigInt(i) => Ok(serde_json::Value::Number(i.into())),
        ValueRef::Float(f) => Ok(serde_json::Number::from_f64(f64::from(f))
            .map_or(serde_json::Value::Null, serde_json::Value::Number)),
        ValueRef::Double(f) => Ok(serde_json::Number::from_f64(f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number)),
        ValueRef::Decimal(_) => {
            // Convert decimal to string representation
            let s: String = row.get(idx)?;
            Ok(serde_json::Value::String(s))
        }
        ValueRef::Timestamp(_, _) | ValueRef::Date32(_) | ValueRef::Time64(_, _) => {
            // Convert timestamp/date/time to string
            let s: String = row.get(idx)?;
            Ok(serde_json::Value::String(s))
        }
        ValueRef::Text(bytes) => {
            let text = std::str::from_utf8(bytes).unwrap_or("");
            Ok(serde_json::Value::String(text.to_string()))
        }
        ValueRef::Blob(b) => {
            // Encode blob as base64
            Ok(serde_json::Value::String(base64_encode(b)))
        }
        _ => {
            // For any other types, try to get as string
            let s: String = row.get(idx).unwrap_or_default();
            Ok(serde_json::Value::String(s))
        }
    }
}

/// Simple base64 encoding.
#[cfg(feature = "duckdb")]
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

impl Default for DuckDBAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "duckdb")]
impl DatabaseAdapter for DuckDBAdapter {
    fn connect(&mut self, connection_string: &str) -> Result<(), AdapterError> {
        self.open(connection_string)
    }

    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError> {
        #[cfg(feature = "duckdb")]
        {
            let conn_mutex = self
                .connection
                .as_ref()
                .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

            let conn = conn_mutex
                .lock()
                .map_err(|e| AdapterError::ConnectionError(format!("Mutex error: {e}")))?;

            let mut stats = HashMap::new();

            // Get all tables
            let mut stmt = conn
                .prepare("SELECT table_name FROM duckdb_tables() WHERE schema_name = 'main'")
                .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?;

            let table_names: Vec<String> = stmt
                .query_map([], |row| row.get(0))
                .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AdapterError::QueryError(format!("Failed to collect tables: {e}")))?;

            for table_name in table_names {
                // Get row count
                let row_count: i64 = conn
                    .query_row(
                        &format!("SELECT COUNT(*) FROM \"{table_name}\""),
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                stats.insert(
                    table_name.clone(),
                    TableStats {
                        row_count: row_count as u64,
                        page_count: 0,
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
        #[cfg(not(feature = "duckdb"))]
        {
            Err(AdapterError::UnsupportedFeature(
                "DuckDB feature not enabled".to_string(),
            ))
        }
    }

    fn gather_column_stats(
        &self,
        table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError> {
        #[cfg(feature = "duckdb")]
        {
            let conn_mutex = self
                .connection
                .as_ref()
                .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

            let conn = conn_mutex
                .lock()
                .map_err(|e| AdapterError::ConnectionError(format!("Mutex error: {e}")))?;

            let mut stats = HashMap::new();

            // Get column names
            let mut stmt = conn
                .prepare(&format!("DESCRIBE \"{table}\""))
                .map_err(|e| AdapterError::QueryError(format!("Failed to describe table: {e}")))?;

            let columns: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| AdapterError::QueryError(format!("Failed to query columns: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AdapterError::QueryError(format!("Failed to collect columns: {e}")))?;

            // Get total rows
            let total_rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);

            for column in columns {
                // Get distinct count
                let distinct_count: i64 = conn
                    .query_row(
                        &format!("SELECT COUNT(DISTINCT \"{column}\") FROM \"{table}\""),
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                // Get null count
                let null_count: i64 = conn
                    .query_row(
                        &format!("SELECT COUNT(*) FROM \"{table}\" WHERE \"{column}\" IS NULL"),
                        [],
                        |row| row.get(0),
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
        #[cfg(not(feature = "duckdb"))]
        {
            Err(AdapterError::UnsupportedFeature(
                "DuckDB feature not enabled".to_string(),
            ))
        }
    }

    fn get_schema_info(&self) -> Result<SchemaInfo, AdapterError> {
        #[cfg(feature = "duckdb")]
        {
            let conn_mutex = self
                .connection
                .as_ref()
                .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

            let conn = conn_mutex
                .lock()
                .map_err(|e| AdapterError::ConnectionError(format!("Mutex error: {e}")))?;

            let mut tables = HashMap::new();

            // Get all tables
            let mut stmt = conn
                .prepare("SELECT table_name FROM duckdb_tables() WHERE schema_name = 'main'")
                .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?;

            let table_names: Vec<String> = stmt
                .query_map([], |row| row.get(0))
                .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AdapterError::QueryError(format!("Failed to collect tables: {e}")))?;

            for table_name in table_names {
                // Get columns
                let mut col_stmt = conn
                    .prepare(&format!("DESCRIBE \"{table_name}\""))
                    .map_err(|e| {
                        AdapterError::QueryError(format!("Failed to describe table: {e}"))
                    })?;

                let columns: Vec<ColumnInfo> = col_stmt
                    .query_map([], |row| {
                        Ok(ColumnInfo {
                            name: row.get(0)?,
                            data_type: row.get(1)?,
                            nullable: row.get::<_, String>(2)?.to_uppercase() == "YES",
                            default_value: row.get(4).ok(),
                        })
                    })
                    .map_err(|e| AdapterError::QueryError(format!("Failed to query columns: {e}")))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        AdapterError::QueryError(format!("Failed to collect columns: {e}"))
                    })?;

                tables.insert(
                    table_name.clone(),
                    TableInfo {
                        name: table_name,
                        columns,
                        primary_key: Vec::new(),
                        foreign_keys: Vec::new(),
                        indexes: Vec::new(),
                    },
                );
            }

            Ok(SchemaInfo { tables })
        }
        #[cfg(not(feature = "duckdb"))]
        {
            Err(AdapterError::UnsupportedFeature(
                "DuckDB feature not enabled".to_string(),
            ))
        }
    }

    fn get_capabilities(&self) -> Result<DatabaseCapabilities, AdapterError> {
        let facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;
        Ok(DatabaseCapabilities {
            database_name: "DuckDB".to_string(),
            dialect: SqlDialect::Postgres,
            features: facts.features.clone(),
            index_types: vec!["art".to_string()],
            max_identifier_length: 128,
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
        SqlDialect::Postgres
    }

    fn database_name(&self) -> &str {
        "DuckDB"
    }

    fn as_facts_provider(&self) -> &dyn FactsProvider {
        self
    }
}

#[cfg(feature = "duckdb")]
impl FactsProvider for DuckDBAdapter {
    fn get_table_stats(&self, _table_name: &str) -> Option<&ra_core::facts::TableStats> {
        None
    }

    fn get_column_stats(
        &self,
        _table_name: &str,
        _column_name: &str,
    ) -> Option<&ra_core::statistics::ColumnStats> {
        None
    }

    fn hardware_profile(&self) -> &ra_core::facts::HardwareProfile {
        &DEFAULT_HARDWARE
    }

    fn get_schema(&self, _table_name: &str) -> Option<&ra_core::facts::TableInfo> {
        None
    }

    fn runtime_stats(&self, _operator_id: &str) -> Option<&ra_core::facts::OperatorStats> {
        None
    }

    fn database_name(&self) -> &'static str {
        "duckdb"
    }

    fn supports_feature(&self, feature_name: &str) -> bool {
        let Ok(facts) = self.facts.lock() else {
            return false;
        };
        facts.features.get(feature_name).copied().unwrap_or(false)
    }

    fn sql_dialect(&self) -> ra_core::SqlDialect {
        ra_core::SqlDialect::DuckDb
    }

    fn memory_limit(&self) -> Option<u64> {
        None
    }

    fn optimizer_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }
}

#[cfg(feature = "duckdb")]
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
