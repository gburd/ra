//! SQL dialect definitions and feature support matrices.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported SQL dialects for translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dialect {
    /// `PostgreSQL` (9.6+).
    PostgreSql,
    /// `MySQL` (5.7+ / 8.0+).
    MySql,
    /// `SQLite` (3.x).
    Sqlite,
    /// `DuckDB`.
    DuckDb,
    /// Microsoft SQL Server (2016+).
    MsSql,
    /// Oracle Database (12c+).
    Oracle,
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::PostgreSql => "PostgreSQL",
            Self::MySql => "MySQL",
            Self::Sqlite => "SQLite",
            Self::DuckDb => "DuckDB",
            Self::MsSql => "MSSQL",
            Self::Oracle => "Oracle",
        };
        write!(f, "{name}")
    }
}

impl Dialect {
    /// All supported dialects.
    pub const ALL: [Self; 6] = [
        Self::PostgreSql,
        Self::MySql,
        Self::Sqlite,
        Self::DuckDb,
        Self::MsSql,
        Self::Oracle,
    ];

    /// Returns the `sqlparser` dialect implementation for
    /// parsing.
    #[must_use]
    pub fn sqlparser_dialect(self) -> Box<dyn sqlparser::dialect::Dialect> {
        match self {
            Self::PostgreSql => Box::new(sqlparser::dialect::PostgreSqlDialect {}),
            Self::MySql => Box::new(sqlparser::dialect::MySqlDialect {}),
            Self::Sqlite => Box::new(sqlparser::dialect::SQLiteDialect {}),
            Self::DuckDb => Box::new(sqlparser::dialect::DuckDbDialect {}),
            Self::MsSql => Box::new(sqlparser::dialect::MsSqlDialect {}),
            Self::Oracle => Box::new(sqlparser::dialect::GenericDialect {}),
        }
    }

    /// Returns the identifier quoting style for this dialect.
    #[must_use]
    pub fn quote_style(self) -> char {
        match self {
            Self::MySql => '`',
            Self::PostgreSql | Self::Sqlite | Self::DuckDb | Self::Oracle | Self::MsSql => '"',
        }
    }

    /// Whether this dialect supports the LIMIT clause.
    #[must_use]
    pub fn supports_limit(self) -> bool {
        match self {
            Self::PostgreSql | Self::MySql | Self::Sqlite | Self::DuckDb => true,
            Self::MsSql | Self::Oracle => false,
        }
    }

    /// Whether this dialect supports OFFSET without FETCH.
    #[must_use]
    pub fn supports_offset(self) -> bool {
        match self {
            Self::PostgreSql | Self::MySql | Self::Sqlite | Self::DuckDb => true,
            Self::MsSql | Self::Oracle => false,
        }
    }

    /// Whether this dialect supports FETCH FIRST/NEXT syntax
    /// (SQL:2008 standard).
    #[must_use]
    pub fn supports_fetch(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::MsSql | Self::Oracle => true,
            Self::MySql | Self::Sqlite => false,
        }
    }

    /// Whether this dialect supports boolean literals
    /// (TRUE/FALSE).
    #[must_use]
    pub fn supports_boolean_literals(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::MySql => true,
            Self::Sqlite | Self::MsSql | Self::Oracle => false,
        }
    }

    /// Whether this dialect supports the `||` string
    /// concatenation operator.
    #[must_use]
    pub fn supports_concat_operator(self) -> bool {
        match self {
            Self::PostgreSql | Self::Sqlite | Self::DuckDb | Self::Oracle => true,
            Self::MySql | Self::MsSql => false,
        }
    }

    /// Whether this dialect uses `+` for string concatenation.
    #[must_use]
    pub fn uses_plus_concat(self) -> bool {
        matches!(self, Self::MsSql)
    }

    /// Whether this dialect supports ILIKE for case-insensitive
    /// pattern matching.
    #[must_use]
    pub fn supports_ilike(self) -> bool {
        matches!(self, Self::PostgreSql | Self::DuckDb)
    }

    /// Whether this dialect supports FULL OUTER JOIN.
    #[must_use]
    pub fn supports_full_outer_join(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::MsSql | Self::Oracle => true,
            Self::MySql | Self::Sqlite => false,
        }
    }

    /// Whether this dialect supports EXCEPT (vs MINUS in
    /// Oracle).
    #[must_use]
    pub fn supports_except(self) -> bool {
        !matches!(self, Self::Oracle)
    }

    /// Whether this dialect supports INTERSECT.
    #[must_use]
    pub fn supports_intersect(self) -> bool {
        true
    }

    /// Whether this dialect supports window functions.
    #[must_use]
    pub fn supports_window_functions(self) -> bool {
        // All supported dialects have window functions.
        true
    }

    /// Whether this dialect supports CTEs (WITH clause).
    #[must_use]
    pub fn supports_cte(self) -> bool {
        true
    }

    /// Whether this dialect supports recursive CTEs.
    #[must_use]
    pub fn supports_recursive_cte(self) -> bool {
        true
    }
}

/// Feature support level for a dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureSupport {
    /// Fully supported with native syntax.
    Native,
    /// Supported via translation/emulation.
    Emulated,
    /// Not supported; will produce a warning.
    Unsupported,
}

impl fmt::Display for FeatureSupport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native => write!(f, "native"),
            Self::Emulated => write!(f, "emulated"),
            Self::Unsupported => write!(f, "unsupported"),
        }
    }
}

/// SQL feature categories for the compatibility matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SqlFeature {
    /// LIMIT clause.
    Limit,
    /// OFFSET clause.
    Offset,
    /// FETCH FIRST/NEXT (SQL:2008).
    Fetch,
    /// Boolean literals (TRUE/FALSE).
    BooleanLiterals,
    /// String concatenation with `||`.
    ConcatOperator,
    /// ILIKE operator.
    Ilike,
    /// FULL OUTER JOIN.
    FullOuterJoin,
    /// EXCEPT set operation.
    Except,
    /// COALESCE function.
    Coalesce,
    /// NULLIF function.
    Nullif,
    /// CAST expression.
    Cast,
    /// String LENGTH function.
    Length,
    /// Substring extraction.
    Substring,
    /// Current timestamp.
    CurrentTimestamp,
    /// Date extraction.
    DateExtract,
}

impl fmt::Display for SqlFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Limit => "LIMIT",
            Self::Offset => "OFFSET",
            Self::Fetch => "FETCH",
            Self::BooleanLiterals => "Boolean Literals",
            Self::ConcatOperator => "|| Concat",
            Self::Ilike => "ILIKE",
            Self::FullOuterJoin => "FULL OUTER JOIN",
            Self::Except => "EXCEPT",
            Self::Coalesce => "COALESCE",
            Self::Nullif => "NULLIF",
            Self::Cast => "CAST",
            Self::Length => "LENGTH",
            Self::Substring => "SUBSTRING",
            Self::CurrentTimestamp => "CURRENT_TIMESTAMP",
            Self::DateExtract => "EXTRACT",
        };
        write!(f, "{name}")
    }
}

/// Returns the support level for a feature in a given dialect.
#[must_use]
pub fn feature_support(dialect: Dialect, feature: SqlFeature) -> FeatureSupport {
    match (dialect, feature) {
        // Unsupported: MySQL/SQLite lack FULL OUTER JOIN
        (Dialect::MySql | Dialect::Sqlite, SqlFeature::FullOuterJoin) => {
            FeatureSupport::Unsupported
        }

        // Emulated features
        (Dialect::MsSql | Dialect::Oracle, SqlFeature::Limit | SqlFeature::Offset)
        | (Dialect::MySql | Dialect::Sqlite, SqlFeature::Fetch)
        | (Dialect::Sqlite | Dialect::MsSql | Dialect::Oracle, SqlFeature::BooleanLiterals)
        | (Dialect::MySql | Dialect::MsSql, SqlFeature::ConcatOperator)
        | (
            Dialect::MySql | Dialect::Sqlite | Dialect::MsSql | Dialect::Oracle,
            SqlFeature::Ilike,
        )
        | (Dialect::Oracle, SqlFeature::Except)
        | (Dialect::MsSql, SqlFeature::Length)
        | (Dialect::Sqlite, SqlFeature::DateExtract) => FeatureSupport::Emulated,

        // Everything else is natively supported
        _ => FeatureSupport::Native,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialect_display() {
        assert_eq!(Dialect::PostgreSql.to_string(), "PostgreSQL");
        assert_eq!(Dialect::MySql.to_string(), "MySQL");
        assert_eq!(Dialect::Sqlite.to_string(), "SQLite");
        assert_eq!(Dialect::DuckDb.to_string(), "DuckDB");
        assert_eq!(Dialect::MsSql.to_string(), "MSSQL");
        assert_eq!(Dialect::Oracle.to_string(), "Oracle");
    }

    #[test]
    fn all_dialects_count() {
        assert_eq!(Dialect::ALL.len(), 6);
    }

    #[test]
    fn quote_styles() {
        assert_eq!(Dialect::PostgreSql.quote_style(), '"');
        assert_eq!(Dialect::MySql.quote_style(), '`');
        assert_eq!(Dialect::MsSql.quote_style(), '"');
    }

    #[test]
    fn limit_support() {
        assert!(Dialect::PostgreSql.supports_limit());
        assert!(Dialect::MySql.supports_limit());
        assert!(!Dialect::MsSql.supports_limit());
        assert!(!Dialect::Oracle.supports_limit());
    }

    #[test]
    fn fetch_support() {
        assert!(Dialect::PostgreSql.supports_fetch());
        assert!(Dialect::MsSql.supports_fetch());
        assert!(!Dialect::MySql.supports_fetch());
        assert!(!Dialect::Sqlite.supports_fetch());
    }

    #[test]
    fn concat_support() {
        assert!(Dialect::PostgreSql.supports_concat_operator());
        assert!(!Dialect::MySql.supports_concat_operator());
        assert!(Dialect::MsSql.uses_plus_concat());
    }

    #[test]
    fn feature_support_matrix() {
        assert_eq!(
            feature_support(Dialect::PostgreSql, SqlFeature::Limit,),
            FeatureSupport::Native
        );
        assert_eq!(
            feature_support(Dialect::MsSql, SqlFeature::Limit,),
            FeatureSupport::Emulated
        );
        assert_eq!(
            feature_support(Dialect::MySql, SqlFeature::FullOuterJoin,),
            FeatureSupport::Unsupported
        );
    }

    #[test]
    fn feature_support_display() {
        assert_eq!(FeatureSupport::Native.to_string(), "native");
        assert_eq!(FeatureSupport::Emulated.to_string(), "emulated");
        assert_eq!(FeatureSupport::Unsupported.to_string(), "unsupported");
    }

    #[test]
    fn sql_feature_display() {
        assert_eq!(SqlFeature::Limit.to_string(), "LIMIT");
        assert_eq!(SqlFeature::ConcatOperator.to_string(), "|| Concat");
    }
}
