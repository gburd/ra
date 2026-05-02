use ra_core::algebra::RelExpr;

use super::error::SqlConversionError;
use super::transform;
use crate::ffi::node::ParseErrors;
use crate::lime_parser;

/// Convert a `ParseErrors` into a `SqlConversionError`.
fn convert_parse_errors(errs: ParseErrors) -> SqlConversionError {
    match errs {
        ParseErrors::Structured(se) => {
            SqlConversionError::StructuredParseErrors(se)
        }
        ParseErrors::Strings(ss) => {
            SqlConversionError::ParseError(ss.join("; "))
        }
    }
}

/// Parse multiple SQL statements and convert each to a `RelExpr`.
///
/// Splits on semicolons. Non-SELECT statements produce errors for that entry.
///
/// # Errors
///
/// Returns error if SQL parsing fails entirely or no statements are found.
pub fn sql_to_relexprs(sql: &str) -> Result<Vec<RelExpr>, SqlConversionError> {
    let statements: Vec<&str> = sql
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if statements.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "no SQL statement found".to_owned(),
        ));
    }

    statements
        .iter()
        .map(|stmt| {
            lime_parser::parse_sql(stmt)
                .map(transform::apply_all)
                .map_err(convert_parse_errors)
        })
        .collect()
}

/// Parse SQL and convert to `RelExpr`.
///
/// If multiple semicolon-separated statements are provided, returns
/// the first one that successfully parses as a query.
///
/// # Errors
///
/// Returns error if SQL is invalid or contains unsupported features.
pub fn sql_to_relexpr(sql: &str) -> Result<RelExpr, SqlConversionError> {
    let statements: Vec<&str> = sql
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if statements.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "no SQL statement found".to_owned(),
        ));
    }

    // Try each statement; return the first successful parse.
    let mut last_err = None;
    for stmt in &statements {
        match lime_parser::parse_sql(stmt) {
            Ok(rel) => return Ok(transform::apply_all(rel)),
            Err(errs) => {
                last_err = Some(errs);
            }
        }
    }

    Err(last_err.map_or_else(
        || {
            SqlConversionError::InvalidSql(
                "no SQL statement found".to_owned(),
            )
        },
        convert_parse_errors,
    ))
}
