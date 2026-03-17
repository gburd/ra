//! POST /api/execute - Execute SQL across a database backend.

use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};

/// Request body for SQL execution.
#[derive(Debug, Deserialize)]
pub struct ExecuteRequest {
    /// SQL statement to execute.
    pub sql: String,
    /// Target database engine ("sqlite" or "duckdb").
    pub engine: String,
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
}

/// Execute SQL against a database backend.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/execute", data = "<req>")]
pub fn execute(
    req: Json<ExecuteRequest>,
) -> ApiResult<ExecuteResponse> {
    let engine = req.engine.to_lowercase();
    if engine != "sqlite" && engine != "duckdb" {
        return Err(AppError::bad_request(
            "invalid_engine",
            format!(
                "unsupported engine '{}', use 'sqlite' or 'duckdb'",
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

    // In production this would delegate to a WASM database adapter.
    // For now, return a placeholder confirming the request was valid.
    Ok(Json(ExecuteResponse {
        columns: vec![],
        rows: vec![],
        rows_affected: 0,
        engine,
    }))
}
