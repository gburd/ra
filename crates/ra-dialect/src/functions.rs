//! Function and operator mapping tables across SQL dialects.
//!
//! Different databases name the same operations differently.
//! This module provides lookup tables to translate function calls
//! and operators between dialects.

use crate::dialect::Dialect;
use std::collections::HashMap;

/// A function mapping entry: source function name maps to
/// target function name, with optional argument rewriting.
#[derive(Debug, Clone)]
pub struct FunctionMapping {
    /// Target function name in the destination dialect.
    pub target_name: String,
    /// Whether arguments need reordering or wrapping.
    pub transform: ArgTransform,
}

/// How to transform function arguments during translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgTransform {
    /// Pass arguments through unchanged.
    Identity,
    /// Wrap each argument with the given function.
    WrapArgs(String),
    /// Rewrite to a completely different expression pattern.
    /// The string is a template: `{0}`, `{1}`, etc. reference
    /// positional arguments.
    Template(String),
}

/// Build the function mapping table for translating from a
/// generic/PostgreSQL-style SQL to the given target dialect.
#[must_use]
pub fn build_function_map(target: Dialect) -> HashMap<String, FunctionMapping> {
    let mut map = HashMap::new();

    match target {
        Dialect::PostgreSql | Dialect::DuckDb => {
            // PostgreSQL and DuckDB share most function names
        }
        Dialect::MySql => {
            build_mysql_functions(&mut map);
        }
        Dialect::Sqlite => {
            build_sqlite_functions(&mut map);
        }
        Dialect::MsSql => {
            build_mssql_functions(&mut map);
        }
        Dialect::Oracle => {
            build_oracle_functions(&mut map);
        }
    }

    map
}

fn build_mysql_functions(map: &mut HashMap<String, FunctionMapping>) {
    // String functions
    map.insert(
        "LENGTH".into(),
        FunctionMapping {
            target_name: "CHAR_LENGTH".into(),
            transform: ArgTransform::Identity,
        },
    );

    // Date/time functions
    map.insert(
        "NOW".into(),
        FunctionMapping {
            target_name: "NOW".into(),
            transform: ArgTransform::Identity,
        },
    );

    // MySQL uses IFNULL instead of COALESCE for two args,
    // but COALESCE also works, so no mapping needed.

    // EXTRACT is supported natively.
}

fn build_sqlite_functions(map: &mut HashMap<String, FunctionMapping>) {
    // SQLite uses strftime for date extraction
    map.insert(
        "EXTRACT".into(),
        FunctionMapping {
            target_name: "STRFTIME".into(),
            transform: ArgTransform::Template("STRFTIME('{0}', {1})".into()),
        },
    );

    // SQLite LENGTH works natively
}

fn build_mssql_functions(map: &mut HashMap<String, FunctionMapping>) {
    // MSSQL uses LEN instead of LENGTH
    map.insert(
        "LENGTH".into(),
        FunctionMapping {
            target_name: "LEN".into(),
            transform: ArgTransform::Identity,
        },
    );

    // MSSQL uses GETDATE() instead of NOW()
    map.insert(
        "NOW".into(),
        FunctionMapping {
            target_name: "GETDATE".into(),
            transform: ArgTransform::Identity,
        },
    );

    // MSSQL uses DATEPART instead of EXTRACT
    map.insert(
        "EXTRACT".into(),
        FunctionMapping {
            target_name: "DATEPART".into(),
            transform: ArgTransform::Identity,
        },
    );

    // MSSQL SUBSTRING uses different syntax:
    // SUBSTRING(str, start, length) -- same positional args
    // but PostgreSQL uses SUBSTRING(str FROM start FOR len)
}

fn build_oracle_functions(map: &mut HashMap<String, FunctionMapping>) {
    // Oracle uses NVL instead of COALESCE for 2-arg case
    // (COALESCE also works in modern Oracle, no mapping needed)

    // Oracle uses SYSDATE instead of NOW()
    map.insert(
        "NOW".into(),
        FunctionMapping {
            target_name: "SYSDATE".into(),
            transform: ArgTransform::Identity,
        },
    );

    // Oracle uses SUBSTR instead of SUBSTRING
    map.insert(
        "SUBSTRING".into(),
        FunctionMapping {
            target_name: "SUBSTR".into(),
            transform: ArgTransform::Identity,
        },
    );
}

/// Operator equivalents across dialects.
/// Returns the SQL string for string concatenation in the target
/// dialect.
#[must_use]
pub fn concat_operator(target: Dialect) -> &'static str {
    match target {
        Dialect::PostgreSql | Dialect::Sqlite | Dialect::DuckDb | Dialect::Oracle => "||",
        Dialect::MsSql => "+",
        Dialect::MySql => "CONCAT",
    }
}

/// Returns the SQL expression for getting the current timestamp.
#[must_use]
pub fn current_timestamp_expr(target: Dialect) -> &'static str {
    match target {
        Dialect::PostgreSql | Dialect::Sqlite | Dialect::DuckDb => "CURRENT_TIMESTAMP",
        Dialect::MySql => "NOW()",
        Dialect::MsSql => "GETDATE()",
        Dialect::Oracle => "SYSDATE",
    }
}

/// Returns the function name for string length.
#[must_use]
pub fn length_function(target: Dialect) -> &'static str {
    match target {
        Dialect::PostgreSql
        | Dialect::MySql
        | Dialect::Sqlite
        | Dialect::DuckDb
        | Dialect::Oracle => "LENGTH",
        Dialect::MsSql => "LEN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_function_map() {
        let map = build_function_map(Dialect::MySql);
        let entry = map.get("LENGTH");
        assert!(entry.is_some());
        assert_eq!(entry.map(|e| e.target_name.as_str()), Some("CHAR_LENGTH"));
    }

    #[test]
    fn mssql_function_map() {
        let map = build_function_map(Dialect::MsSql);
        assert_eq!(
            map.get("LENGTH").map(|e| e.target_name.as_str()),
            Some("LEN")
        );
        assert_eq!(
            map.get("NOW").map(|e| e.target_name.as_str()),
            Some("GETDATE")
        );
    }

    #[test]
    fn oracle_function_map() {
        let map = build_function_map(Dialect::Oracle);
        assert_eq!(
            map.get("NOW").map(|e| e.target_name.as_str()),
            Some("SYSDATE")
        );
        assert_eq!(
            map.get("SUBSTRING").map(|e| e.target_name.as_str()),
            Some("SUBSTR")
        );
    }

    #[test]
    fn postgres_function_map_empty() {
        let map = build_function_map(Dialect::PostgreSql);
        assert!(map.is_empty());
    }

    #[test]
    fn concat_operators() {
        assert_eq!(concat_operator(Dialect::PostgreSql), "||");
        assert_eq!(concat_operator(Dialect::MsSql), "+");
        assert_eq!(concat_operator(Dialect::MySql), "CONCAT");
    }

    #[test]
    fn current_timestamp_expressions() {
        assert_eq!(
            current_timestamp_expr(Dialect::PostgreSql),
            "CURRENT_TIMESTAMP"
        );
        assert_eq!(current_timestamp_expr(Dialect::MySql), "NOW()");
        assert_eq!(current_timestamp_expr(Dialect::MsSql), "GETDATE()");
        assert_eq!(current_timestamp_expr(Dialect::Oracle), "SYSDATE");
    }

    #[test]
    fn length_functions() {
        assert_eq!(length_function(Dialect::PostgreSql), "LENGTH");
        assert_eq!(length_function(Dialect::MsSql), "LEN");
    }

    #[test]
    fn arg_transform_equality() {
        assert_eq!(ArgTransform::Identity, ArgTransform::Identity);
        assert_ne!(
            ArgTransform::Identity,
            ArgTransform::WrapArgs("LOWER".into())
        );
    }
}
