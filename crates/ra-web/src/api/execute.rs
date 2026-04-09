//! POST /api/execute - Execute SQL across a database backend.

use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::config::DatabaseConfig;
use crate::errors::{ApiResult, AppError};

/// Request body for SQL execution.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecuteRequest {
    /// SQL statement to execute.
    pub sql: String,
    /// Target database engine ("sqlite", "duckdb", "postgresql", "mysql", "mariadb").
    pub engine: String,
    /// Optional database configuration.
    pub config: Option<DatabaseConfig>,
}

/// Response body from SQL execution.
#[derive(Debug, Serialize)]
pub struct ExecuteResponse {
    /// Column names in the result set.
    pub columns: Vec<String>,
    /// Rows as vectors of string values.
    pub rows: Vec<Vec<String>>,
    /// Number of rows affected (for DML).
    pub rows_affected: u64,
    /// Execution engine used.
    pub engine: String,
    /// Execution time in milliseconds.
    pub execution_time_ms: f64,
}

/// Execute SQL against a database backend.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/execute", data = "<req>")]
pub async fn execute(
    req: Json<ExecuteRequest>,
) -> ApiResult<ExecuteResponse> {
    let start = Instant::now();

    let engine = req.engine.to_lowercase();
    if engine != "sqlite"
        && engine != "duckdb"
        && engine != "postgresql"
        && engine != "mysql"
        && engine != "mariadb"
        && engine != "mariadb-11"
    {
        return Err(AppError::bad_request(
            "invalid_engine",
            format!(
                "unsupported engine '{}', use 'sqlite', 'duckdb', 'postgresql', 'mysql', or 'mariadb'",
                req.engine
            ),
        ));
    }

    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL statement cannot be empty",
        ));
    }

    // Get database configuration
    let config = req.config.clone().unwrap_or_else(|| match engine.as_str() {
        "sqlite" => DatabaseConfig::sqlite(":memory:"),
        "duckdb" => DatabaseConfig::duckdb(":memory:"),
        "postgresql" => DatabaseConfig::postgres("postgresql://localhost/test"),
        "mysql" => DatabaseConfig::mysql("mysql://localhost/test"),
        "mariadb" | "mariadb-11" => DatabaseConfig::mysql("mysql://localhost/test"),
        _ => DatabaseConfig::sqlite(":memory:"),
    });

    // Execute the query with timeout
    let (columns, rows, rows_affected) = execute_query(&req.sql, &config).await?;

    let execution_time = start.elapsed().as_secs_f64() * 1000.0;

    Ok(Json(ExecuteResponse {
        columns,
        rows,
        rows_affected,
        engine,
        execution_time_ms: execution_time,
    }))
}

/// Execute a SQL query against the specified database.
async fn execute_query(
    sql: &str,
    config: &DatabaseConfig,
) -> Result<(Vec<String>, Vec<Vec<String>>, u64), AppError> {
    use ra_adapters::{DatabaseAdapter, DuckDBAdapter, MySQLAdapter, PostgresAdapter, SQLiteAdapter};
    use std::time::Duration;

    let timeout_secs = std::env::var("QUERY_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    let query_timeout = Duration::from_secs(timeout_secs);

    match config {
        DatabaseConfig::PostgreSQL { connection_string, pool_size: _ } => {
            let connection_string = connection_string.clone();
            let sql = sql.to_owned();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = PostgresAdapter::new();
                    adapter.connect(&connection_string)?;
                    adapter.execute(&sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "Query execution timed out after {timeout_secs} seconds. \
                    Consider optimizing the query or increasing QUERY_TIMEOUT_SECS."
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("PostgreSQL task failed: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("PostgreSQL query failed: {e}"))
            })?;

            let columns = extract_columns_from_json(&result.rows);
            let rows = json_rows_to_string_rows(&result.rows);
            Ok((columns, rows, result.row_count as u64))
        }

        DatabaseConfig::MySQL { connection_string, pool_size: _ } => {
            let connection_string = connection_string.clone();
            let sql = sql.to_owned();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = MySQLAdapter::new();
                    adapter.connect(&connection_string)?;
                    adapter.execute(&sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "Query execution timed out after {timeout_secs} seconds. \
                    Consider optimizing the query or increasing QUERY_TIMEOUT_SECS."
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("MySQL task failed: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("MySQL query failed: {e}"))
            })?;

            let columns = extract_columns_from_json(&result.rows);
            let rows = json_rows_to_string_rows(&result.rows);
            Ok((columns, rows, result.row_count as u64))
        }

        DatabaseConfig::SQLite { database_path } => {
            let database_path = database_path.clone();
            let sql = sql.to_owned();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = SQLiteAdapter::new();
                    adapter.connect(&database_path)?;
                    adapter.execute(&sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "Query execution timed out after {timeout_secs} seconds. \
                    Consider optimizing the query or increasing QUERY_TIMEOUT_SECS."
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("SQLite task failed: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("SQLite query failed: {e}"))
            })?;

            let columns = extract_columns_from_json(&result.rows);
            let rows = json_rows_to_string_rows(&result.rows);
            Ok((columns, rows, result.row_count as u64))
        }

        DatabaseConfig::DuckDB { database_path } => {
            let database_path = database_path.clone();
            let sql = sql.to_owned();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = DuckDBAdapter::new();
                    adapter.connect(&database_path)?;
                    adapter.execute(&sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "Query execution timed out after {timeout_secs} seconds. \
                    Consider optimizing the query or increasing QUERY_TIMEOUT_SECS."
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("DuckDB task failed: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("DuckDB query failed: {e}"))
            })?;

            let columns = extract_columns_from_hashmaps(&result.rows);
            let rows = hashmap_rows_to_string_rows(&result.rows);
            Ok((columns, rows, result.row_count as u64))
        }
    }
}

/// Extract column names from JSON rows (PostgreSQL format).
fn extract_columns_from_json(rows: &[serde_json::Value]) -> Vec<String> {
    if let Some(first_row) = rows.first() {
        if let serde_json::Value::Object(map) = first_row {
            return map.keys().cloned().collect();
        }
    }
    vec![]
}

/// Convert JSON rows to string rows (PostgreSQL format).
fn json_rows_to_string_rows(rows: &[serde_json::Value]) -> Vec<Vec<String>> {
    rows.iter()
        .filter_map(|row| {
            if let serde_json::Value::Object(map) = row {
                Some(
                    map.values()
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Null => "NULL".to_string(),
                            other => other.to_string(),
                        })
                        .collect(),
                )
            } else {
                None
            }
        })
        .collect()
}

/// Extract column names from HashMap rows (DuckDB format).
fn extract_columns_from_hashmaps(rows: &[std::collections::HashMap<String, serde_json::Value>]) -> Vec<String> {
    if let Some(first_row) = rows.first() {
        return first_row.keys().cloned().collect();
    }
    vec![]
}

/// Convert HashMap rows to string rows (DuckDB format).
fn hashmap_rows_to_string_rows(rows: &[std::collections::HashMap<String, serde_json::Value>]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| {
            row.values()
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => "NULL".to_string(),
                    other => other.to_string(),
                })
                .collect()
        })
        .collect()
}
