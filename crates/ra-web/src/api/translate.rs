//! POST /api/translate - Translate SQL between database dialects.

use ra_dialect::{Dialect, DialectTranslator};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};

/// Request body for SQL translation.
#[derive(Debug, Deserialize)]
pub struct TranslateRequest {
    /// SQL statement to translate.
    pub sql: String,
    /// Source dialect name.
    pub from: String,
    /// Target dialect name.
    pub to: String,
}

/// Response body from SQL translation.
#[derive(Debug, Serialize)]
pub struct TranslateResponse {
    /// The translated SQL string.
    pub sql: String,
    /// Source dialect used.
    pub from: String,
    /// Target dialect used.
    pub to: String,
    /// Any warnings from the translation.
    pub warnings: Vec<TranslationWarningDto>,
}

/// A translation warning in the response.
#[derive(Debug, Serialize)]
pub struct TranslationWarningDto {
    /// Warning message.
    pub message: String,
    /// Severity level.
    pub severity: String,
}

fn parse_dialect(name: &str) -> Result<Dialect, AppError> {
    match name.to_lowercase().as_str() {
        "postgresql" | "postgres" | "pg" => Ok(Dialect::PostgreSql),
        "mysql" => Ok(Dialect::MySql),
        "sqlite" => Ok(Dialect::Sqlite),
        "duckdb" => Ok(Dialect::DuckDb),
        "sqlserver" | "mssql" => Ok(Dialect::MsSql),
        "oracle" => Ok(Dialect::Oracle),
        _ => Err(AppError::bad_request(
            "invalid_dialect",
            format!("unsupported dialect '{name}', use: postgresql, mysql, sqlite, duckdb, sqlserver, oracle"),
        )),
    }
}

/// Translate SQL from one dialect to another.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/translate", data = "<req>")]
pub fn translate(
    req: Json<TranslateRequest>,
) -> ApiResult<TranslateResponse> {
    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL statement cannot be empty",
        ));
    }

    let from = parse_dialect(&req.from)?;
    let to = parse_dialect(&req.to)?;

    let translator = DialectTranslator::new(from, to);
    let result = translator.translate(&req.sql).map_err(|e| {
        AppError::bad_request("translation_error", e.to_string())
    })?;

    let warnings = result
        .warnings
        .iter()
        .map(|w| TranslationWarningDto {
            message: w.message.clone(),
            severity: format!("{:?}", w.severity),
        })
        .collect();

    Ok(Json(TranslateResponse {
        sql: result.sql,
        from: req.from.clone(),
        to: req.to.clone(),
        warnings,
    }))
}
