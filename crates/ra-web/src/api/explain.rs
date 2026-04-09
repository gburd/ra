//! POST /api/explain - Get the EXPLAIN output for a SQL query.

use redis::aio::ConnectionManager;
use rocket::serde::json::Json;
use rocket::State;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::cache;
use crate::config::DatabaseConfig;
use crate::errors::{ApiResult, AppError};

/// Request body for EXPLAIN.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExplainRequest {
    /// SQL statement to explain.
    pub sql: String,
    /// Target database engine ("sqlite", "duckdb", "postgresql", "mysql", "mariadb").
    pub engine: String,
    /// Whether to include ANALYZE timing data.
    #[serde(default)]
    pub analyze: bool,
    /// Optional database configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<DatabaseConfig>,
}

/// Response body from EXPLAIN.
#[derive(Debug, Serialize)]
pub struct ExplainResponse {
    /// The EXPLAIN output (JSON for PostgreSQL/MySQL, text for others).
    pub plan: serde_json::Value,
    /// Engine that produced the plan.
    pub engine: String,
    /// Execution time in milliseconds.
    pub execution_time_ms: f64,
}

/// Get EXPLAIN output for a SQL query.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/explain", data = "<req>")]
pub async fn explain(
    req: Json<ExplainRequest>,
    redis: &State<ConnectionManager>,
) -> ApiResult<ExplainResponse> {
    let start = Instant::now();
    let engine = req.engine.to_lowercase();

    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL statement cannot be empty",
        ));
    }

    // Check cache first
    let mut redis_conn = redis.inner().clone();
    if let Ok(Some(cached)) = cache::get_cached_plan(&mut redis_conn, &req.sql, &engine, req.analyze).await {
        tracing::info!("Returning cached EXPLAIN result for engine={}", engine);
        return Ok(Json(ExplainResponse {
            plan: cached.plan,
            engine: cached.engine,
            execution_time_ms: 0.0,
        }));
    }

    // Get database configuration
    let config = req.config.clone().unwrap_or_else(|| {
        std::env::var(&format!("{}_URL", engine.to_uppercase()))
            .ok()
            .and_then(|url| match engine.as_str() {
                "sqlite" => Some(DatabaseConfig::sqlite(&url)),
                "duckdb" => Some(DatabaseConfig::duckdb(&url)),
                "postgresql" => Some(DatabaseConfig::postgres(&url)),
                "mysql" | "mariadb" | "mariadb-11" => Some(DatabaseConfig::mysql(&url)),
                _ => None,
            })
            .unwrap_or_else(|| match engine.as_str() {
                "sqlite" => DatabaseConfig::sqlite(":memory:"),
                "duckdb" => DatabaseConfig::duckdb(":memory:"),
                "postgresql" => DatabaseConfig::postgres(
                    "postgresql://test_user:test_pass@postgres-16:5432/test_db"
                ),
                "mysql" => DatabaseConfig::mysql(
                    "mysql://test_user:test_pass@mysql-8:3306/test_db"
                ),
                "mariadb" | "mariadb-11" => DatabaseConfig::mysql(
                    "mysql://test_user:test_pass@mariadb-11:3306/test_db"
                ),
                _ => DatabaseConfig::sqlite(":memory:"),
            })
    });

    // Execute EXPLAIN query
    let plan = explain_query(&req.sql, &config, req.analyze, &engine).await?;

    let execution_time = start.elapsed().as_secs_f64() * 1000.0;

    // Cache the result for future requests
    let mut redis_conn = redis.inner().clone();
    if let Err(e) = cache::cache_plan(&mut redis_conn, &req.sql, &engine, req.analyze, &plan).await {
        tracing::warn!("Failed to cache EXPLAIN result: {}", e);
        // Continue even if caching fails
    }

    Ok(Json(ExplainResponse {
        plan,
        engine,
        execution_time_ms: execution_time,
    }))
}

/// Execute an EXPLAIN query against the specified database.
async fn explain_query(
    sql: &str,
    config: &DatabaseConfig,
    analyze: bool,
    engine: &str,
) -> Result<serde_json::Value, AppError> {
    use ra_adapters::{DatabaseAdapter, DuckDBAdapter, MySQLAdapter, PostgresAdapter, SQLiteAdapter};
    use std::time::Duration;

    let timeout_secs = std::env::var("QUERY_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    let query_timeout = Duration::from_secs(timeout_secs);

    // Construct EXPLAIN SQL based on engine
    let explain_sql = match engine {
        "postgresql" => {
            if analyze {
                format!("EXPLAIN (FORMAT JSON, ANALYZE true) {}", sql)
            } else {
                format!("EXPLAIN (FORMAT JSON) {}", sql)
            }
        }
        "mysql" | "mariadb" | "mariadb-11" => format!("EXPLAIN FORMAT=JSON {}", sql),
        "duckdb" => format!("EXPLAIN {}", sql),
        "sqlite" => format!("EXPLAIN QUERY PLAN {}", sql),
        _ => return Err(AppError::bad_request(
            "unsupported_engine",
            format!("Engine '{}' not supported for EXPLAIN", engine),
        )),
    };

    match config {
        DatabaseConfig::PostgreSQL { connection_string, pool_size: _ } => {
            let connection_string = connection_string.clone();
            let explain_sql = explain_sql.clone();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = PostgresAdapter::new();
                    adapter.connect(&connection_string)?;
                    adapter.execute(&explain_sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "EXPLAIN timed out after {timeout_secs} seconds"
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("Failed to spawn task: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("PostgreSQL EXPLAIN failed: {e}"))
            })?;

            // PostgreSQL returns JSON in the first row
            if !result.rows.is_empty() {
                if let Some(json_val) = result.rows[0].get("QUERY PLAN") {
                    return Ok(json_val.clone());
                }
                // Fallback: return entire first row
                return Ok(result.rows[0].clone());
            }

            Ok(serde_json::json!({
                "error": "No EXPLAIN output returned"
            }))
        }

        DatabaseConfig::MySQL { connection_string, pool_size: _ } => {
            let connection_string = connection_string.clone();
            let explain_sql = explain_sql.clone();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = MySQLAdapter::new();
                    adapter.connect(&connection_string)?;
                    adapter.execute(&explain_sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "EXPLAIN timed out after {timeout_secs} seconds"
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("Failed to spawn task: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("MySQL EXPLAIN failed: {e}"))
            })?;

            // MySQL returns JSON in the rows
            if !result.rows.is_empty() {
                // Try to parse the entire result as JSON
                return Ok(serde_json::to_value(&result.rows).unwrap_or_else(|_| {
                    serde_json::json!({"rows": result.rows})
                }));
            }

            Ok(serde_json::json!({
                "error": "No EXPLAIN output returned"
            }))
        }

        DatabaseConfig::SQLite { database_path } => {
            let path = database_path.clone();
            let explain_sql = explain_sql.clone();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = SQLiteAdapter::new();
                    adapter.connect(&path)?;
                    adapter.execute(&explain_sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "EXPLAIN timed out after {timeout_secs} seconds"
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("Failed to spawn task: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("SQLite EXPLAIN failed: {e}"))
            })?;

            // SQLite EXPLAIN returns text rows
            let plan_text = result.rows.iter()
                .map(|row| serde_json::to_string(row).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n");

            Ok(serde_json::json!({
                "plan": plan_text,
                "rows": result.rows
            }))
        }

        DatabaseConfig::DuckDB { database_path } => {
            let path = database_path.clone();
            let explain_sql = explain_sql.clone();

            let result = tokio::time::timeout(
                query_timeout,
                tokio::task::spawn_blocking(move || {
                    let mut adapter = DuckDBAdapter::new();
                    adapter.open(&path)?;
                    adapter.execute(&explain_sql)
                })
            )
            .await
            .map_err(|_| {
                AppError::timeout(format!(
                    "EXPLAIN timed out after {timeout_secs} seconds"
                ))
            })?
            .map_err(|e| {
                AppError::internal(format!("Failed to spawn task: {e}"))
            })?
            .map_err(|e| {
                AppError::internal(format!("DuckDB EXPLAIN failed: {e}"))
            })?;

            // DuckDB EXPLAIN returns text
            let plan_text = result.rows.iter()
                .map(|row| serde_json::to_string(row).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n");

            Ok(serde_json::json!({
                "plan": plan_text,
                "rows": result.rows
            }))
        }
    }
}
