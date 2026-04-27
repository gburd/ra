use ra_core::algebra::RelExpr;
use sqlparser::ast::Statement;
use sqlparser::parser::Parser;

use crate::parser::ProfileDialect;
use crate::profile::ParserProfile;

use super::error::SqlConversionError;
use super::query::convert_query;

/// Create an enhanced parser profile with common PostgreSQL extensions.
fn create_enhanced_profile() -> ParserProfile {
    ParserProfile::load("postgresql-17+pgvector+pg_trgm+pg_textsearch")
        .unwrap_or_else(|_| ParserProfile::universal())
}

/// Parse multiple SQL statements and convert each to a `RelExpr`.
///
/// Splits on semicolons. Non-SELECT statements are returned
/// as errors for the individual entry.
///
/// # Errors
///
/// Returns error if SQL parsing fails entirely.
pub fn sql_to_relexprs(sql: &str) -> Result<Vec<RelExpr>, SqlConversionError> {
    let profile = create_enhanced_profile();
    let dialect = ProfileDialect::new(profile);
    let statements = Parser::parse_sql(&dialect, sql)
        .map_err(|e| SqlConversionError::ParseError(e.to_string()))?;

    if statements.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "no SQL statement found".to_owned(),
        ));
    }

    statements
        .iter()
        .map(|stmt| match stmt {
            Statement::Query(query) => convert_query(query),
            _ => Err(SqlConversionError::UnsupportedFeature(
                "only SELECT queries are supported".to_owned(),
            )),
        })
        .collect()
}

/// Parse SQL and convert to RelExpr.
///
/// # Errors
///
/// Returns error if SQL is invalid or contains unsupported features.
pub fn sql_to_relexpr(sql: &str) -> Result<RelExpr, SqlConversionError> {
    let profile = create_enhanced_profile();
    let dialect = ProfileDialect::new(profile);
    let statements = Parser::parse_sql(&dialect, sql)
        .map_err(|e| SqlConversionError::ParseError(e.to_string()))?;

    if statements.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "no SQL statement found".to_owned(),
        ));
    }

    for stmt in &statements {
        if let Statement::Query(query) = stmt {
            return convert_query(query);
        }
    }

    Err(SqlConversionError::UnsupportedFeature(
        "only SELECT queries are supported".to_owned(),
    ))
}
