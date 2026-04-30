//! `MySQL` database adapter implementation with r2d2 connection pooling.

#[cfg_attr(not(feature = "mysql"), expect(unused_imports))]
use crate::{
    AdapterError, ColumnInfo, DatabaseAdapter, DatabaseCapabilities, ForeignKeyInfo, IndexInfo,
    SchemaInfo, TableInfo,
};
use ra_core::{FactsProvider, SqlDialect};
use ra_stats::types::{ColumnStats, TableStats};
#[cfg_attr(not(feature = "mysql"), expect(unused_imports))]
use std::time::Instant;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

#[cfg(feature = "mysql")]
use mysql::{OptsBuilder, Pool};

#[cfg(feature = "mysql")]
use mysql::prelude::*;

/// `MySQL` execution result with timing information.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Number of rows returned or affected.
    pub row_count: usize,
    /// Execution time.
    pub duration: Duration,
    /// Query result rows.
    pub rows: Vec<HashMap<String, serde_json::Value>>,
}

/// `MySQL` EXPLAIN output.
#[derive(Debug, Clone)]
pub struct ExplainPlan {
    /// Plan as JSON.
    pub json: serde_json::Value,
    /// Human-readable plan.
    pub text: String,
}

/// `MySQL` database adapter.
///
/// Connects to `MySQL` databases to gather schema information, statistics,
/// and execute queries with performance metrics.
///
/// # Features
///
/// - Connection pooling via `r2d2_mysql`
/// - Native query execution with timing
/// - Ra-optimized query execution
/// - EXPLAIN FORMAT=JSON support
/// - FULLTEXT index detection
/// - Table and column statistics from `INFORMATION_SCHEMA`
pub struct MySQLAdapter {
    connection_string: Option<String>,
    #[cfg(feature = "mysql")]
    pool: Option<Pool>,
    facts: Mutex<MySQLFacts>,
}

impl std::fmt::Debug for MySQLAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "mysql")]
        {
            f.debug_struct("MySQLAdapter")
                .field("connection_string", &self.connection_string)
                .field("pool", &self.pool.as_ref().map(|_| "<connected>"))
                .finish_non_exhaustive()
        }
        #[cfg(not(feature = "mysql"))]
        {
            f.debug_struct("MySQLAdapter")
                .field("connection_string", &self.connection_string)
                .finish_non_exhaustive()
        }
    }
}

/// Internal storage for gathered facts.
#[derive(Debug, Clone)]
struct MySQLFacts {
    #[cfg_attr(not(feature = "mysql"), expect(dead_code))]
    table_stats: HashMap<String, ra_core::CoreTableStats>,
    #[cfg_attr(not(feature = "mysql"), expect(dead_code))]
    column_stats: HashMap<(String, String), ra_core::ColumnStats>,
    #[cfg_attr(not(feature = "mysql"), expect(dead_code))]
    schemas: HashMap<String, ra_core::facts::TableInfo>,
    #[expect(dead_code, reason = "adapter scaffolding")]
    hardware: ra_core::CoreHardwareProfile,
    features: HashMap<String, bool>,
}

impl MySQLFacts {
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
            features: HashMap::new(),
        }
    }
}

impl MySQLAdapter {
    /// Create a new `MySQL` adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            connection_string: None,
            #[cfg(feature = "mysql")]
            pool: None,
            facts: Mutex::new(MySQLFacts::new()),
        }
    }

    /// Execute a query with native `MySQL` execution and timing.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or query execution fails.
    #[cfg(feature = "mysql")]
    pub fn execute_native(&self, query: &str) -> Result<ExecutionResult, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let start = Instant::now();
        let result: Vec<mysql::Row> = conn
            .query(query)
            .map_err(|e| AdapterError::QueryError(format!("Query failed: {e}")))?;
        let duration = start.elapsed();

        let row_count = result.len();
        let rows = result
            .into_iter()
            .map(|row| {
                let mut map = HashMap::new();
                for (idx, col) in row.columns_ref().iter().enumerate() {
                    let value = mysql_value_to_json(&row, idx);
                    map.insert(col.name_str().to_string(), value);
                }
                map
            })
            .collect();

        Ok(ExecutionResult {
            row_count,
            duration,
            rows,
        })
    }

    /// Execute a query directly against native `MySQL` (stub when feature disabled).
    ///
    /// # Errors
    ///
    /// Always returns `AdapterError::UnsupportedFeature` when the mysql feature is disabled.
    #[cfg(not(feature = "mysql"))]
    pub fn execute_native(&self, _query: &str) -> Result<ExecutionResult, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    /// Execute a query and return results compatible with ra-web API.
    ///
    /// This method is similar to `execute_native` but returns a structure
    /// compatible with the `PostgreSQL` adapter's `execute()` method.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or query execution fails.
    #[cfg(feature = "mysql")]
    pub fn execute(&self, query: &str) -> Result<super::postgres::ExecutionResult, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let start = Instant::now();
        let result: Vec<mysql::Row> = conn
            .query(query)
            .map_err(|e| AdapterError::QueryError(format!("Query failed: {e}")))?;
        let duration = start.elapsed();

        let row_count = result.len();
        let rows: Vec<serde_json::Value> = result
            .into_iter()
            .map(|row| {
                let mut map = serde_json::Map::new();
                for (idx, col) in row.columns_ref().iter().enumerate() {
                    let value = mysql_value_to_json(&row, idx);
                    map.insert(col.name_str().to_string(), value);
                }
                serde_json::Value::Object(map)
            })
            .collect();

        Ok(super::postgres::ExecutionResult {
            rows,
            row_count,
            execution_time_ms: duration.as_millis() as u64,
            plan: None,
        })
    }

    /// Execute a query with Ra optimization (stub when feature disabled).
    ///
    /// # Errors
    ///
    /// Always returns `AdapterError::UnsupportedFeature` when the mysql feature is disabled.
    #[cfg(not(feature = "mysql"))]
    pub fn execute(&self, _query: &str) -> Result<super::postgres::ExecutionResult, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    /// Execute a query with Ra optimization.
    ///
    /// This would parse the query, optimize it with Ra, and execute the optimized version.
    /// For now, this is a placeholder that just calls `execute_native`.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or query execution fails.
    pub fn execute_with_ra(&self, query: &str) -> Result<ExecutionResult, AdapterError> {
        // TODO: Integrate with Ra optimizer
        // 1. Parse query with ra-parser
        // 2. Optimize with ra-core
        // 3. Execute optimized query
        tracing::info!("Ra optimization not yet integrated, using native execution");
        self.execute_native(query)
    }

    /// Get EXPLAIN plan for a query.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or EXPLAIN fails.
    #[cfg(feature = "mysql")]
    pub fn get_explain_plan(&self, query: &str) -> Result<ExplainPlan, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        // Get JSON format explain
        let explain_query = format!("EXPLAIN FORMAT=JSON {query}");
        let json_result: Vec<mysql::Row> = conn
            .query(&explain_query)
            .map_err(|e| AdapterError::QueryError(format!("EXPLAIN failed: {e}")))?;

        let json = if let Some(row) = json_result.first() {
            let json_str: String = row
                .get(0)
                .ok_or_else(|| AdapterError::QueryError("No EXPLAIN output".to_string()))?;
            serde_json::from_str(&json_str)
                .map_err(|e| AdapterError::QueryError(format!("Invalid JSON: {e}")))?
        } else {
            serde_json::Value::Null
        };

        // Get text format explain
        let text_query = format!("EXPLAIN {query}");
        let text_result: Vec<mysql::Row> = conn
            .query(&text_query)
            .map_err(|e| AdapterError::QueryError(format!("EXPLAIN failed: {e}")))?;

        let text = text_result
            .iter()
            .map(|row| {
                let mut parts = Vec::new();
                for idx in 0..row.len() {
                    if let Some(val) = mysql_value_to_string(row, idx) {
                        parts.push(val);
                    }
                }
                parts.join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ExplainPlan { json, text })
    }

    /// Get query execution plan (stub when feature disabled).
    ///
    /// # Errors
    ///
    /// Always returns `AdapterError::UnsupportedFeature` when the mysql feature is disabled.
    #[cfg(not(feature = "mysql"))]
    pub fn get_explain_plan(&self, _query: &str) -> Result<ExplainPlan, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    /// Check if a table has FULLTEXT indexes.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or query fails.
    #[cfg(feature = "mysql")]
    pub fn check_fulltext_indexes(&self, table: &str) -> Result<Vec<String>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let query = format!(
            "SELECT INDEX_NAME FROM INFORMATION_SCHEMA.STATISTICS \
             WHERE TABLE_SCHEMA = DATABASE() \
             AND TABLE_NAME = '{table}' \
             AND INDEX_TYPE = 'FULLTEXT'"
        );

        let indexes: Vec<String> = conn
            .query_map(query, |index_name: String| index_name)
            .map_err(|e| {
                AdapterError::QueryError(format!("Failed to check FULLTEXT indexes: {e}"))
            })?;

        Ok(indexes)
    }

    /// Check for full-text indexes on a table (stub when feature disabled).
    ///
    /// # Errors
    ///
    /// Always returns `AdapterError::UnsupportedFeature` when the mysql feature is disabled.
    #[cfg(not(feature = "mysql"))]
    pub fn check_fulltext_indexes(&self, _table: &str) -> Result<Vec<String>, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    /// Get query statistics from `MySQL`.
    ///
    /// # Errors
    ///
    /// Returns an error if not connected or query fails.
    #[cfg(feature = "mysql")]
    pub fn get_query_stats(&self) -> Result<HashMap<String, u64>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let mut stats = HashMap::new();

        // Get handler statistics
        let handler_stats: Vec<(String, u64)> = conn
            .query_map(
                "SHOW SESSION STATUS LIKE 'Handler%'",
                |(var_name, value): (String, String)| {
                    let parsed_value = value.parse::<u64>().unwrap_or(0);
                    (var_name, parsed_value)
                },
            )
            .map_err(|e| AdapterError::QueryError(format!("Failed to get handler stats: {e}")))?;

        for (name, value) in handler_stats {
            stats.insert(name, value);
        }

        // Get created tmp tables
        let tmp_stats: Vec<(String, u64)> = conn
            .query_map(
                "SHOW SESSION STATUS LIKE 'Created_tmp%'",
                |(var_name, value): (String, String)| {
                    let parsed_value = value.parse::<u64>().unwrap_or(0);
                    (var_name, parsed_value)
                },
            )
            .map_err(|e| AdapterError::QueryError(format!("Failed to get tmp table stats: {e}")))?;

        for (name, value) in tmp_stats {
            stats.insert(name, value);
        }

        Ok(stats)
    }

    /// Get query statistics (stub when feature disabled).
    ///
    /// # Errors
    ///
    /// Always returns `AdapterError::UnsupportedFeature` when the mysql feature is disabled.
    #[cfg(not(feature = "mysql"))]
    pub fn get_query_stats(&self) -> Result<HashMap<String, u64>, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    /// Convert `ra_stats::types::TableStats` to `ra_core::CoreTableStats`.
    #[cfg(feature = "mysql")]
    fn to_core_table_stats(stats: &TableStats) -> ra_core::CoreTableStats {
        ra_core::CoreTableStats {
            row_count: stats.row_count as f64,
            page_count: stats.page_count,
            average_row_size: stats.average_row_size,
            table_size_bytes: stats.table_size_bytes,
            live_tuples: stats.live_tuples.map(|v| v as f64),
            dead_tuples: stats.dead_tuples.map(|v| v as f64),
            last_analyzed: stats.last_analyzed,
            estimated_modifications: 0,
            confidence: 0.9,
        }
    }

    /// Convert `ra_stats::types::ColumnStats` to `ra_core::ColumnStats`.
    #[cfg(feature = "mysql")]
    fn to_core_column_stats(stats: &ColumnStats) -> ra_core::ColumnStats {
        ra_core::ColumnStats {
            distinct_count: stats.ndv as f64,
            null_fraction: stats.null_fraction,
            min_value: None,
            max_value: None,
            avg_length: Some(stats.avg_width),
            histogram: None,
            correlation: stats.correlation,
            most_common_values: None,
            most_common_freqs: None,
        }
    }

    /// Map `MySQL` type names to core `DataType`.
    #[cfg(any(feature = "mysql", test))]
    fn mysql_to_core_type(mysql_type: &str) -> ra_core::DataType {
        let lower = mysql_type.to_lowercase();
        match lower.as_str() {
            t if t.contains("int") => ra_core::DataType::Integer,
            t if t.contains("float") || t.contains("double") || t.contains("decimal") => {
                ra_core::DataType::Float
            }
            t if t.contains("char") || t.contains("text") || t.contains("enum") => {
                ra_core::DataType::String
            }
            t if t.contains("bool") => ra_core::DataType::Boolean,
            t if t.contains("date") || t.contains("time") => ra_core::DataType::Timestamp,
            t if t.contains("blob") || t.contains("binary") => ra_core::DataType::Binary,
            t if t.contains("json") => ra_core::DataType::Json,
            _ => ra_core::DataType::Other(mysql_type.to_string()),
        }
    }
}

#[cfg(feature = "mysql")]
fn mysql_value_to_json(row: &mysql::Row, idx: usize) -> serde_json::Value {
    use mysql::Value;

    match row.as_ref(idx) {
        Some(Value::Bytes(b)) => {
            if let Ok(s) = std::str::from_utf8(b) {
                serde_json::Value::String(s.to_string())
            } else {
                serde_json::Value::String(base64_encode(b))
            }
        }
        Some(Value::Int(i)) => serde_json::Value::Number((*i).into()),
        Some(Value::UInt(u)) => serde_json::Value::Number((*u).into()),
        Some(Value::Float(f)) => serde_json::Number::from_f64(f64::from(*f))
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Some(Value::Double(d)) => serde_json::Number::from_f64(*d)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Some(Value::Date(y, m, d, h, min, s, _us)) => {
            serde_json::Value::String(format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}"))
        }
        Some(Value::Time(neg, d, h, m, s, _us)) => {
            let sign = if *neg { "-" } else { "" };
            serde_json::Value::String(format!("{sign}{d} {h:02}:{m:02}:{s:02}"))
        }
        Some(Value::NULL) | None => serde_json::Value::Null,
    }
}

#[cfg(feature = "mysql")]
fn mysql_value_to_string(row: &mysql::Row, idx: usize) -> Option<String> {
    use mysql::Value;

    match row.as_ref(idx) {
        Some(Value::NULL) => Some("NULL".to_string()),
        Some(Value::Bytes(b)) => std::str::from_utf8(b).ok().map(ToString::to_string),
        Some(Value::Int(i)) => Some(i.to_string()),
        Some(Value::UInt(u)) => Some(u.to_string()),
        Some(Value::Float(f)) => Some(f.to_string()),
        Some(Value::Double(d)) => Some(d.to_string()),
        Some(Value::Date(y, m, d, h, min, s, _us)) => {
            Some(format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}"))
        }
        Some(Value::Time(neg, d, h, m, s, _us)) => {
            let sign = if *neg { "-" } else { "" };
            Some(format!("{sign}{d} {h:02}:{m:02}:{s:02}"))
        }
        None => None,
    }
}

/// Simple base64 encoding.
#[cfg(any(feature = "mysql", test))]
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

impl Default for MySQLAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "mysql")]
impl DatabaseAdapter for MySQLAdapter {
    fn connect(&mut self, connection_string: &str) -> Result<(), AdapterError> {
        let opts = OptsBuilder::from_opts(
            mysql::Opts::from_url(connection_string)
                .map_err(|e| AdapterError::InvalidConfiguration(format!("Invalid URL: {e}")))?,
        )
        .pool_opts(mysql::PoolOpts::default().with_constraints(
            mysql::PoolConstraints::new(5, 20).ok_or_else(|| {
                AdapterError::InvalidConfiguration("Invalid pool constraints".to_string())
            })?,
        ));

        let pool = Pool::new(opts)
            .map_err(|e| AdapterError::ConnectionError(format!("Failed to create pool: {e}")))?;

        // Test connection
        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Failed to connect: {e}")))?;

        // Verify MySQL version
        let version: String = conn
            .query_first("SELECT VERSION()")
            .map_err(|e| AdapterError::ConnectionError(format!("Failed to query version: {e}")))?
            .ok_or_else(|| AdapterError::ConnectionError("No version returned".to_string()))?;

        tracing::info!(version = %version, "Connected to MySQL");

        self.connection_string = Some(connection_string.to_string());
        self.pool = Some(pool);

        // Detect capabilities
        let mut facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;
        facts.features.insert("fulltext".to_string(), true);
        facts.features.insert("json".to_string(), true);
        facts.features.insert("window_functions".to_string(), true);

        Ok(())
    }

    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let query = "SELECT TABLE_NAME, TABLE_ROWS, DATA_LENGTH, INDEX_LENGTH, AVG_ROW_LENGTH, \
                     UPDATE_TIME \
                     FROM INFORMATION_SCHEMA.TABLES \
                     WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE'";

        let results: Vec<(String, u64, u64, u64, u64, Option<String>)> = conn
            .query_map(
                query,
                |(table_name, rows, data_len, index_len, avg_row, update_time): (
                    String,
                    u64,
                    u64,
                    u64,
                    u64,
                    Option<String>,
                )| {
                    (table_name, rows, data_len, index_len, avg_row, update_time)
                },
            )
            .map_err(|e| AdapterError::QueryError(format!("Failed to gather statistics: {e}")))?;

        let mut stats = HashMap::new();
        let mut facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;

        for (table_name, row_count, data_length, _index_length, avg_row_length, _update_time) in
            results
        {
            let page_count = if avg_row_length > 0 {
                data_length / (avg_row_length * 100)
            } else {
                0
            };

            let table_stats = TableStats {
                row_count,
                page_count,
                average_row_size: avg_row_length as f64,
                table_size_bytes: data_length,
                live_tuples: None,
                dead_tuples: None,
                last_analyzed: None,
            };

            facts
                .table_stats
                .insert(table_name.clone(), Self::to_core_table_stats(&table_stats));

            stats.insert(table_name, table_stats);
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

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        // Get column names first
        let col_query = format!(
            "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}'"
        );

        let columns: Vec<String> = conn
            .query_map(col_query, |col_name: String| col_name)
            .map_err(|e| AdapterError::QueryError(format!("Failed to get columns: {e}")))?;

        let mut stats = HashMap::new();
        let mut facts = self
            .facts
            .lock()
            .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;

        for column in columns {
            // Get distinct count and null fraction
            let stats_query = format!(
                "SELECT COUNT(DISTINCT `{column}`) as ndv, \
                 SUM(CASE WHEN `{column}` IS NULL THEN 1 ELSE 0 END) / COUNT(*) as null_frac, \
                 AVG(LENGTH(`{column}`)) as avg_width \
                 FROM `{table}`"
            );

            let result: Option<(u64, f64, Option<f64>)> =
                conn.query_first(stats_query).map_err(|e| {
                    AdapterError::QueryError(format!("Failed to gather column stats: {e}"))
                })?;

            if let Some((ndv, null_frac, avg_width)) = result {
                let col_stats = ColumnStats {
                    column_id: column.clone(),
                    ndv,
                    null_fraction: null_frac,
                    avg_width: avg_width.unwrap_or(0.0),
                    mcv: None,
                    histogram: None,
                    correlation: None,
                };

                facts.column_stats.insert(
                    (table.to_string(), column.clone()),
                    Self::to_core_column_stats(&col_stats),
                );

                stats.insert(column, col_stats);
            }
        }

        Ok(stats)
    }

    #[expect(clippy::too_many_lines, reason = "schema query assembly")]
    fn get_schema_info(&self) -> Result<SchemaInfo, AdapterError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| AdapterError::ConnectionError("Not connected".to_string()))?;

        let mut conn = pool
            .get_conn()
            .map_err(|e| AdapterError::ConnectionError(format!("Pool error: {e}")))?;

        let mut tables = HashMap::new();

        // Get all tables
        let table_query = "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES \
                          WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE'";

        let table_names: Vec<String> = conn
            .query_map(table_query, |table_name: String| table_name)
            .map_err(|e| AdapterError::QueryError(format!("Failed to query tables: {e}")))?;

        for table_name in table_names {
            // Get columns
            let col_query = format!(
                "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COLUMN_DEFAULT \
                 FROM INFORMATION_SCHEMA.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table_name}' \
                 ORDER BY ORDINAL_POSITION"
            );

            let columns: Vec<ColumnInfo> = conn
                .query_map(
                    col_query,
                    |(name, data_type, nullable, default): (
                        String,
                        String,
                        String,
                        Option<String>,
                    )| ColumnInfo {
                        name,
                        data_type,
                        nullable: nullable == "YES",
                        default_value: default,
                    },
                )
                .map_err(|e| AdapterError::QueryError(format!("Failed to query columns: {e}")))?;

            // Get primary key
            let pk_query = format!(
                "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table_name}' \
                 AND CONSTRAINT_NAME = 'PRIMARY' \
                 ORDER BY ORDINAL_POSITION"
            );

            let primary_key: Vec<String> =
                conn.query_map(pk_query, |col: String| col).map_err(|e| {
                    AdapterError::QueryError(format!("Failed to query primary key: {e}"))
                })?;

            // Get foreign keys
            let fk_query = format!(
                "SELECT CONSTRAINT_NAME, COLUMN_NAME, \
                 REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                 FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table_name}' \
                 AND REFERENCED_TABLE_NAME IS NOT NULL"
            );

            let foreign_keys: Vec<ForeignKeyInfo> = conn
                .query_map(
                    fk_query,
                    |(name, column, ref_table, ref_column): (String, String, String, String)| {
                        ForeignKeyInfo {
                            name,
                            columns: vec![column],
                            referenced_table: ref_table,
                            referenced_columns: vec![ref_column],
                        }
                    },
                )
                .map_err(|e| {
                    AdapterError::QueryError(format!("Failed to query foreign keys: {e}"))
                })?;

            // Get indexes
            let idx_query = format!(
                "SELECT INDEX_NAME, COLUMN_NAME, NON_UNIQUE, INDEX_TYPE \
                 FROM INFORMATION_SCHEMA.STATISTICS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table_name}' \
                 ORDER BY INDEX_NAME, SEQ_IN_INDEX"
            );

            let idx_rows: Vec<(String, String, i32, String)> = conn
                .query_map(
                    idx_query,
                    |(name, col, non_unique, idx_type): (String, String, i32, String)| {
                        (name, col, non_unique, idx_type)
                    },
                )
                .map_err(|e| AdapterError::QueryError(format!("Failed to query indexes: {e}")))?;

            // Group index columns
            let mut index_map: HashMap<String, (Vec<String>, bool, String)> = HashMap::new();
            for (idx_name, col_name, non_unique, idx_type) in idx_rows {
                let entry = index_map
                    .entry(idx_name)
                    .or_insert_with(|| (Vec::new(), non_unique == 0, idx_type));
                entry.0.push(col_name);
            }

            let indexes: Vec<IndexInfo> = index_map
                .into_iter()
                .map(|(name, (columns, unique, index_type))| IndexInfo {
                    name,
                    columns,
                    unique,
                    index_type,
                })
                .collect();

            // Build core schema facts
            let core_columns: Vec<(String, ra_core::DataType)> = columns
                .iter()
                .map(|c| (c.name.clone(), Self::mysql_to_core_type(&c.data_type)))
                .collect();

            let core_fks: Vec<ra_core::ForeignKey> = foreign_keys
                .iter()
                .map(|fk| ra_core::ForeignKey {
                    columns: fk.columns.clone(),
                    referenced_table: fk.referenced_table.clone(),
                    referenced_columns: fk.referenced_columns.clone(),
                })
                .collect();

            let core_indexes: Vec<ra_core::facts::IndexInfo> = indexes
                .iter()
                .map(|idx| ra_core::facts::IndexInfo {
                    name: idx.name.clone(),
                    index_type: if idx.index_type.to_uppercase() == "FULLTEXT" {
                        // Note: FullText doesn't exist in IndexType, using Gin (similar use case)
                        ra_core::facts::IndexType::Gin
                    } else {
                        ra_core::facts::IndexType::BTree
                    },
                    columns: idx.columns.clone(),
                    included_columns: vec![],
                    is_unique: idx.unique,
                })
                .collect();

            let mut facts = self
                .facts
                .lock()
                .map_err(|e| AdapterError::ConnectionError(format!("Mutex poisoned: {e}")))?;
            facts.schemas.insert(
                table_name.clone(),
                ra_core::facts::TableInfo {
                    name: table_name.clone(),
                    columns: core_columns,
                    primary_key: primary_key.clone(),
                    foreign_keys: core_fks,
                    indexes: core_indexes,
                    storage_format: ra_core::facts::StorageFormat::RowBased,
                },
            );

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
            database_name: "MySQL".to_string(),
            dialect: SqlDialect::Mysql,
            features: facts.features.clone(),
            index_types: vec![
                "BTREE".to_string(),
                "HASH".to_string(),
                "FULLTEXT".to_string(),
            ],
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
        SqlDialect::Mysql
    }

    fn database_name(&self) -> &str {
        "MySQL"
    }

    fn as_facts_provider(&self) -> &dyn FactsProvider {
        self
    }
}

#[cfg(not(feature = "mysql"))]
impl DatabaseAdapter for MySQLAdapter {
    fn connect(&mut self, connection_string: &str) -> Result<(), AdapterError> {
        self.connection_string = Some(connection_string.to_string());
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled. Recompile with --features mysql".to_string(),
        ))
    }

    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    fn gather_column_stats(
        &self,
        _table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    fn get_schema_info(&self) -> Result<SchemaInfo, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    fn get_capabilities(&self) -> Result<DatabaseCapabilities, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    fn supports_feature(&self, _feature: &str) -> Result<bool, AdapterError> {
        Err(AdapterError::UnsupportedFeature(
            "MySQL feature not enabled".to_string(),
        ))
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Mysql
    }

    fn database_name(&self) -> &str {
        "MySQL"
    }

    fn as_facts_provider(&self) -> &dyn FactsProvider {
        self
    }
}

impl FactsProvider for MySQLAdapter {
    fn get_table_stats(&self, _table: &str) -> Option<&ra_core::CoreTableStats> {
        // Cannot return reference from Mutex guard - use gather_statistics instead
        None
    }

    fn get_column_stats(&self, _table: &str, _column: &str) -> Option<&ra_core::ColumnStats> {
        // Cannot return reference from Mutex guard - use gather_column_stats instead
        None
    }

    fn hardware_profile(&self) -> &ra_core::CoreHardwareProfile {
        // Return static default
        &DEFAULT_HARDWARE
    }

    fn get_schema(&self, _table: &str) -> Option<&ra_core::facts::TableInfo> {
        // Cannot return reference from Mutex guard - use get_schema_info instead
        None
    }

    fn runtime_stats(&self, _operator_id: &str) -> Option<&ra_core::facts::OperatorStats> {
        None
    }

    fn database_name(&self) -> &'static str {
        "MySQL"
    }

    fn supports_feature(&self, feature: &str) -> bool {
        let Ok(facts) = self.facts.lock() else {
            return false;
        };
        facts.features.get(feature).copied().unwrap_or(false)
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Mysql
    }

    fn memory_limit(&self) -> Option<u64> {
        None
    }

    fn optimizer_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }
}

static DEFAULT_HARDWARE: ra_core::CoreHardwareProfile = ra_core::CoreHardwareProfile {
    cpu_cores: 8,
    available_memory: 16 * 1024 * 1024 * 1024,
    total_memory: 16 * 1024 * 1024 * 1024,
    simd_width: 256,
    has_gpu: false,
    gpu_memory: None,
    l1_cache_size: 32 * 1024,
    l2_cache_size: 256 * 1024,
    l3_cache_size: 8 * 1024 * 1024,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_adapter() {
        let adapter = MySQLAdapter::new();
        assert_eq!(DatabaseAdapter::database_name(&adapter), "MySQL");
        assert_eq!(DatabaseAdapter::sql_dialect(&adapter), SqlDialect::Mysql);
    }

    #[test]
    fn default_adapter() {
        let adapter = MySQLAdapter::default();
        assert!(adapter.connection_string.is_none());
    }

    #[test]
    fn mysql_to_core_type_mapping() {
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("INT"),
            ra_core::DataType::Integer
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("BIGINT"),
            ra_core::DataType::Integer
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("DOUBLE"),
            ra_core::DataType::Float
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("VARCHAR"),
            ra_core::DataType::String
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("TEXT"),
            ra_core::DataType::String
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("BOOLEAN"),
            ra_core::DataType::Boolean
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("TIMESTAMP"),
            ra_core::DataType::Timestamp
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("BLOB"),
            ra_core::DataType::Binary
        );
        assert_eq!(
            MySQLAdapter::mysql_to_core_type("JSON"),
            ra_core::DataType::Json
        );
    }

    #[test]
    fn base64_encoding() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }
}
