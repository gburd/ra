use ra_core::algebra::RelExpr;

use super::error::SqlConversionError;
use crate::lime_parser;

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
                .map_err(|errs| SqlConversionError::ParseError(errs.join("; ")))
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
            Ok(rel) => return Ok(rel),
            Err(errs) => {
                last_err = Some(errs);
            }
        }
    }

    Err(SqlConversionError::ParseError(last_err.map_or_else(
        || "no SQL statement found".to_owned(),
        |e| e.join("; "),
    )))
}
