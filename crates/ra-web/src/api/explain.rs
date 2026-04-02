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
    /// The EXPLAIN output as a formatted string.
    pub plan: String,
    /// Engine that produced the plan.
    pub engine: String,
    /// Whether ANALYZE was included.
    pub analyzed: bool,
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

    // In production this would run EXPLAIN via a WASM adapter or direct DB connection.
    // Return a placeholder with sample EXPLAIN output.
    let plan_text = if req.analyze {
        format!(
            "QUERY PLAN (EXPLAIN ANALYZE)\n\
             Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32) (actual time=0.012..0.234 rows=1000 loops=1)\n\
               Filter: (department_id = 1)\n\
               Rows Removed by Filter: 500\n\
             Planning Time: 0.123 ms\n\
             Execution Time: 0.456 ms\n\
             \n\
             Engine: {}\n\
             Query: {}",
            engine,
            req.sql.lines().next().unwrap_or("").chars().take(60).collect::<String>()
        )
    } else {
        format!(
            "QUERY PLAN\n\
             Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32)\n\
               Filter: (department_id = 1)\n\
             \n\
             Engine: {}\n\
             Query: {}",
            engine,
            req.sql.lines().next().unwrap_or("").chars().take(60).collect::<String>()
        )
    };

    Ok(Json(ExplainResponse {
        plan: plan_text,
        engine,
        analyzed: req.analyze,
    }))
}
