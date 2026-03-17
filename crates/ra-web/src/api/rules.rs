//! POST /api/rules - Query rule metadata.

use ra_engine::all_rules;
use rocket::serde::json::Json;
use serde::Serialize;

use crate::errors::ApiResult;

/// Response body listing available optimization rules.
#[derive(Debug, Serialize)]
pub struct RulesResponse {
    /// Total number of rules.
    pub count: usize,
    /// Rule names.
    pub rules: Vec<String>,
}

/// List all available optimization rules.
#[allow(clippy::unnecessary_wraps)]
#[rocket::get("/api/rules")]
pub fn list_rules() -> ApiResult<RulesResponse> {
    let rules = all_rules();
    let names: Vec<String> =
        rules.iter().map(|r| r.name.to_string()).collect();

    Ok(Json(RulesResponse {
        count: names.len(),
        rules: names,
    }))
}
