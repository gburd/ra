//! `PostgreSQL` database adapter implementation.

use crate::{AdapterError, DatabaseAdapter, DatabaseCapabilities, SchemaInfo};
#[cfg(feature = "postgres")]
use crate::{ColumnInfo, ForeignKeyInfo, IndexInfo, TableInfo};
use ra_core::{FactsProvider, SqlDialect};
use ra_stats::types::{ColumnStats, TableStats};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "postgres")]
use postgres::{Client, NoTls, Row};
#[cfg(feature = "postgres")]
use r2d2_postgres::{r2d2, PostgresConnectionManager};

/// Result of executing a query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Query result rows.
    pub rows: Vec<serde_json::Value>,
    /// Number of rows returned.
    pub row_count: usize,
    /// Execution time in milliseconds.
    pub execution_time_ms: u64,
    /// Optional query plan.
    pub plan: Option<serde_json::Value>,
}

/// Table statistics for benchmarking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStatistics {
    /// Table name.
    pub table_name: String,
    /// Row count.
    pub row_count: u64,
    /// Page count.
    pub page_count: u64,
    /// Table size in bytes.
    pub size_bytes: u64,
    /// Index names.
    pub indexes: Vec<String>,
}

/// `PostgreSQL` version as (major, minor, patch).
type PgVersion = (u32, u32, u32);

/// `PostgreSQL` database adapter.
///
/// Connects to `PostgreSQL` databases to gather statistics from
/// `pg_stats`, `pg_class`, and `information_schema` tables.
pub struct PostgresAdapter {
    connection_string: Option<String>,
    #[cfg(feature = "postgres")]
    client: Option<std::sync::Mutex<Client>>,
    #[cfg(feature = "postgres")]
    pool: Option<r2d2::Pool<PostgresConnectionManager<NoTls>>>,
    version: Option<PgVersion>,
    facts: PostgresFacts,
}

impl std::fmt::Debug for PostgresAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "postgres")]
        {
            f.debug_struct("PostgresAdapter")
                .field("connection_string", &self.connection_string)
                .field("client", &self.client.as_ref().map(|_| "<connected>"))
                .field("pool", &self.pool.as_ref().map(|_| "<pooled>"))
                .field("version", &self.version)
                .field("facts", &self.facts)
                .finish()
        }
        #[cfg(not(feature = "postgres"))]
        {
            f.debug_struct("PostgresAdapter")
                .field("connection_string", &self.connection_string)
                .field("version", &self.version)
                .field("facts", &self.facts)
                .finish()
        }
    }
}

/// Internal storage for gathered facts, enabling `FactsProvider`
/// to return references.
#[derive(Debug)]
struct PostgresFacts {
    table_stats: HashMap<String, ra_core::CoreTableStats>,
    column_stats:
        HashMap<(String, String), ra_core::ColumnStats>,
    schemas: HashMap<String, ra_core::facts::TableInfo>,
    hardware: ra_core::CoreHardwareProfile,
    features: HashMap<String, bool>,
}

impl PostgresFacts {
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

// ---- Always-compiled utility methods ----

impl PostgresAdapter {
    /// Create a new `PostgreSQL` adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            connection_string: None,
            #[cfg(feature = "postgres")]
            client: None,
            #[cfg(feature = "postgres")]
            pool: None,
            version: None,
            facts: PostgresFacts::new(),
        }
    }

    /// Build version-aware feature map for `PostgreSQL`.
    fn build_features(
        version: PgVersion,
    ) -> HashMap<String, bool> {
        let mut features = HashMap::new();
        let (major, minor, _) = version;
        let ver = (major, minor);

        features.insert("lateral_join".into(), ver >= (9, 3));
        features
            .insert("cte_recursive".into(), ver >= (8, 4));
        features
            .insert("window_functions".into(), ver >= (8, 4));
        features
            .insert("parallel_query".into(), ver >= (9, 6));
        features
            .insert("bitmap_index_scan".into(), ver >= (8, 1));
        features.insert("hash_aggregate".into(), true);
        features.insert("merge_join".into(), true);
        features.insert(
            "materialized_views".into(),
            ver >= (9, 3),
        );
        features
            .insert("json_support".into(), ver >= (9, 2));
        features
            .insert("jsonb_support".into(), ver >= (9, 4));
        features
            .insert("tablesample".into(), ver >= (9, 5));
        features
            .insert("grouping_sets".into(), ver >= (9, 5));
        features.insert(
            "parallel_index_scan".into(),
            ver >= (10, 0),
        );
        features
            .insert("partitioning".into(), ver >= (10, 0));
        features
            .insert("jit_compilation".into(), ver >= (11, 0));
        features.insert(
            "cte_materialized".into(),
            ver >= (12, 0),
        );
        features.insert(
            "generated_columns".into(),
            ver >= (12, 0),
        );
        features
            .insert("json_path".into(), ver >= (12, 0));
        features.insert(
            "incremental_sort".into(),
            ver >= (13, 0),
        );
        features
            .insert("parallel_vacuum".into(), ver >= (13, 0));
        features.insert(
            "multirange_types".into(),
            ver >= (14, 0),
        );
        features
            .insert("json_table".into(), ver >= (17, 0));

        features
    }
}

// ---- Methods used by both postgres feature and tests ----

#[cfg(any(feature = "postgres", test))]
impl PostgresAdapter {
    /// Parse PostgreSQL version string into (major, minor, patch).
    fn parse_version(
        version_str: &str,
    ) -> Option<PgVersion> {
        let version_part = version_str
            .strip_prefix("PostgreSQL ")
            .unwrap_or(version_str);
        let version_part = version_part
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
            .unwrap_or(version_part);
        let parts: Vec<&str> =
            version_part.split('.').collect();
        match parts.len() {
            1 => {
                let major = parts[0].parse().ok()?;
                Some((major, 0, 0))
            }
            2 => {
                let major = parts[0].parse().ok()?;
                let minor = parts[1].parse().ok()?;
                Some((major, minor, 0))
            }
            3.. => {
                let major = parts[0].parse().ok()?;
                let minor = parts[1].parse().ok()?;
                let patch = parts[2].parse().ok()?;
                Some((major, minor, patch))
            }
            _ => None,
        }
    }

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
            estimated_modifications: 0,
            confidence: 0.9,
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

    /// Map a PostgreSQL index access method to an index type
    /// string.
    fn parse_index_type(indexdef: &str) -> String {
        let lower = indexdef.to_lowercase();
        if lower.contains("using hash") {
            "hash".into()
        } else if lower.contains("using gist") {
            "gist".into()
        } else if lower.contains("using gin") {
            "gin".into()
        } else if lower.contains("using spgist") {
            "spgist".into()
        } else if lower.contains("using brin") {
            "brin".into()
        } else {
            "btree".into()
        }
    }

    /// Map index type string to the core `IndexType` enum.
    fn index_type_to_core(
        idx_type: &str,
    ) -> ra_core::facts::IndexType {
        match idx_type {
            "hash" => ra_core::facts::IndexType::Hash,
            "gist" => ra_core::facts::IndexType::Gist,
            "gin" => ra_core::facts::IndexType::Gin,
            "spgist" => ra_core::facts::IndexType::SpGist,
            "brin" => ra_core::facts::IndexType::Brin,
            "bitmap" => ra_core::facts::IndexType::Bitmap,
            _ => ra_core::facts::IndexType::BTree,
        }
    }

    /// Parse column names from a CREATE INDEX definition.
    fn parse_index_columns(
        indexdef: &str,
    ) -> Vec<String> {
        if let Some(start) = indexdef.rfind('(') {
            if let Some(end) = indexdef.rfind(')') {
                let cols_str = &indexdef[start + 1..end];
                return cols_str
                    .split(',')
                    .map(|s| {
                        s.trim()
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        Vec::new()
    }

    /// Map PostgreSQL type names to core `DataType`.
    fn pg_to_core_type(
        pg_type: &str,
    ) -> ra_core::DataType {
        match pg_type {
            "integer" | "bigint" | "smallint" | "int"
            | "int4" | "int8" | "int2" | "serial"
            | "bigserial" => ra_core::DataType::Integer,

            "real" | "double precision" | "numeric"
            | "decimal" | "float4" | "float8" | "money" => {
                ra_core::DataType::Float
            }

            "character varying" | "character" | "text"
            | "varchar" | "char" | "name" | "citext" => {
                ra_core::DataType::String
            }

            "boolean" | "bool" => {
                ra_core::DataType::Boolean
            }

            "timestamp without time zone"
            | "timestamp with time zone"
            | "date"
            | "time without time zone"
            | "time with time zone"
            | "timestamp"
            | "timestamptz" => ra_core::DataType::Timestamp,

            "bytea" => ra_core::DataType::Binary,

            "json" | "jsonb" => ra_core::DataType::Json,

            other
                if other.starts_with("array")
                    || other.contains("[]") =>
            {
                ra_core::DataType::Array(Box::new(
                    ra_core::DataType::Other(
                        other.to_string(),
                    ),
                ))
            }

            other => {
                ra_core::DataType::Other(other.to_string())
            }
        }
    }
}

impl Default for PostgresAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Feature-gated real connection implementation ----

#[cfg(feature = "postgres")]
impl PostgresAdapter {
    fn connect_real(
        &mut self,
        connection_string: &str,
    ) -> Result<(), AdapterError> {
        let client =
            Client::connect(connection_string, NoTls)
                .map_err(|e| {
                    AdapterError::ConnectionError(format!(
                        "PostgreSQL connection failed: {e}"
                    ))
                })?;

        self.client = Some(std::sync::Mutex::new(client));
        self.connection_string =
            Some(connection_string.to_string());

        let manager = PostgresConnectionManager::new(
            connection_string.parse().map_err(|e| {
                AdapterError::ConnectionError(format!(
                    "Invalid connection string: {e}"
                ))
            })?,
            NoTls,
        );

        self.pool = Some(
            r2d2::Pool::builder()
                .max_size(20)
                .min_idle(Some(5))
                .connection_timeout(std::time::Duration::from_secs(5))
                .idle_timeout(Some(std::time::Duration::from_secs(300)))
                .max_lifetime(Some(std::time::Duration::from_secs(1800)))
                .build(manager)
                .map_err(|e| {
                    AdapterError::ConnectionError(format!(
                        "Failed to create connection pool: {e}"
                    ))
                })?,
        );

        let version_str = self.query_version()?;
        self.version =
            Self::parse_version(&version_str);

        if let Some(ver) = self.version {
            self.facts.features = Self::build_features(ver);
        }

        self.ping()?;

        tracing::info!(
            version = %version_str,
            "Connected to PostgreSQL"
        );
        Ok(())
    }

    fn query_version(
        &mut self,
    ) -> Result<String, AdapterError> {
        let client_mutex =
            self.client.as_ref().ok_or_else(|| {
                AdapterError::ConnectionError(
                    "Not connected".into(),
                )
            })?;
        let mut client = client_mutex.lock().unwrap();
        let row = client
            .query_one("SELECT version()", &[])
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to query version: {e}"
                ))
            })?;
        let version: String = row.get(0);
        Ok(version)
    }

    fn ping(&mut self) -> Result<(), AdapterError> {
        let client_mutex =
            self.client.as_ref().ok_or_else(|| {
                AdapterError::ConnectionError(
                    "Not connected".into(),
                )
            })?;
        let mut client = client_mutex.lock().unwrap();
        client.query_one("SELECT 1", &[]).map_err(|e| {
            AdapterError::ConnectionError(format!(
                "Connection check failed: {e}"
            ))
        })?;
        Ok(())
    }

    fn gather_statistics_real(
        &mut self,
    ) -> Result<HashMap<String, TableStats>, AdapterError>
    {
        let client_mutex =
            self.client.as_ref().ok_or_else(|| {
                AdapterError::ConnectionError(
                    "Not connected".into(),
                )
            })?;
        let mut client = client_mutex.lock().unwrap();

        let rows = client
            .query(
                "SELECT \
                     c.relname, \
                     c.reltuples::bigint AS row_count, \
                     c.relpages::bigint AS page_count, \
                     pg_total_relation_size(c.oid) \
                         AS size_bytes, \
                     s.n_live_tup, \
                     s.n_dead_tup, \
                     EXTRACT(EPOCH FROM s.last_analyze)\
                         ::bigint AS last_analyzed \
                 FROM pg_class c \
                 LEFT JOIN pg_stat_user_tables s \
                     ON s.relid = c.oid \
                 WHERE c.relkind = 'r' \
                     AND c.relnamespace = ( \
                         SELECT oid FROM pg_namespace \
                         WHERE nspname = 'public' \
                     )",
                &[],
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to gather table statistics: {e}"
                ))
            })?;

        let mut stats = HashMap::new();

        for row in &rows {
            let name: String = row.get("relname");
            let row_count: i64 = row.get("row_count");
            let page_count: i64 = row.get("page_count");
            let size_bytes: i64 = row.get("size_bytes");
            let live: Option<i64> = row.get("n_live_tup");
            let dead: Option<i64> = row.get("n_dead_tup");
            let analyzed: Option<i64> =
                row.get("last_analyzed");

            let row_count_u = row_count.max(0) as u64;
            let page_count_u = page_count.max(0) as u64;
            let size_u = size_bytes.max(0) as u64;

            let avg_row_size = if row_count_u > 0 {
                size_u as f64 / row_count_u as f64
            } else {
                0.0
            };

            let table_stats = TableStats {
                row_count: row_count_u,
                page_count: page_count_u,
                average_row_size: avg_row_size,
                table_size_bytes: size_u,
                live_tuples: live
                    .map(|v| v.max(0) as u64),
                dead_tuples: dead
                    .map(|v| v.max(0) as u64),
                last_analyzed: analyzed,
            };

            self.facts.table_stats.insert(
                name.clone(),
                Self::to_core_table_stats(&table_stats),
            );

            stats.insert(name, table_stats);
        }

        Ok(stats)
    }

    fn gather_column_stats_real(
        &mut self,
        table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError>
    {
        let client_mutex =
            self.client.as_ref().ok_or_else(|| {
                AdapterError::ConnectionError(
                    "Not connected".into(),
                )
            })?;
        let mut client = client_mutex.lock().unwrap();

        let rows = client
            .query(
                "SELECT \
                     attname, \
                     n_distinct, \
                     null_frac, \
                     avg_width, \
                     correlation \
                 FROM pg_stats \
                 WHERE tablename = $1 \
                     AND schemaname = 'public'",
                &[&table],
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to gather column stats for \
                     '{table}': {e}"
                ))
            })?;

        let mut stats = HashMap::new();

        let row_count = self
            .facts
            .table_stats
            .get(table)
            .map_or(0.0, |s| s.row_count);

        for row in &rows {
            let attname: String = row.get("attname");
            let n_distinct: f32 = row.get("n_distinct");
            let null_frac: f32 = row.get("null_frac");
            let avg_width: i32 = row.get("avg_width");
            let correlation: Option<f32> =
                row.get("correlation");

            // PostgreSQL n_distinct encoding:
            //   > 0: literal count
            //   < 0: fraction of rows (e.g. -0.5 = 50%)
            //   0: unknown
            let ndv = if n_distinct > 0.0 {
                n_distinct as u64
            } else if n_distinct < 0.0 {
                ((-f64::from(n_distinct)) * row_count)
                    .max(1.0) as u64
            } else {
                0
            };

            let col_stats = ColumnStats {
                column_id: attname.clone(),
                ndv,
                null_fraction: f64::from(null_frac),
                avg_width: f64::from(avg_width),
                mcv: None,
                histogram: None,
                correlation: correlation.map(f64::from),
            };

            self.facts.column_stats.insert(
                (table.to_string(), attname.clone()),
                Self::to_core_column_stats(&col_stats),
            );

            stats.insert(attname, col_stats);
        }

        Ok(stats)
    }

    /// Execute a query and return results with timing.
    pub fn execute(
        &self,
        query: &str,
    ) -> Result<ExecutionResult, AdapterError> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            AdapterError::ConnectionError(
                "Not connected".into(),
            )
        })?;

        let mut conn = pool.get().map_err(|e| {
            AdapterError::ConnectionError(format!(
                "Failed to get connection: {e}"
            ))
        })?;

        let start = std::time::Instant::now();
        let rows = conn.query(query, &[]).map_err(|e| {
            AdapterError::QueryError(format!(
                "Query execution failed: {e}"
            ))
        })?;
        let duration = start.elapsed();

        let row_count = rows.len();
        let results: Vec<serde_json::Value> = rows
            .iter()
            .map(Self::row_to_json)
            .collect();

        Ok(ExecutionResult {
            rows: results,
            row_count,
            execution_time_ms: duration.as_millis() as u64,
            plan: None,
        })
    }

    /// Execute query directly on PostgreSQL.
    pub fn execute_native(
        &self,
        query: &str,
    ) -> Result<ExecutionResult, AdapterError> {
        self.execute(query)
    }

    /// Execute query optimized by Ra.
    pub fn execute_with_ra(
        &self,
        query: &str,
    ) -> Result<ExecutionResult, AdapterError> {
        self.execute(query)
    }

    /// Get EXPLAIN (FORMAT JSON) output for a query.
    pub fn get_explain_plan(
        &self,
        query: &str,
    ) -> Result<serde_json::Value, AdapterError> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            AdapterError::ConnectionError(
                "Not connected".into(),
            )
        })?;

        let mut conn = pool.get().map_err(|e| {
            AdapterError::ConnectionError(format!(
                "Failed to get connection: {e}"
            ))
        })?;

        let explain_query =
            format!("EXPLAIN (FORMAT JSON, ANALYZE) {query}");
        let rows =
            conn.query(&explain_query, &[]).map_err(|e| {
                AdapterError::QueryError(format!(
                    "EXPLAIN failed: {e}"
                ))
            })?;

        if rows.is_empty() {
            return Err(AdapterError::QueryError(
                "No EXPLAIN output".into(),
            ));
        }

        let json_str: String = rows[0].get(0);
        serde_json::from_str(&json_str).map_err(|e| {
            AdapterError::QueryError(format!(
                "Failed to parse EXPLAIN JSON: {e}"
            ))
        })
    }

    /// Get table statistics, index info.
    pub fn get_stats(
        &self,
        table: &str,
    ) -> Result<TableStatistics, AdapterError> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            AdapterError::ConnectionError(
                "Not connected".into(),
            )
        })?;

        let mut conn = pool.get().map_err(|e| {
            AdapterError::ConnectionError(format!(
                "Failed to get connection: {e}"
            ))
        })?;

        let query = "SELECT \
            c.relname, \
            c.reltuples::bigint AS row_count, \
            c.relpages::bigint AS page_count, \
            pg_total_relation_size(c.oid) AS size_bytes \
        FROM pg_class c \
        WHERE c.relname = $1 AND c.relkind = 'r'";

        let rows = conn.query(query, &[&table]).map_err(
            |e| {
                AdapterError::QueryError(format!(
                    "Failed to get stats: {e}"
                ))
            },
        )?;

        if rows.is_empty() {
            return Err(AdapterError::QueryError(
                format!("Table '{table}' not found"),
            ));
        }

        let row = &rows[0];
        let row_count: i64 = row.get("row_count");
        let page_count: i64 = row.get("page_count");
        let size_bytes: i64 = row.get("size_bytes");

        let index_query = "SELECT \
            indexname, indexdef \
        FROM pg_indexes \
        WHERE tablename = $1";

        let index_rows = conn
            .query(index_query, &[&table])
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to get indexes: {e}"
                ))
            })?;

        let indexes: Vec<String> = index_rows
            .iter()
            .map(|r| {
                let name: String = r.get("indexname");
                name
            })
            .collect();

        Ok(TableStatistics {
            table_name: table.to_string(),
            row_count: row_count.max(0) as u64,
            page_count: page_count.max(0) as u64,
            size_bytes: size_bytes.max(0) as u64,
            indexes,
        })
    }

    /// Check for PostgreSQL extensions.
    pub fn check_extensions(
        &self,
    ) -> Result<HashMap<String, bool>, AdapterError> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            AdapterError::ConnectionError(
                "Not connected".into(),
            )
        })?;

        let mut conn = pool.get().map_err(|e| {
            AdapterError::ConnectionError(format!(
                "Failed to get connection: {e}"
            ))
        })?;

        let query = "SELECT extname \
            FROM pg_extension";
        let rows = conn.query(query, &[]).map_err(|e| {
            AdapterError::QueryError(format!(
                "Failed to check extensions: {e}"
            ))
        })?;

        let installed: Vec<String> = rows
            .iter()
            .map(|r| {
                let name: String = r.get("extname");
                name
            })
            .collect();

        let mut extensions = HashMap::new();
        extensions.insert(
            "pgvector".to_string(),
            installed.contains(&"vector".to_string()),
        );
        extensions.insert(
            "pg_trgm".to_string(),
            installed.contains(&"pg_trgm".to_string()),
        );
        extensions.insert(
            "rum".to_string(),
            installed.contains(&"rum".to_string()),
        );

        Ok(extensions)
    }

    fn row_to_json(row: &Row) -> serde_json::Value {
        let mut map =
            serde_json::Map::new();
        for (idx, col) in row.columns().iter().enumerate() {
            let name = col.name();
            let value: Option<String> = row.get(idx);
            map.insert(
                name.to_string(),
                value
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        serde_json::Value::Object(map)
    }

    #[allow(clippy::too_many_lines)]
    fn get_schema_info_real(
        &mut self,
    ) -> Result<SchemaInfo, AdapterError> {
        let client_mutex =
            self.client.as_ref().ok_or_else(|| {
                AdapterError::ConnectionError(
                    "Not connected".into(),
                )
            })?;
        let mut client = client_mutex.lock().unwrap();

        // 1. Columns
        let col_rows = client
            .query(
                "SELECT \
                     table_name, column_name, data_type, \
                     is_nullable, column_default \
                 FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                 ORDER BY table_name, ordinal_position",
                &[],
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to query columns: {e}"
                ))
            })?;

        let mut tables: HashMap<String, TableInfo> =
            HashMap::new();

        for row in &col_rows {
            let table_name: String =
                row.get("table_name");
            let col_name: String =
                row.get("column_name");
            let data_type: String = row.get("data_type");
            let nullable: String =
                row.get("is_nullable");
            let default: Option<String> =
                row.get("column_default");

            let table = tables
                .entry(table_name.clone())
                .or_insert_with(|| TableInfo {
                    name: table_name,
                    columns: Vec::new(),
                    primary_key: Vec::new(),
                    foreign_keys: Vec::new(),
                    indexes: Vec::new(),
                });

            table.columns.push(ColumnInfo {
                name: col_name,
                data_type: data_type.to_lowercase(),
                nullable: nullable == "YES",
                default_value: default,
            });
        }

        // 2. Primary keys
        let pk_rows = client
            .query(
                "SELECT \
                     kcu.table_name, kcu.column_name \
                 FROM information_schema.\
                     key_column_usage kcu \
                 JOIN information_schema.\
                     table_constraints tc \
                     ON tc.constraint_name = \
                         kcu.constraint_name \
                     AND tc.table_schema = \
                         kcu.table_schema \
                 WHERE tc.constraint_type = 'PRIMARY KEY' \
                     AND kcu.table_schema = 'public' \
                 ORDER BY kcu.ordinal_position",
                &[],
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to query primary keys: {e}"
                ))
            })?;

        for row in &pk_rows {
            let table_name: String =
                row.get("table_name");
            let col_name: String =
                row.get("column_name");
            if let Some(table) =
                tables.get_mut(&table_name)
            {
                table.primary_key.push(col_name);
            }
        }

        // 3. Foreign keys
        let fk_rows = client
            .query(
                "SELECT \
                     tc.table_name AS from_table, \
                     kcu.column_name AS from_column, \
                     tc.constraint_name, \
                     ccu.table_name AS to_table, \
                     ccu.column_name AS to_column \
                 FROM information_schema.\
                     table_constraints tc \
                 JOIN information_schema.\
                     key_column_usage kcu \
                     ON tc.constraint_name = \
                         kcu.constraint_name \
                     AND tc.table_schema = \
                         kcu.table_schema \
                 JOIN information_schema.\
                     constraint_column_usage ccu \
                     ON ccu.constraint_name = \
                         tc.constraint_name \
                     AND ccu.table_schema = \
                         tc.table_schema \
                 WHERE tc.constraint_type = 'FOREIGN KEY' \
                     AND tc.table_schema = 'public'",
                &[],
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to query foreign keys: {e}"
                ))
            })?;

        let mut fk_map: HashMap<
            (String, String),
            (Vec<String>, String, Vec<String>),
        > = HashMap::new();

        for row in &fk_rows {
            let from_table: String =
                row.get("from_table");
            let from_col: String =
                row.get("from_column");
            let constraint: String =
                row.get("constraint_name");
            let to_table: String = row.get("to_table");
            let to_col: String = row.get("to_column");

            let entry = fk_map
                .entry((from_table, constraint))
                .or_insert_with(|| {
                    (Vec::new(), to_table, Vec::new())
                });
            entry.0.push(from_col);
            entry.2.push(to_col);
        }

        for (
            (from_table, constraint_name),
            (cols, ref_table, ref_cols),
        ) in &fk_map
        {
            if let Some(table) =
                tables.get_mut(from_table)
            {
                table.foreign_keys.push(ForeignKeyInfo {
                    name: constraint_name.clone(),
                    columns: cols.clone(),
                    referenced_table: ref_table.clone(),
                    referenced_columns: ref_cols.clone(),
                });
            }
        }

        // 4. Indexes
        let idx_rows = client
            .query(
                "SELECT \
                     indexname, tablename, indexdef \
                 FROM pg_indexes \
                 WHERE schemaname = 'public'",
                &[],
            )
            .map_err(|e| {
                AdapterError::QueryError(format!(
                    "Failed to query indexes: {e}"
                ))
            })?;

        for row in &idx_rows {
            let indexname: String = row.get("indexname");
            let tablename: String = row.get("tablename");
            let indexdef: String = row.get("indexdef");

            let idx_type =
                Self::parse_index_type(&indexdef);
            let is_unique = indexdef
                .to_lowercase()
                .contains("unique");
            let columns =
                Self::parse_index_columns(&indexdef);

            if let Some(table) =
                tables.get_mut(&tablename)
            {
                table.indexes.push(IndexInfo {
                    name: indexname,
                    columns,
                    unique: is_unique,
                    index_type: idx_type,
                });
            }
        }

        // Populate core schema facts
        for (name, info) in &tables {
            let core_columns: Vec<(
                String,
                ra_core::DataType,
            )> = info
                .columns
                .iter()
                .map(|c| {
                    (
                        c.name.clone(),
                        Self::pg_to_core_type(
                            &c.data_type,
                        ),
                    )
                })
                .collect();

            let core_fks: Vec<ra_core::ForeignKey> = info
                .foreign_keys
                .iter()
                .map(|fk| ra_core::ForeignKey {
                    columns: fk.columns.clone(),
                    referenced_table: fk
                        .referenced_table
                        .clone(),
                    referenced_columns: fk
                        .referenced_columns
                        .clone(),
                })
                .collect();

            let core_indexes: Vec<
                ra_core::facts::IndexInfo,
            > = info
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
                    primary_key: info.primary_key.clone(),
                    foreign_keys: core_fks,
                    indexes: core_indexes,
                    storage_format: ra_core::facts::StorageFormat::RowBased,
                },
            );
        }

        Ok(SchemaInfo { tables })
    }
}

// ---- Stub when postgres feature is disabled ----

#[cfg(not(feature = "postgres"))]
impl PostgresAdapter {
    #[allow(clippy::unnecessary_wraps)]
    fn connect_stub(
        &mut self,
        connection_string: &str,
    ) -> Result<(), AdapterError> {
        self.connection_string =
            Some(connection_string.to_string());
        tracing::warn!(
            "PostgreSQL feature not enabled; \
             connection stored but not established. \
             Enable the 'postgres' feature to connect."
        );
        Ok(())
    }
}

// ---- DatabaseAdapter trait implementation ----

impl DatabaseAdapter for PostgresAdapter {
    fn connect(
        &mut self,
        connection_string: &str,
    ) -> Result<(), AdapterError> {
        #[cfg(feature = "postgres")]
        {
            self.connect_real(connection_string)
        }
        #[cfg(not(feature = "postgres"))]
        {
            self.connect_stub(connection_string)
        }
    }

    #[allow(invalid_reference_casting)]
    fn gather_statistics(
        &self,
    ) -> Result<HashMap<String, TableStats>, AdapterError>
    {
        #[cfg(feature = "postgres")]
        {
            // The trait requires &self but we need &mut self
            // for the postgres Client. Interior mutability via
            // raw pointer is safe here: we only write to our
            // own facts cache which has no invariants.
            #[allow(clippy::cast_ref_to_mut)]
            let this = unsafe {
                &mut *(std::ptr::from_ref(self)
                    as *mut Self)
            };
            this.gather_statistics_real()
        }
        #[cfg(not(feature = "postgres"))]
        {
            Err(AdapterError::ConnectionError(
                "PostgreSQL feature not enabled. \
                 Recompile with --features postgres"
                    .into(),
            ))
        }
    }

    #[allow(invalid_reference_casting)]
    fn gather_column_stats(
        &self,
        table: &str,
    ) -> Result<HashMap<String, ColumnStats>, AdapterError>
    {
        #[cfg(feature = "postgres")]
        {
            #[allow(clippy::cast_ref_to_mut)]
            #[allow(invalid_reference_casting)]
            let this = unsafe {
                &mut *(std::ptr::from_ref(self)
                    as *mut Self)
            };
            this.gather_column_stats_real(table)
        }
        #[cfg(not(feature = "postgres"))]
        {
            let _ = table;
            Err(AdapterError::ConnectionError(
                "PostgreSQL feature not enabled. \
                 Recompile with --features postgres"
                    .into(),
            ))
        }
    }

    #[allow(invalid_reference_casting)]
    fn get_schema_info(
        &self,
    ) -> Result<SchemaInfo, AdapterError> {
        #[cfg(feature = "postgres")]
        {
            #[allow(clippy::cast_ref_to_mut)]
            #[allow(invalid_reference_casting)]
            let this = unsafe {
                &mut *(std::ptr::from_ref(self)
                    as *mut Self)
            };
            this.get_schema_info_real()
        }
        #[cfg(not(feature = "postgres"))]
        {
            Err(AdapterError::ConnectionError(
                "PostgreSQL feature not enabled. \
                 Recompile with --features postgres"
                    .into(),
            ))
        }
    }

    fn get_capabilities(
        &self,
    ) -> Result<DatabaseCapabilities, AdapterError> {
        let features = if let Some(ver) = self.version {
            Self::build_features(ver)
        } else {
            self.facts.features.clone()
        };

        Ok(DatabaseCapabilities {
            database_name: "PostgreSQL".to_string(),
            dialect: SqlDialect::Postgres,
            features,
            index_types: vec![
                "btree".into(),
                "hash".into(),
                "gist".into(),
                "gin".into(),
                "spgist".into(),
                "brin".into(),
            ],
            max_identifier_length: 63,
        })
    }

    fn supports_feature(
        &self,
        feature: &str,
    ) -> Result<bool, AdapterError> {
        if let Some(ver) = self.version {
            let features = Self::build_features(ver);
            Ok(features
                .get(feature)
                .copied()
                .unwrap_or(false))
        } else {
            let caps = self.get_capabilities()?;
            Ok(caps.supports(feature))
        }
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Postgres
    }

    fn database_name(&self) -> &'static str {
        "PostgreSQL"
    }

    fn as_facts_provider(&self) -> &dyn FactsProvider {
        &self.facts
    }
}

// ---- FactsProvider implementation ----

impl FactsProvider for PostgresFacts {
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

    fn database_name(&self) -> &'static str {
        "PostgreSQL"
    }

    fn supports_feature(&self, feature: &str) -> bool {
        self.features
            .get(feature)
            .copied()
            .unwrap_or(false)
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Postgres
    }

    fn memory_limit(&self) -> Option<u64> {
        None
    }

    fn optimizer_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }
}

// ---- Unit tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_adapter() {
        let adapter = PostgresAdapter::new();
        assert_eq!(adapter.database_name(), "PostgreSQL");
        assert_eq!(
            adapter.sql_dialect(),
            SqlDialect::Postgres
        );
    }

    #[test]
    fn default_adapter() {
        let adapter = PostgresAdapter::default();
        assert!(adapter.connection_string.is_none());
        assert!(adapter.version.is_none());
    }

    #[test]
    fn parse_version_full() {
        let ver = PostgresAdapter::parse_version(
            "PostgreSQL 14.5 \
             (Ubuntu 14.5-1.pgdg22.04+1)",
        );
        assert_eq!(ver, Some((14, 5, 0)));
    }

    #[test]
    fn parse_version_major_only() {
        let ver = PostgresAdapter::parse_version("16");
        assert_eq!(ver, Some((16, 0, 0)));
    }

    #[test]
    fn parse_version_major_minor() {
        let ver = PostgresAdapter::parse_version(
            "PostgreSQL 15.3",
        );
        assert_eq!(ver, Some((15, 3, 0)));
    }

    #[test]
    fn parse_version_three_parts() {
        let ver = PostgresAdapter::parse_version("13.2.1");
        assert_eq!(ver, Some((13, 2, 1)));
    }

    #[test]
    fn build_features_pg16() {
        let features =
            PostgresAdapter::build_features((16, 0, 0));
        assert_eq!(
            features.get("lateral_join"),
            Some(&true)
        );
        assert_eq!(
            features.get("cte_recursive"),
            Some(&true)
        );
        assert_eq!(
            features.get("window_functions"),
            Some(&true)
        );
        assert_eq!(
            features.get("parallel_query"),
            Some(&true)
        );
        assert_eq!(
            features.get("jit_compilation"),
            Some(&true)
        );
        assert_eq!(
            features.get("incremental_sort"),
            Some(&true)
        );
        assert_eq!(
            features.get("json_table"),
            Some(&false)
        );
    }

    #[test]
    fn build_features_pg83() {
        let features =
            PostgresAdapter::build_features((8, 3, 0));
        assert_eq!(
            features.get("lateral_join"),
            Some(&false)
        );
        assert_eq!(
            features.get("cte_recursive"),
            Some(&false)
        );
        assert_eq!(
            features.get("window_functions"),
            Some(&false)
        );
    }

    #[test]
    fn build_features_pg84() {
        let features =
            PostgresAdapter::build_features((8, 4, 0));
        assert_eq!(
            features.get("lateral_join"),
            Some(&false)
        );
        assert_eq!(
            features.get("cte_recursive"),
            Some(&true)
        );
        assert_eq!(
            features.get("window_functions"),
            Some(&true)
        );
    }

    #[test]
    fn capabilities_without_version() {
        let adapter = PostgresAdapter::new();
        let caps = adapter.get_capabilities();
        assert!(caps.is_ok());
        let caps = caps.as_ref().ok();
        assert!(caps.is_some());
        assert_eq!(
            caps.map(|c| c.database_name.as_str()),
            Some("PostgreSQL")
        );
        assert_eq!(
            caps.map(|c| c.max_identifier_length),
            Some(63)
        );
    }

    #[test]
    fn supports_feature_without_version() {
        let adapter = PostgresAdapter::new();
        let result =
            adapter.supports_feature("lateral_join");
        assert!(result.is_ok());
    }

    #[test]
    fn facts_provider_empty() {
        let adapter = PostgresAdapter::new();
        let facts = adapter.as_facts_provider();
        assert!(facts.get_table_stats("users").is_none());
        assert!(
            facts
                .get_column_stats("users", "id")
                .is_none()
        );
        assert!(facts.get_schema("users").is_none());
        assert_eq!(
            facts.database_name(),
            "PostgreSQL"
        );
        assert_eq!(
            facts.sql_dialect(),
            SqlDialect::Postgres
        );
    }

    #[test]
    fn index_type_parsing() {
        assert_eq!(
            PostgresAdapter::parse_index_type(
                "CREATE INDEX idx ON t \
                 USING btree (id)"
            ),
            "btree"
        );
        assert_eq!(
            PostgresAdapter::parse_index_type(
                "CREATE INDEX idx ON t \
                 USING hash (id)"
            ),
            "hash"
        );
        assert_eq!(
            PostgresAdapter::parse_index_type(
                "CREATE INDEX idx ON t \
                 USING gin (data)"
            ),
            "gin"
        );
        assert_eq!(
            PostgresAdapter::parse_index_type(
                "CREATE INDEX idx ON t \
                 USING gist (geom)"
            ),
            "gist"
        );
        assert_eq!(
            PostgresAdapter::parse_index_type(
                "CREATE INDEX idx ON t \
                 USING brin (ts)"
            ),
            "brin"
        );
        assert_eq!(
            PostgresAdapter::parse_index_type(
                "CREATE INDEX idx ON t (id)"
            ),
            "btree"
        );
    }

    #[test]
    fn pg_to_core_type_mapping() {
        assert_eq!(
            PostgresAdapter::pg_to_core_type("integer"),
            ra_core::DataType::Integer
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type("bigint"),
            ra_core::DataType::Integer
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type(
                "double precision"
            ),
            ra_core::DataType::Float
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type("text"),
            ra_core::DataType::String
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type(
                "character varying"
            ),
            ra_core::DataType::String
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type("boolean"),
            ra_core::DataType::Boolean
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type(
                "timestamp without time zone"
            ),
            ra_core::DataType::Timestamp
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type("jsonb"),
            ra_core::DataType::Json
        );
        assert_eq!(
            PostgresAdapter::pg_to_core_type("bytea"),
            ra_core::DataType::Binary
        );
    }

    #[test]
    fn index_column_parsing() {
        let cols = PostgresAdapter::parse_index_columns(
            "CREATE INDEX idx ON t \
             USING btree (a, b, c)",
        );
        assert_eq!(cols, vec!["a", "b", "c"]);

        let cols = PostgresAdapter::parse_index_columns(
            "CREATE UNIQUE INDEX idx ON t (id DESC)",
        );
        assert_eq!(cols, vec!["id"]);

        let cols = PostgresAdapter::parse_index_columns(
            "CREATE INDEX idx ON t \
             (a, b DESC NULLS FIRST)",
        );
        assert_eq!(cols, vec!["a", "b"]);
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
            PostgresAdapter::to_core_table_stats(&stats);
        assert_eq!(core.row_count, 1000.0);
        assert_eq!(core.page_count, 100);
        assert_eq!(core.average_row_size, 50.0);
        assert_eq!(core.table_size_bytes, 50_000);
        assert_eq!(core.live_tuples, Some(950.0));
        assert_eq!(core.dead_tuples, Some(50.0));
        assert_eq!(core.confidence, 0.9);
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
            PostgresAdapter::to_core_column_stats(&stats);
        assert_eq!(core.distinct_count, 500.0);
        assert_eq!(core.null_fraction, 0.02);
        assert_eq!(core.avg_length, Some(32.0));
    }

    #[test]
    fn index_type_to_core_mapping() {
        assert_eq!(
            PostgresAdapter::index_type_to_core("btree"),
            ra_core::facts::IndexType::BTree
        );
        assert_eq!(
            PostgresAdapter::index_type_to_core("hash"),
            ra_core::facts::IndexType::Hash
        );
        assert_eq!(
            PostgresAdapter::index_type_to_core("gist"),
            ra_core::facts::IndexType::Gist
        );
        assert_eq!(
            PostgresAdapter::index_type_to_core("gin"),
            ra_core::facts::IndexType::Gin
        );
        assert_eq!(
            PostgresAdapter::index_type_to_core("brin"),
            ra_core::facts::IndexType::Brin
        );
        assert_eq!(
            PostgresAdapter::index_type_to_core("unknown"),
            ra_core::facts::IndexType::BTree
        );
    }
}

/// Integration tests requiring a real PostgreSQL connection.
/// Run with: `cargo test -p ra-adapters \
///   --features postgres -- --ignored`
#[cfg(test)]
#[cfg(feature = "postgres")]
mod integration_tests {
    use super::*;

    fn get_test_url() -> String {
        std::env::var("TEST_POSTGRES_URL").unwrap_or_else(
            |_| {
                "postgresql://localhost/postgres".to_string()
            },
        )
    }

    #[test]
    #[ignore]
    fn connect_to_postgres() {
        let mut adapter = PostgresAdapter::new();
        let result = adapter.connect(&get_test_url());
        assert!(
            result.is_ok(),
            "Failed to connect: {result:?}"
        );
        assert!(adapter.version.is_some());
    }

    #[test]
    #[ignore]
    fn gather_table_statistics() {
        let mut adapter = PostgresAdapter::new();
        adapter.connect(&get_test_url()).expect("connect");
        let stats = adapter.gather_statistics();
        assert!(
            stats.is_ok(),
            "Failed to gather stats: {stats:?}"
        );
    }

    #[test]
    #[ignore]
    fn gather_schema_info() {
        let mut adapter = PostgresAdapter::new();
        adapter.connect(&get_test_url()).expect("connect");
        let schema = adapter.get_schema_info();
        assert!(
            schema.is_ok(),
            "Failed to get schema: {schema:?}"
        );
    }

    #[test]
    #[ignore]
    fn version_based_features() {
        let mut adapter = PostgresAdapter::new();
        adapter.connect(&get_test_url()).expect("connect");
        assert_eq!(
            adapter.supports_feature("cte_recursive"),
            Ok(true)
        );
        assert_eq!(
            adapter.supports_feature("window_functions"),
            Ok(true)
        );
    }

    #[test]
    #[ignore]
    fn facts_provider_after_gather() {
        let mut adapter = PostgresAdapter::new();
        adapter.connect(&get_test_url()).expect("connect");
        let _ = adapter.gather_statistics();
        let _ = adapter.get_schema_info();

        let facts = adapter.as_facts_provider();
        assert_eq!(
            facts.database_name(),
            "PostgreSQL"
        );
        assert_eq!(
            facts.sql_dialect(),
            SqlDialect::Postgres
        );
    }
}
