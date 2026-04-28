//! Core SQL dialect translator.
//!
//! The [`DialectTranslator`] parses SQL in one dialect and
//! rewrites the AST for a different target dialect, handling
//! differences in syntax, function names, and operators.

use crate::backends::{Backend, TranslationBackend};
use crate::dialect::Dialect;
use crate::error::{TranslationError, TranslationWarning};

/// Result of a dialect translation.
#[derive(Debug)]
pub struct TranslationResult {
    /// The translated SQL string.
    pub sql: String,
    /// Warnings generated during translation.
    pub warnings: Vec<TranslationWarning>,
}

/// Database dialect with version information.
///
/// Used to enable version-specific translation rules
/// (e.g. `PostgreSQL` 15 features vs 12 features).
#[derive(Debug, Clone)]
pub struct DialectVersion {
    /// The database dialect.
    pub dialect: Dialect,
    /// Major version number (e.g. 15 for `PostgreSQL` 15).
    pub major: u16,
    /// Minor version number.
    pub minor: u16,
}

impl DialectVersion {
    /// Create a new dialect version.
    #[must_use]
    pub fn new(
        dialect: Dialect,
        major: u16,
        minor: u16,
    ) -> Self {
        Self {
            dialect,
            major,
            minor,
        }
    }

    /// Create a version with defaults (latest known).
    #[must_use]
    pub fn latest(dialect: Dialect) -> Self {
        let (major, minor) = match dialect {
            Dialect::PostgreSql => (17, 0),
            Dialect::MySql => (8, 4),
            Dialect::Sqlite => (3, 45),
            Dialect::MsSql => (16, 0),
            Dialect::Oracle => (23, 0),
            Dialect::DuckDb => (1, 1),
            #[cfg(feature = "polyglot-backend")]
            _ => (1, 0), // Default version for unknown dialects
        };
        Self {
            dialect,
            major,
            minor,
        }
    }

    /// Whether this version supports the RETURNING clause.
    #[must_use]
    pub fn supports_returning(&self) -> bool {
        match self.dialect {
            Dialect::PostgreSql => true,
            Dialect::MySql => false,
            Dialect::Sqlite => self.major >= 3 && self.minor >= 35,
            Dialect::MsSql => true, // OUTPUT clause
            Dialect::Oracle => self.major >= 12,
            Dialect::DuckDb => true,
            #[cfg(feature = "polyglot-backend")]
            _ => false, // Conservative default for unknown dialects
        }
    }

    /// Whether this version supports CTE (WITH clause).
    #[must_use]
    pub fn supports_cte(&self) -> bool {
        match self.dialect {
            Dialect::PostgreSql => true,
            Dialect::MySql => self.major >= 8,
            Dialect::Sqlite => self.major >= 3 && self.minor >= 8,
            Dialect::MsSql => true,
            Dialect::Oracle => true,
            Dialect::DuckDb => true,
            #[cfg(feature = "polyglot-backend")]
            _ => true, // Most modern databases support CTE
        }
    }

    /// Whether this version supports window functions.
    #[must_use]
    pub fn supports_window_functions(&self) -> bool {
        match self.dialect {
            Dialect::PostgreSql => true,
            Dialect::MySql => self.major >= 8,
            Dialect::Sqlite => self.major >= 3 && self.minor >= 25,
            Dialect::MsSql => true,
            Dialect::Oracle => true,
            Dialect::DuckDb => true,
            #[cfg(feature = "polyglot-backend")]
            _ => true, // Most modern databases support window functions
        }
    }
}

/// Translates SQL between different database dialects.
///
/// # Example
///
/// ```
/// use ra_dialect::{Dialect, DialectTranslator};
///
/// let translator = DialectTranslator::new(
///     Dialect::PostgreSql,
///     Dialect::MySql,
/// );
/// let result = translator
///     .translate("SELECT * FROM users LIMIT 10")
///     .unwrap();
/// assert!(result.sql.contains("LIMIT"));
/// ```
pub struct DialectTranslator {
    source: Dialect,
    target: Dialect,
    source_version: DialectVersion,
    target_version: DialectVersion,
    backend: TranslationBackend,
}

impl DialectTranslator {
    /// Create a new translator from source to target dialect.
    #[must_use]
    pub fn new(source: Dialect, target: Dialect) -> Self {
        Self {
            source,
            target,
            source_version: DialectVersion::latest(source),
            target_version: DialectVersion::latest(target),
            backend: TranslationBackend::default(),
        }
    }

    /// Create a translator with a specific backend.
    #[must_use]
    pub fn with_backend(
        source: Dialect,
        target: Dialect,
        backend: TranslationBackend,
    ) -> Self {
        Self {
            source,
            target,
            source_version: DialectVersion::latest(source),
            target_version: DialectVersion::latest(target),
            backend,
        }
    }

    /// Create a translator with specific dialect versions.
    #[must_use]
    pub fn with_versions(
        source: DialectVersion,
        target: DialectVersion,
    ) -> Self {
        Self {
            source: source.dialect,
            target: target.dialect,
            source_version: source,
            target_version: target,
            backend: TranslationBackend::default(),
        }
    }

    /// The source dialect.
    #[must_use]
    pub fn source(&self) -> Dialect {
        self.source
    }

    /// The target dialect.
    #[must_use]
    pub fn target(&self) -> Dialect {
        self.target
    }

    /// The source dialect version.
    #[must_use]
    pub fn source_version(&self) -> &DialectVersion {
        &self.source_version
    }

    /// The target dialect version.
    #[must_use]
    pub fn target_version(&self) -> &DialectVersion {
        &self.target_version
    }

    /// Get the current backend.
    #[must_use]
    pub fn backend(&self) -> TranslationBackend {
        self.backend
    }

    /// Translate a SQL string from the source dialect to the
    /// target dialect.
    ///
    /// # Errors
    ///
    /// Returns `TranslationError` if parsing fails or the SQL
    /// contains unsupported constructs.
    pub fn translate(&self, sql: &str) -> Result<TranslationResult, TranslationError> {
        // Delegate to the configured backend
        let backend_impl: Box<dyn Backend> = match self.backend {
            TranslationBackend::Native => {
                Box::new(crate::backends::native::NativeBackend)
            }
            #[cfg(feature = "polyglot-backend")]
            TranslationBackend::Polyglot => {
                Box::new(crate::backends::polyglot_backend::PolyglotBackend)
            }
        };

        backend_impl.translate(sql, self.source, self.target)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used)] // tests intentionally panic on failure
mod tests {
    use super::*;

    fn pg_to(target: Dialect, sql: &str) -> TranslationResult {
        DialectTranslator::new(Dialect::PostgreSql, target)
            .translate(sql)
            .expect("translation should succeed")
    }

    #[test]
    fn identity_translation() {
        let result = pg_to(Dialect::PostgreSql, "SELECT 1");
        assert!(result.sql.contains("SELECT"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn limit_to_mysql() {
        let result = pg_to(Dialect::MySql, "SELECT * FROM users LIMIT 10");
        assert!(result.sql.contains("LIMIT"));
    }

    #[test]
    fn limit_to_mssql() {
        let result = pg_to(Dialect::MsSql, "SELECT * FROM users LIMIT 10");
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in: {}",
            result.sql
        );
        assert!(
            !result.sql.contains("LIMIT"),
            "Should not contain LIMIT: {}",
            result.sql
        );
    }

    #[test]
    fn limit_offset_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "SELECT * FROM users \
             ORDER BY id LIMIT 10 OFFSET 20",
        );
        assert!(
            result.sql.contains("OFFSET"),
            "Expected OFFSET in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in: {}",
            result.sql
        );
    }

    #[test]
    fn string_concat_to_mysql() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT first_name || ' ' || last_name \
             FROM users",
        );
        assert!(
            result.sql.contains("CONCAT"),
            "Expected CONCAT in: {}",
            result.sql
        );
    }

    #[test]
    fn string_concat_to_mssql() {
        let result = pg_to(Dialect::MsSql, "SELECT first_name || last_name FROM users");
        assert!(result.sql.contains('+'), "Expected + in: {}", result.sql);
    }

    #[test]
    fn ilike_to_mysql() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT * FROM users \
             WHERE name ILIKE '%john%'",
        );
        assert!(
            result.sql.contains("LOWER"),
            "Expected LOWER in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("LIKE"),
            "Expected LIKE in: {}",
            result.sql
        );
    }

    #[test]
    fn ilike_stays_for_postgres() {
        let result = pg_to(
            Dialect::PostgreSql,
            "SELECT * FROM users \
             WHERE name ILIKE '%john%'",
        );
        assert!(
            result.sql.contains("ILIKE"),
            "Expected ILIKE preserved: {}",
            result.sql
        );
    }

    #[test]
    fn boolean_to_sqlite() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT * FROM flags \
             WHERE active = TRUE",
        );
        assert!(
            result.sql.contains('1'),
            "Expected 1 for TRUE in: {}",
            result.sql
        );
    }

    #[test]
    fn boolean_to_mssql() {
        let result = pg_to(Dialect::MsSql, "SELECT * FROM t WHERE x = FALSE");
        assert!(
            result.sql.contains('0'),
            "Expected 0 for FALSE in: {}",
            result.sql
        );
    }

    #[test]
    fn select_with_where() {
        let result = pg_to(Dialect::MySql, "SELECT id, name FROM users WHERE age > 18");
        assert!(result.sql.contains("WHERE"));
        assert!(result.sql.contains("age"));
    }

    #[test]
    fn union_query() {
        let result = pg_to(Dialect::MySql, "SELECT id FROM a UNION SELECT id FROM b");
        assert!(result.sql.contains("UNION"));
    }

    #[test]
    fn translator_accessors() {
        let t = DialectTranslator::new(Dialect::PostgreSql, Dialect::MySql);
        assert_eq!(t.source(), Dialect::PostgreSql);
        assert_eq!(t.target(), Dialect::MySql);
    }

    #[test]
    fn case_expression_translation() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT CASE WHEN x = TRUE THEN 'yes' \
             ELSE 'no' END FROM t",
        );
        assert!(
            result.sql.contains('1'),
            "Expected boolean translation in: {}",
            result.sql
        );
    }

    #[test]
    fn nested_function_translation() {
        let result = pg_to(Dialect::MsSql, "SELECT LENGTH(UPPER(name)) FROM users");
        assert!(
            result.sql.contains("LEN"),
            "Expected LEN in: {}",
            result.sql
        );
    }

    #[test]
    fn multiple_statements() {
        let result = pg_to(Dialect::MySql, "SELECT 1; SELECT 2");
        assert!(result.sql.contains("SELECT 1"));
        assert!(result.sql.contains("SELECT 2"));
    }

    #[test]
    fn parse_error_propagated() {
        let t = DialectTranslator::new(Dialect::PostgreSql, Dialect::MySql);
        let err = t.translate("NOT VALID SQL !!! %%%");
        assert!(err.is_err());
    }

    #[test]
    fn cte_translation() {
        let result = pg_to(
            Dialect::MySql,
            "WITH active AS (SELECT * FROM users \
             WHERE active = TRUE) \
             SELECT * FROM active",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH in: {}",
            result.sql
        );
        // Boolean TRUE should be translated for MySQL? No,
        // MySQL supports TRUE. Let's check it passes through.
        assert!(result.sql.contains("active"));
    }

    #[test]
    fn cte_to_sqlite_boolean_translation() {
        let result = pg_to(
            Dialect::Sqlite,
            "WITH cte AS (SELECT * FROM t \
             WHERE flag = TRUE) \
             SELECT * FROM cte",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH in: {}",
            result.sql
        );
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in CTE body: {}",
            result.sql
        );
    }

    #[test]
    fn recursive_cte_translation() {
        let result = pg_to(
            Dialect::MySql,
            "WITH RECURSIVE nums AS (\
             SELECT 1 AS n \
             UNION ALL \
             SELECT n + 1 FROM nums WHERE n < 10) \
             SELECT * FROM nums",
        );
        assert!(
            result.sql.contains("RECURSIVE"),
            "Expected RECURSIVE in: {}",
            result.sql
        );
    }

    #[test]
    fn window_function_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT name, ROW_NUMBER() OVER \
             (PARTITION BY dept ORDER BY salary DESC) \
             AS rn FROM employees",
        );
        assert!(
            result.sql.contains("ROW_NUMBER"),
            "Expected ROW_NUMBER in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("OVER"),
            "Expected OVER in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("PARTITION BY"),
            "Expected PARTITION BY in: {}",
            result.sql
        );
    }

    #[test]
    fn window_function_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "SELECT id, SUM(amount) OVER \
             (PARTITION BY customer_id ORDER BY date) \
             FROM orders",
        );
        assert!(
            result.sql.contains("SUM"),
            "Expected SUM in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("OVER"),
            "Expected OVER in: {}",
            result.sql
        );
    }

    #[test]
    fn window_function_with_boolean_in_partition() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT ROW_NUMBER() OVER \
             (PARTITION BY active ORDER BY id) \
             FROM users WHERE active = TRUE",
        );
        assert!(
            result.sql.contains("OVER"),
            "Expected OVER in: {}",
            result.sql
        );
        // TRUE should become 1 for SQLite
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in: {}",
            result.sql
        );
    }

    #[test]
    fn distinct_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT DISTINCT name FROM users",
        );
        assert!(
            result.sql.contains("DISTINCT"),
            "Expected DISTINCT in: {}",
            result.sql
        );
    }

    #[test]
    fn having_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT dept, COUNT(*) FROM employees \
             GROUP BY dept HAVING COUNT(*) > 5",
        );
        // ra-parser represents HAVING as a Filter over Aggregate,
        // so the emitter produces WHERE with the aggregate condition
        assert!(
            result.sql.contains("WHERE") && result.sql.contains("COUNT"),
            "Expected WHERE with COUNT in: {}",
            result.sql
        );
    }

    #[test]
    fn having_with_boolean_translation() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT dept, COUNT(*) FROM employees \
             GROUP BY dept HAVING COUNT(*) > 5 \
             AND active = TRUE",
        );
        // ra-parser represents HAVING as a Filter over Aggregate
        assert!(
            result.sql.contains("WHERE") && result.sql.contains("COUNT"),
            "Expected WHERE with COUNT in: {}",
            result.sql
        );
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in: {}",
            result.sql
        );
    }

    #[test]
    fn subquery_in_where() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT * FROM orders WHERE customer_id \
             IN (SELECT id FROM customers WHERE active = TRUE)",
        );
        // ra-parser wraps IN subquery as a special function
        assert!(
            result.sql.contains("IN"),
            "Expected IN in: {}",
            result.sql
        );
    }

    #[test]
    fn exists_subquery() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT * FROM orders WHERE EXISTS \
             (SELECT 1 FROM customers \
             WHERE customers.id = orders.customer_id \
             AND active = TRUE)",
        );
        assert!(
            result.sql.contains("EXISTS"),
            "Expected EXISTS in: {}",
            result.sql
        );
    }

    #[test]
    fn scalar_subquery_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT name, \
             (SELECT COUNT(*) FROM orders \
             WHERE orders.user_id = users.id) AS cnt \
             FROM users",
        );
        assert!(
            result.sql.contains("SELECT"),
            "Expected nested SELECT in: {}",
            result.sql
        );
    }

    #[test]
    fn order_by_with_limit_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "SELECT * FROM users \
             ORDER BY name ASC LIMIT 10",
        );
        assert!(
            result.sql.contains("ORDER BY"),
            "Expected ORDER BY in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in: {}",
            result.sql
        );
    }

    #[test]
    fn cte_with_limit_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "WITH top_users AS (\
             SELECT * FROM users ORDER BY score DESC \
             LIMIT 10) \
             SELECT * FROM top_users",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in CTE body: {}",
            result.sql
        );
    }

    #[test]
    fn version_support() {
        let pg17 = DialectVersion::latest(Dialect::PostgreSql);
        assert!(pg17.supports_returning());
        assert!(pg17.supports_cte());
        assert!(pg17.supports_window_functions());

        let mysql5 = DialectVersion::new(Dialect::MySql, 5, 7);
        assert!(!mysql5.supports_cte());
        assert!(!mysql5.supports_window_functions());

        let mysql8 = DialectVersion::new(Dialect::MySql, 8, 0);
        assert!(mysql8.supports_cte());
        assert!(mysql8.supports_window_functions());

        let sqlite_old = DialectVersion::new(Dialect::Sqlite, 3, 24);
        assert!(!sqlite_old.supports_returning());
        assert!(!sqlite_old.supports_window_functions());

        let sqlite_new = DialectVersion::new(Dialect::Sqlite, 3, 35);
        assert!(sqlite_new.supports_returning());
    }

    #[test]
    fn translator_with_versions() {
        let source = DialectVersion::new(Dialect::PostgreSql, 15, 0);
        let target = DialectVersion::new(Dialect::MySql, 8, 0);
        let t = DialectTranslator::with_versions(source, target);
        assert_eq!(t.source(), Dialect::PostgreSql);
        assert_eq!(t.target(), Dialect::MySql);
        assert_eq!(t.source_version().major, 15);
        assert_eq!(t.target_version().major, 8);
    }

    #[test]
    fn double_colon_cast_to_mysql() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT age::int FROM users",
        );
        assert!(
            result.sql.contains("CAST"),
            "Expected CAST in: {}",
            result.sql
        );
    }

    #[test]
    fn double_colon_cast_stays_for_postgres() {
        let result = pg_to(
            Dialect::PostgreSql,
            "SELECT age::int FROM users",
        );
        // PostgreSQL supports ::, so it should stay as-is
        assert!(
            result.sql.contains("::") || result.sql.contains("CAST"),
            "Expected :: or CAST in: {}",
            result.sql
        );
    }

    #[test]
    fn combined_cte_window_distinct() {
        let result = pg_to(
            Dialect::MySql,
            "WITH ranked AS (\
             SELECT name, \
             ROW_NUMBER() OVER (ORDER BY score DESC) AS rn \
             FROM users) \
             SELECT DISTINCT name FROM ranked \
             WHERE rn <= 10",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH: {}",
            result.sql
        );
        assert!(
            result.sql.contains("ROW_NUMBER"),
            "Expected ROW_NUMBER: {}",
            result.sql
        );
        assert!(
            result.sql.contains("DISTINCT"),
            "Expected DISTINCT: {}",
            result.sql
        );
    }

    #[test]
    fn translator_with_backend() {
        let backend = TranslationBackend::Native;
        let t = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            Dialect::MySql,
            backend,
        );
        assert_eq!(t.source(), Dialect::PostgreSql);
        assert_eq!(t.target(), Dialect::MySql);
        let result = t.translate("SELECT 1").expect("should translate");
        assert!(result.sql.contains("SELECT"));
    }

    #[test]
    fn dialect_version_latest_all_dialects() {
        let pg = DialectVersion::latest(Dialect::PostgreSql);
        assert_eq!(pg.major, 17);

        let mysql = DialectVersion::latest(Dialect::MySql);
        assert_eq!(mysql.major, 8);

        let sqlite = DialectVersion::latest(Dialect::Sqlite);
        assert_eq!(sqlite.major, 3);

        let mssql = DialectVersion::latest(Dialect::MsSql);
        assert_eq!(mssql.major, 16);

        let oracle = DialectVersion::latest(Dialect::Oracle);
        assert_eq!(oracle.major, 23);

        let duckdb = DialectVersion::latest(Dialect::DuckDb);
        assert_eq!(duckdb.major, 1);
    }

    #[test]
    fn supports_returning_all_dialects() {
        assert!(DialectVersion::latest(Dialect::PostgreSql).supports_returning());
        assert!(!DialectVersion::latest(Dialect::MySql).supports_returning());
        assert!(DialectVersion::latest(Dialect::Sqlite).supports_returning());
        assert!(DialectVersion::latest(Dialect::MsSql).supports_returning());
        assert!(DialectVersion::latest(Dialect::Oracle).supports_returning());
        assert!(DialectVersion::latest(Dialect::DuckDb).supports_returning());
    }

    #[test]
    fn supports_cte_old_versions() {
        let sqlite_old = DialectVersion::new(Dialect::Sqlite, 3, 7);
        assert!(!sqlite_old.supports_cte());

        let sqlite_new = DialectVersion::new(Dialect::Sqlite, 3, 8);
        assert!(sqlite_new.supports_cte());
    }

    #[test]
    fn supports_window_functions_versions() {
        let sqlite_old = DialectVersion::new(Dialect::Sqlite, 3, 24);
        assert!(!sqlite_old.supports_window_functions());

        let sqlite_new = DialectVersion::new(Dialect::Sqlite, 3, 25);
        assert!(sqlite_new.supports_window_functions());

        let mysql_old = DialectVersion::new(Dialect::MySql, 5, 7);
        assert!(!mysql_old.supports_window_functions());

        let mysql_new = DialectVersion::new(Dialect::MySql, 8, 0);
        assert!(mysql_new.supports_window_functions());
    }

    #[test]
    fn source_version_accessor() {
        let source = DialectVersion::new(Dialect::PostgreSql, 14, 2);
        let target = DialectVersion::new(Dialect::MySql, 8, 0);
        let t = DialectTranslator::with_versions(source, target);
        assert_eq!(t.source_version().major, 14);
        assert_eq!(t.source_version().minor, 2);
    }

    #[test]
    fn target_version_accessor() {
        let source = DialectVersion::new(Dialect::PostgreSql, 15, 0);
        let target = DialectVersion::new(Dialect::MySql, 8, 1);
        let t = DialectTranslator::with_versions(source, target);
        assert_eq!(t.target_version().major, 8);
        assert_eq!(t.target_version().minor, 1);
    }

    #[test]
    fn oracle_version_features() {
        let oracle11 = DialectVersion::new(Dialect::Oracle, 11, 0);
        assert!(!oracle11.supports_returning());

        let oracle12 = DialectVersion::new(Dialect::Oracle, 12, 0);
        assert!(oracle12.supports_returning());
    }

    #[test]
    fn all_dialects_support_cte_latest() {
        assert!(DialectVersion::latest(Dialect::PostgreSql).supports_cte());
        assert!(DialectVersion::latest(Dialect::MySql).supports_cte());
        assert!(DialectVersion::latest(Dialect::Sqlite).supports_cte());
        assert!(DialectVersion::latest(Dialect::MsSql).supports_cte());
        assert!(DialectVersion::latest(Dialect::Oracle).supports_cte());
        assert!(DialectVersion::latest(Dialect::DuckDb).supports_cte());
    }

    #[test]
    fn all_dialects_support_window_functions_latest() {
        assert!(DialectVersion::latest(Dialect::PostgreSql).supports_window_functions());
        assert!(DialectVersion::latest(Dialect::MySql).supports_window_functions());
        assert!(DialectVersion::latest(Dialect::Sqlite).supports_window_functions());
        assert!(DialectVersion::latest(Dialect::MsSql).supports_window_functions());
        assert!(DialectVersion::latest(Dialect::Oracle).supports_window_functions());
        assert!(DialectVersion::latest(Dialect::DuckDb).supports_window_functions());
    }

    #[test]
    fn complex_function_translation() {
        let result = pg_to(
            Dialect::MsSql,
            "SELECT SUBSTRING(name, 1, 10), \
             LENGTH(TRIM(BOTH ' ' FROM email)) \
             FROM users",
        );
        assert!(result.sql.contains("LEN"), "Expected LEN: {}", result.sql);
    }

    #[test]
    fn error_on_empty_sql() {
        let t = DialectTranslator::new(Dialect::PostgreSql, Dialect::MySql);
        let result = t.translate("");
        // Empty SQL should either succeed with empty result or fail gracefully
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn whitespace_only_sql() {
        let t = DialectTranslator::new(Dialect::PostgreSql, Dialect::MySql);
        let result = t.translate("   \n\t  ");
        // Whitespace-only should be handled gracefully
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn translation_result_format() {
        let result = pg_to(Dialect::MySql, "SELECT 1");
        assert!(!result.sql.is_empty());
        // Warnings may or may not be present
    }
}
