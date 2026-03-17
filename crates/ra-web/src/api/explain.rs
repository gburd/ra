//! POST /api/explain - Get the EXPLAIN output for a SQL query.

use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};

/// Request body for EXPLAIN.
#[derive(Debug, Deserialize)]
pub struct ExplainRequest {
    /// SQL statement to explain.
    pub sql: String,
    /// Target database engine ("sqlite" or "duckdb").
    pub engine: String,
    /// Whether to include ANALYZE timing data.
    #[serde(default)]
    pub analyze: bool,
}

/// Response body from EXPLAIN.
#[derive(Debug, Serialize)]
pub struct ExplainResponse {
    /// The EXPLAIN output as structured rows.
    pub plan: Vec<ExplainNode>,
    /// Engine that produced the plan.
    pub engine: String,
    /// Whether ANALYZE was included.
    pub analyzed: bool,
}

/// A single node in the EXPLAIN output tree.
#[derive(Debug, Serialize)]
pub struct ExplainNode {
    /// Indentation depth (for tree display).
    pub depth: usize,
    /// Node type (e.g., "Seq Scan", "Hash Join").
    pub node_type: String,
    /// Additional detail text.
    pub detail: String,
}

/// Get EXPLAIN output for a SQL query.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/explain", data = "<req>")]
pub fn explain(
    req: Json<ExplainRequest>,
) -> ApiResult<ExplainResponse> {
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

    // In production this would run EXPLAIN via a WASM adapter.
    // Return a placeholder confirming valid request.
    Ok(Json(ExplainResponse {
        plan: vec![],
        engine,
        analyzed: req.analyze,
    }))
}
