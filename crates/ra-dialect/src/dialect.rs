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

    // Extended dialects (polyglot-backend only)
    /// Google `BigQuery`.
    #[cfg(feature = "polyglot-backend")]
    BigQuery,
    /// Snowflake.
    #[cfg(feature = "polyglot-backend")]
    Snowflake,
    /// Databricks.
    #[cfg(feature = "polyglot-backend")]
    Databricks,
    /// Amazon Redshift.
    #[cfg(feature = "polyglot-backend")]
    Redshift,
    /// `ClickHouse`.
    #[cfg(feature = "polyglot-backend")]
    ClickHouse,
    /// Trino (formerly `PrestoSQL`).
    #[cfg(feature = "polyglot-backend")]
    Trino,
    /// Presto.
    #[cfg(feature = "polyglot-backend")]
    Presto,
    /// Amazon Athena.
    #[cfg(feature = "polyglot-backend")]
    Athena,
    /// Apache Hive.
    #[cfg(feature = "polyglot-backend")]
    Hive,
    /// Apache Spark SQL.
    #[cfg(feature = "polyglot-backend")]
    Spark,
    /// Teradata.
    #[cfg(feature = "polyglot-backend")]
    Teradata,
    /// Exasol.
    #[cfg(feature = "polyglot-backend")]
    Exasol,
    /// Microsoft Fabric.
    #[cfg(feature = "polyglot-backend")]
    Fabric,
    /// Dremio.
    #[cfg(feature = "polyglot-backend")]
    Dremio,
    /// Apache Drill.
    #[cfg(feature = "polyglot-backend")]
    Drill,
    /// Apache Druid.
    #[cfg(feature = "polyglot-backend")]
    Druid,
    /// `CockroachDB`.
    #[cfg(feature = "polyglot-backend")]
    CockroachDb,
    /// Materialize.
    #[cfg(feature = "polyglot-backend")]
    Materialize,
    /// `RisingWave`.
    #[cfg(feature = "polyglot-backend")]
    RisingWave,
    /// `SingleStore` (formerly `MemSQL`).
    #[cfg(feature = "polyglot-backend")]
    SingleStore,
    /// `StarRocks`.
    #[cfg(feature = "polyglot-backend")]
    StarRocks,
    /// Apache Doris.
    #[cfg(feature = "polyglot-backend")]
    Doris,
    /// `TiDB`.
    #[cfg(feature = "polyglot-backend")]
    TiDb,
    /// Tableau.
    #[cfg(feature = "polyglot-backend")]
    Tableau,
    /// Apache Solr.
    #[cfg(feature = "polyglot-backend")]
    Solr,
    /// Dune Analytics.
    #[cfg(feature = "polyglot-backend")]
    Dune,
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
            #[cfg(feature = "polyglot-backend")]
            Self::BigQuery => "BigQuery",
            #[cfg(feature = "polyglot-backend")]
            Self::Snowflake => "Snowflake",
            #[cfg(feature = "polyglot-backend")]
            Self::Databricks => "Databricks",
            #[cfg(feature = "polyglot-backend")]
            Self::Redshift => "Redshift",
            #[cfg(feature = "polyglot-backend")]
            Self::ClickHouse => "ClickHouse",
            #[cfg(feature = "polyglot-backend")]
            Self::Trino => "Trino",
            #[cfg(feature = "polyglot-backend")]
            Self::Presto => "Presto",
            #[cfg(feature = "polyglot-backend")]
            Self::Athena => "Athena",
            #[cfg(feature = "polyglot-backend")]
            Self::Hive => "Hive",
            #[cfg(feature = "polyglot-backend")]
            Self::Spark => "Spark",
            #[cfg(feature = "polyglot-backend")]
            Self::Teradata => "Teradata",
            #[cfg(feature = "polyglot-backend")]
            Self::Exasol => "Exasol",
            #[cfg(feature = "polyglot-backend")]
            Self::Fabric => "Fabric",
            #[cfg(feature = "polyglot-backend")]
            Self::Dremio => "Dremio",
            #[cfg(feature = "polyglot-backend")]
            Self::Drill => "Drill",
            #[cfg(feature = "polyglot-backend")]
            Self::Druid => "Druid",
            #[cfg(feature = "polyglot-backend")]
            Self::CockroachDb => "CockroachDB",
            #[cfg(feature = "polyglot-backend")]
            Self::Materialize => "Materialize",
            #[cfg(feature = "polyglot-backend")]
            Self::RisingWave => "RisingWave",
            #[cfg(feature = "polyglot-backend")]
            Self::SingleStore => "SingleStore",
            #[cfg(feature = "polyglot-backend")]
            Self::StarRocks => "StarRocks",
            #[cfg(feature = "polyglot-backend")]
            Self::Doris => "Doris",
            #[cfg(feature = "polyglot-backend")]
            Self::TiDb => "TiDB",
            #[cfg(feature = "polyglot-backend")]
            Self::Tableau => "Tableau",
            #[cfg(feature = "polyglot-backend")]
            Self::Solr => "Solr",
            #[cfg(feature = "polyglot-backend")]
            Self::Dune => "Dune",
        };
        write!(f, "{name}")
    }
}

impl Dialect {
    /// All supported dialects (core set).
    pub const ALL_CORE: [Self; 6] = [
        Self::PostgreSql,
        Self::MySql,
        Self::Sqlite,
        Self::DuckDb,
        Self::MsSql,
        Self::Oracle,
    ];

    /// All supported dialects (including extended with polyglot-backend).
    #[cfg(feature = "polyglot-backend")]
    pub const ALL: [Self; 32] = [
        Self::PostgreSql,
        Self::MySql,
        Self::Sqlite,
        Self::DuckDb,
        Self::MsSql,
        Self::Oracle,
        Self::BigQuery,
        Self::Snowflake,
        Self::Databricks,
        Self::Redshift,
        Self::ClickHouse,
        Self::Trino,
        Self::Presto,
        Self::Athena,
        Self::Hive,
        Self::Spark,
        Self::Teradata,
        Self::Exasol,
        Self::Fabric,
        Self::Dremio,
        Self::Drill,
        Self::Druid,
        Self::CockroachDb,
        Self::Materialize,
        Self::RisingWave,
        Self::SingleStore,
        Self::StarRocks,
        Self::Doris,
        Self::TiDb,
        Self::Tableau,
        Self::Solr,
        Self::Dune,
    ];

    /// Returns the identifier quoting style for this dialect.
    #[must_use]
    pub fn quote_style(self) -> char {
        match self {
            Self::MySql => '`',
            Self::PostgreSql | Self::Sqlite | Self::DuckDb | Self::Oracle | Self::MsSql => '"',
            #[cfg(feature = "polyglot-backend")]
            _ => '"', // Most extended dialects use double quotes
        }
    }

    /// Whether this dialect supports the LIMIT clause.
    #[must_use]
    pub fn supports_limit(self) -> bool {
        match self {
            Self::PostgreSql | Self::MySql | Self::Sqlite | Self::DuckDb => true,
            Self::MsSql | Self::Oracle => false,
            #[cfg(feature = "polyglot-backend")]
            _ => true, // Most modern dialects support LIMIT
        }
    }

    /// Whether this dialect supports OFFSET without FETCH.
    #[must_use]
    pub fn supports_offset(self) -> bool {
        match self {
            Self::PostgreSql | Self::MySql | Self::Sqlite | Self::DuckDb => true,
            Self::MsSql | Self::Oracle => false,
            #[cfg(feature = "polyglot-backend")]
            _ => true, // Most modern dialects support OFFSET
        }
    }

    /// Whether this dialect supports FETCH FIRST/NEXT syntax
    /// (SQL:2008 standard).
    #[must_use]
    pub fn supports_fetch(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::MsSql | Self::Oracle => true,
            Self::MySql | Self::Sqlite => false,
            #[cfg(feature = "polyglot-backend")]
            _ => false, // Most dialects prefer LIMIT over FETCH
        }
    }

    /// Whether this dialect supports boolean literals
    /// (TRUE/FALSE).
    #[must_use]
    pub fn supports_boolean_literals(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::MySql => true,
            Self::Sqlite | Self::MsSql | Self::Oracle => false,
            #[cfg(feature = "polyglot-backend")]
            _ => true, // Most modern dialects support boolean literals
        }
    }

    /// Whether this dialect supports the `||` string
    /// concatenation operator.
    #[must_use]
    pub fn supports_concat_operator(self) -> bool {
        match self {
            Self::PostgreSql | Self::Sqlite | Self::DuckDb | Self::Oracle => true,
            Self::MySql | Self::MsSql => false,
            #[cfg(feature = "polyglot-backend")]
            _ => false, // Most dialects use CONCAT function
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
        match self {
            Self::PostgreSql | Self::DuckDb => true,
            #[cfg(feature = "polyglot-backend")]
            Self::Snowflake | Self::Redshift => true,
            _ => false,
        }
    }

    /// Whether this dialect supports FULL OUTER JOIN.
    #[must_use]
    pub fn supports_full_outer_join(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::MsSql | Self::Oracle => true,
            Self::MySql | Self::Sqlite => false,
            #[cfg(feature = "polyglot-backend")]
            _ => true, // Most modern databases support FULL OUTER JOIN
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

    /// Whether this dialect supports named windows
    /// (`WINDOW w AS (...)`).
    #[must_use]
    pub fn supports_named_windows(self) -> bool {
        match self {
            Self::PostgreSql | Self::MySql | Self::DuckDb => true,
            Self::Sqlite | Self::MsSql | Self::Oracle => false,
            #[cfg(feature = "polyglot-backend")]
            _ => false, // Conservative default for unknown dialects
        }
    }

    /// Whether this dialect supports the GROUPS frame mode
    /// in window functions.
    #[must_use]
    pub fn supports_window_frame_groups(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::Sqlite => true,
            Self::MySql | Self::MsSql | Self::Oracle => false,
            #[cfg(feature = "polyglot-backend")]
            _ => false, // Conservative default for unknown dialects
        }
    }

    /// Whether this dialect supports DISTINCT ON
    /// (`PostgreSQL` extension).
    #[must_use]
    pub fn supports_distinct_on(self) -> bool {
        matches!(self, Self::PostgreSql | Self::DuckDb)
    }

    /// Whether this dialect uses TOP N instead of LIMIT
    /// for row limiting.
    #[must_use]
    pub fn uses_top(self) -> bool {
        matches!(self, Self::MsSql)
    }

    /// Whether this dialect supports NULLS FIRST/LAST in
    /// ORDER BY.
    #[must_use]
    pub fn supports_nulls_first_last(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb | Self::Oracle => true,
            Self::MySql | Self::Sqlite | Self::MsSql => false,
            #[cfg(feature = "polyglot-backend")]
            _ => true, // Most modern databases support NULLS FIRST/LAST
        }
    }

    /// Whether this dialect supports the `::` double-colon
    /// cast syntax (e.g. `value::int`).
    #[must_use]
    pub fn supports_double_colon_cast(self) -> bool {
        matches!(self, Self::PostgreSql | Self::DuckDb)
    }

    /// Whether this dialect supports the RETURNING clause
    /// in INSERT/UPDATE/DELETE.
    #[must_use]
    pub fn supports_returning(self) -> bool {
        match self {
            Self::PostgreSql | Self::DuckDb => true,
            Self::MySql | Self::MsSql => false,
            Self::Sqlite => true, // since 3.35
            Self::Oracle => true, // since 12c
            #[cfg(feature = "polyglot-backend")]
            _ => false, // Conservative default for unknown dialects
        }
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
    /// Window functions (`ROW_NUMBER`, RANK, etc.).
    WindowFunctions,
    /// Common Table Expressions (WITH clause).
    Cte,
    /// Recursive CTEs (WITH RECURSIVE).
    RecursiveCte,
    /// SELECT DISTINCT.
    Distinct,
    /// HAVING clause.
    Having,
    /// Subqueries (scalar, EXISTS, IN).
    Subquery,
    /// ORDER BY clause.
    OrderBy,
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
            Self::WindowFunctions => "Window Functions",
            Self::Cte => "CTE (WITH)",
            Self::RecursiveCte => "RECURSIVE CTE",
            Self::Distinct => "DISTINCT",
            Self::Having => "HAVING",
            Self::Subquery => "Subqueries",
            Self::OrderBy => "ORDER BY",
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
        | (
            Dialect::Sqlite | Dialect::MsSql | Dialect::Oracle,
            SqlFeature::BooleanLiterals,
        )
        | (Dialect::MySql | Dialect::MsSql, SqlFeature::ConcatOperator)
        | (
            Dialect::MySql | Dialect::Sqlite | Dialect::MsSql | Dialect::Oracle,
            SqlFeature::Ilike,
        )
        | (Dialect::Oracle, SqlFeature::Except)
        | (Dialect::MsSql, SqlFeature::Length)
        | (
            Dialect::Sqlite,
            SqlFeature::DateExtract | SqlFeature::WindowFunctions,
        )
        | (Dialect::MySql, SqlFeature::WindowFunctions) => {
            FeatureSupport::Emulated
        }

        // Everything else is natively supported (CTE, Recursive CTE,
        // Distinct, Having, Subquery, OrderBy are universal across
        // all 6 supported dialects at their minimum versions)
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
        assert_eq!(Dialect::ALL.len(), 32);
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
        assert_eq!(
            SqlFeature::WindowFunctions.to_string(),
            "Window Functions"
        );
        assert_eq!(SqlFeature::Cte.to_string(), "CTE (WITH)");
        assert_eq!(SqlFeature::Distinct.to_string(), "DISTINCT");
    }

    #[test]
    fn window_function_support() {
        assert_eq!(
            feature_support(Dialect::PostgreSql, SqlFeature::WindowFunctions),
            FeatureSupport::Native
        );
        assert_eq!(
            feature_support(Dialect::MySql, SqlFeature::WindowFunctions),
            FeatureSupport::Emulated
        );
        assert_eq!(
            feature_support(Dialect::Sqlite, SqlFeature::WindowFunctions),
            FeatureSupport::Emulated
        );
    }

    #[test]
    fn cte_support() {
        for dialect in &Dialect::ALL {
            assert_eq!(
                feature_support(*dialect, SqlFeature::Cte),
                FeatureSupport::Native,
                "CTE should be native for {dialect}"
            );
            assert_eq!(
                feature_support(*dialect, SqlFeature::RecursiveCte),
                FeatureSupport::Native,
                "Recursive CTE should be native for {dialect}"
            );
        }
    }

    #[test]
    fn universal_features() {
        for dialect in &Dialect::ALL {
            assert_eq!(
                feature_support(*dialect, SqlFeature::Distinct),
                FeatureSupport::Native,
                "DISTINCT should be native for {dialect}"
            );
            assert_eq!(
                feature_support(*dialect, SqlFeature::Having),
                FeatureSupport::Native,
                "HAVING should be native for {dialect}"
            );
            assert_eq!(
                feature_support(*dialect, SqlFeature::Subquery),
                FeatureSupport::Native,
                "Subquery should be native for {dialect}"
            );
            assert_eq!(
                feature_support(*dialect, SqlFeature::OrderBy),
                FeatureSupport::Native,
                "ORDER BY should be native for {dialect}"
            );
        }
    }

    #[test]
    fn named_windows_support() {
        assert!(Dialect::PostgreSql.supports_named_windows());
        assert!(Dialect::MySql.supports_named_windows());
        assert!(!Dialect::MsSql.supports_named_windows());
        assert!(!Dialect::Oracle.supports_named_windows());
    }

    #[test]
    fn distinct_on_support() {
        assert!(Dialect::PostgreSql.supports_distinct_on());
        assert!(Dialect::DuckDb.supports_distinct_on());
        assert!(!Dialect::MySql.supports_distinct_on());
    }

    #[test]
    fn nulls_first_last_support() {
        assert!(Dialect::PostgreSql.supports_nulls_first_last());
        assert!(!Dialect::MySql.supports_nulls_first_last());
        assert!(!Dialect::MsSql.supports_nulls_first_last());
    }

    #[test]
    fn double_colon_cast_support() {
        assert!(
            Dialect::PostgreSql.supports_double_colon_cast()
        );
        assert!(
            Dialect::DuckDb.supports_double_colon_cast()
        );
        assert!(
            !Dialect::MySql.supports_double_colon_cast()
        );
        assert!(
            !Dialect::MsSql.supports_double_colon_cast()
        );
    }

    #[test]
    fn returning_support() {
        assert!(Dialect::PostgreSql.supports_returning());
        assert!(!Dialect::MySql.supports_returning());
        assert!(Dialect::Sqlite.supports_returning());
        assert!(Dialect::Oracle.supports_returning());
    }
}
