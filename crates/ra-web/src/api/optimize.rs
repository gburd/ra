//! POST /api/optimize - Optimize a relational algebra expression.

use ra_core::algebra::RelExpr;
use ra_engine::Optimizer;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};
use crate::rate_limit::RateGuard;

/// Request body for query optimization.
#[derive(Debug, Deserialize)]
pub struct OptimizeRequest {
    /// JSON-serialized relational algebra expression.
    pub expr: serde_json::Value,
}

/// Response body from optimization.
#[derive(Debug, Serialize)]
pub struct OptimizeResponse {
    /// The original expression.
    pub original: serde_json::Value,
    /// The optimized expression.
    pub optimized: serde_json::Value,
    /// Number of rules applied.
    pub rules_applied: usize,
}

/// Optimize a relational algebra expression.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/optimize", data = "<req>")]
pub fn optimize(
    _rate: RateGuard,
    req: Json<OptimizeRequest>,
) -> ApiResult<OptimizeResponse> {
    let expr: RelExpr =
        serde_json::from_value(req.expr.clone()).map_err(|e| {
            AppError::bad_request(
                "invalid_expr",
                format!("failed to parse expression: {e}"),
            )
        })?;

    let optimizer = Optimizer::new();
    let optimized = optimizer.optimize(&expr).map_err(|e| {
        AppError::internal(format!("optimization failed: {e}"))
    })?;

    let optimized_json =
        serde_json::to_value(&optimized).map_err(|e| {
            AppError::internal(format!(
                "failed to serialize result: {e}"
            ))
        })?;

    Ok(Json(OptimizeResponse {
        original: req.expr.clone(),
        optimized: optimized_json,
        rules_applied: 0,
    }))
}
