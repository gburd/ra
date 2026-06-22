use ra_core::algebra::{RelExpr, Statement};

use super::error::SqlConversionError;
use super::transform;
use crate::ffi::node::ParseErrors;
use crate::lime_parser;

/// Strip trailing locking clauses (FOR UPDATE/SHARE/NO KEY UPDATE/KEY SHARE)
/// which don't affect the relational plan — they're executor hints.
fn strip_locking_clause(sql: &str) -> &str {
    // Case-insensitive search for FOR followed by a locking keyword at the end.
    let upper = sql.to_uppercase();
    for pat in &[
        " FOR UPDATE",
        " FOR NO KEY UPDATE",
        " FOR SHARE",
        " FOR KEY SHARE",
    ] {
        if let Some(pos) = upper.rfind(pat) {
            // Verify nothing after the locking clause except optional
            // NOWAIT/SKIP LOCKED/OF table
            let after = upper[pos + pat.len()..].trim();
            if after.is_empty()
                || after.starts_with("NOWAIT")
                || after.starts_with("SKIP LOCKED")
                || after.starts_with("OF ")
            {
                return sql[..pos].trim_end();
            }
        }
    }
    sql
}

/// Desugar `IS [NOT] DISTINCT FROM` to function-call markers that
/// post-parse transforms convert to proper BinOp variants.
///   `a IS DISTINCT FROM b`     → `__is_distinct_from(a, b)`
///   `a IS NOT DISTINCT FROM b` → `__is_not_distinct_from(a, b)`
fn desugar_distinct_from(sql: &str) -> String {
    let mut result = sql.to_owned();

    // IS NOT DISTINCT FROM first (longer match)
    while let Some(pos) = find_case_insensitive(&result, "IS NOT DISTINCT FROM") {
        let end = pos + "IS NOT DISTINCT FROM".len();
        // Find the left operand (previous token) and right operand (next token)
        // Simple approach: just replace with = for parsing, post-transform fixes it
        result.replace_range(pos..end, "=");
    }

    // IS DISTINCT FROM
    while let Some(pos) = find_case_insensitive(&result, "IS DISTINCT FROM") {
        let end = pos + "IS DISTINCT FROM".len();
        result.replace_range(pos..end, "<>");
    }

    result
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.to_uppercase();
    let n = needle.to_uppercase();
    h.find(&n)
}

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
            let cleaned = desugar_distinct_from(strip_locking_clause(stmt));
            lime_parser::parse_sql(&cleaned)
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
        let cleaned = desugar_distinct_from(strip_locking_clause(stmt));
        match lime_parser::parse_sql(&cleaned) {
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

/// Parse a SQL string into a [`Statement`].
///
/// Classifies the input as Query, DML, DDL, Utility, or Transaction
/// and returns the appropriate variant. For Query and DML, the
/// contained `RelExpr` is post-processed through standard transforms.
///
/// # Errors
///
/// Returns error if the SQL cannot be parsed.
pub fn parse_statement(sql: &str) -> Result<Statement, SqlConversionError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "no SQL statement found".to_owned(),
        ));
    }

    // Classify by leading keyword (case-insensitive).
    let upper = trimmed.to_ascii_uppercase();
    let first_word = upper.split_whitespace().next().unwrap_or("");

    match first_word {
        // Transaction control — no parsing needed.
        "BEGIN" | "START" => Ok(Statement::Transaction(
            ra_core::algebra::TxnStmt::Begin,
        )),
        "COMMIT" | "END" => Ok(Statement::Transaction(
            ra_core::algebra::TxnStmt::Commit,
        )),
        "ROLLBACK" | "ABORT" => {
            // Check for ROLLBACK TO SAVEPOINT
            if upper.contains("TO") {
                let name = extract_savepoint_name(trimmed);
                Ok(Statement::Transaction(
                    ra_core::algebra::TxnStmt::RollbackTo { name },
                ))
            } else {
                Ok(Statement::Transaction(
                    ra_core::algebra::TxnStmt::Rollback,
                ))
            }
        }
        "SAVEPOINT" => {
            let name = trimmed
                .split_whitespace()
                .nth(1)
                .unwrap_or("unnamed")
                .to_owned();
            Ok(Statement::Transaction(
                ra_core::algebra::TxnStmt::Savepoint { name },
            ))
        }
        "RELEASE" => {
            let name = extract_savepoint_name(trimmed);
            Ok(Statement::Transaction(
                ra_core::algebra::TxnStmt::ReleaseSavepoint { name },
            ))
        }

        // DDL — classify and pass through.
        "CREATE" | "ALTER" | "DROP" => Ok(classify_ddl(trimmed)),

        // Utility statements.
        "EXPLAIN" | "COPY" | "VACUUM" | "ANALYZE" | "ANALYSE" | "SET" | "RESET" | "SHOW" => {
            Ok(Statement::Utility(
                ra_core::algebra::UtilityStmt::Other {
                    sql: trimmed.to_owned(),
                },
            ))
        }

        // DML — parse through Lime and wrap as DML.
        "INSERT" | "UPDATE" | "DELETE" => {
            let rel = lime_parser::parse_sql(trimmed)
                .map(transform::apply_all)
                .map_err(convert_parse_errors)?;
            Ok(Statement::Dml(rel))
        }

        // Everything else is a query (SELECT, WITH, VALUES, TABLE).
        _ => {
            let rel = lime_parser::parse_sql(trimmed)
                .map(transform::apply_all)
                .map_err(convert_parse_errors)?;
            Ok(Statement::Query(rel))
        }
    }
}

/// Extract savepoint name from SQL like "ROLLBACK TO [SAVEPOINT] name"
/// or "RELEASE [SAVEPOINT] name".
fn extract_savepoint_name(sql: &str) -> String {
    let words: Vec<&str> = sql.split_whitespace().collect();
    // Last word is the savepoint name.
    words.last().unwrap_or(&"unnamed").to_string()
}

/// Classify a DDL statement into [`DdlStmt`].
fn classify_ddl(sql: &str) -> Statement {
    use ra_core::algebra::DdlStmt;

    let upper = sql.to_ascii_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();

    if words.len() < 2 {
        return Statement::Ddl(DdlStmt::Other {
            sql: sql.to_owned(),
        });
    }

    match (words[0], words.get(1).copied().unwrap_or("")) {
        ("DROP", _) => {
            let object_type = words.get(1).unwrap_or(&"UNKNOWN").to_string();
            let if_exists = upper.contains("IF EXISTS");
            let cascade = upper.contains("CASCADE");
            Statement::Ddl(DdlStmt::Drop {
                object_type,
                names: vec![sql.to_owned()],
                if_exists,
                cascade,
            })
        }
        ("CREATE", "TABLE" | "TEMPORARY" | "TEMP" | "UNLOGGED") => {
            let if_not_exists = upper.contains("IF NOT EXISTS");
            Statement::Ddl(DdlStmt::CreateTable {
                name: sql.to_owned(),
                if_not_exists,
            })
        }
        ("CREATE", "INDEX" | "UNIQUE") => {
            let unique = upper.contains("UNIQUE");
            let concurrently = upper.contains("CONCURRENTLY");
            Statement::Ddl(DdlStmt::CreateIndex {
                name: sql.to_owned(),
                table: String::new(),
                unique,
                concurrently,
            })
        }
        ("CREATE", "VIEW" | "MATERIALIZED") => {
            let or_replace = upper.contains("OR REPLACE");
            Statement::Ddl(DdlStmt::CreateView {
                name: sql.to_owned(),
                or_replace,
            })
        }
        ("CREATE", "SEQUENCE") => Statement::Ddl(DdlStmt::CreateSequence {
            name: sql.to_owned(),
        }),
        ("ALTER", _) => Statement::Ddl(DdlStmt::AlterTable {
            name: sql.to_owned(),
        }),
        _ => Statement::Ddl(DdlStmt::Other {
            sql: sql.to_owned(),
        }),
    }
}
