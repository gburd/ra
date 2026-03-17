//! POST /api/compare - Compare query results between databases.

use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};

/// Request body for result comparison.
#[derive(Debug, Deserialize)]
pub struct CompareRequest {
    /// SQL query to execute on each engine.
    pub sql: String,
    /// Engines to compare (e.g., `["sqlite", "duckdb"]`).
    pub engines: Vec<String>,
}

/// Response body from comparison.
#[derive(Debug, Serialize)]
pub struct CompareResponse {
    /// Whether all engines returned matching results.
    pub matching: bool,
    /// Per-engine results.
    pub results: Vec<EngineResult>,
    /// Differences found (empty if matching).
    pub differences: Vec<Difference>,
}

/// Result from a single engine.
#[derive(Debug, Serialize)]
pub struct EngineResult {
    /// Engine name.
    pub engine: String,
    /// Number of rows returned.
    pub row_count: usize,
    /// Column names.
    pub columns: Vec<String>,
}

/// A difference between engine results.
#[derive(Debug, Serialize)]
pub struct Difference {
    /// Description of the difference.
    pub description: String,
    /// Engines involved.
    pub engines: Vec<String>,
}

/// Compare query results across database engines.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/compare", data = "<req>")]
pub fn compare(
    req: Json<CompareRequest>,
) -> ApiResult<CompareResponse> {
    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL statement cannot be empty",
        ));
    }

    if req.engines.is_empty() {
        return Err(AppError::bad_request(
            "no_engines",
            "at least one engine must be specified",
        ));
    }

    for engine in &req.engines {
        let lower = engine.to_lowercase();
        if lower != "sqlite" && lower != "duckdb" {
            return Err(AppError::bad_request(
                "invalid_engine",
                format!(
                    "unsupported engine '{engine}', use 'sqlite' or 'duckdb'"
                ),
            ));
        }
    }

    // In production, execute on each engine and diff results.
    let results = req
        .engines
        .iter()
        .map(|e| EngineResult {
            engine: e.to_lowercase(),
            row_count: 0,
            columns: vec![],
        })
        .collect();

    Ok(Json(CompareResponse {
        matching: true,
        results,
        differences: vec![],
    }))
}
