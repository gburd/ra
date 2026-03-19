//! Connection factory: create the right connector from a URL string.
//!
//! Detects the database backend from the URL scheme and creates
//! the appropriate connector. Wraps backend-specific connectors
//! in [`AnyConnector`] to provide a uniform mutable interface.

use std::path::Path;

use crate::connector::MetadataResult;
use crate::error::MetadataError;
use crate::explain::ExplainPlan;
use crate::mysql::MySqlConnector;
use crate::postgres::PostgresConnector;
use crate::schema::{DatabaseKind, SchemaInfo, TableStats};
use crate::sqlite::SqliteConnector;

/// A backend-agnostic connector that delegates to the detected backend.
///
/// Unlike [`DatabaseConnector`](crate::connector::DatabaseConnector),
/// this owns the connection mutably and can perform all operations
/// without the `_mut` suffix workaround.
pub enum AnyConnector {
    /// `PostgreSQL` backend.
    Postgres(PostgresConnector),
    /// `MySQL` backend.
    MySql(MySqlConnector),
    /// `SQLite` backend.
    SQLite(SqliteConnector),
}

impl AnyConnector {
    /// Return the database kind.
    pub fn kind(&self) -> DatabaseKind {
        match self {
            Self::Postgres(_) => DatabaseKind::PostgreSQL,
            Self::MySql(_) => DatabaseKind::MySQL,
            Self::SQLite(_) => DatabaseKind::SQLite,
        }
    }

    /// Gather full schema metadata.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if catalog queries fail.
    pub fn gather_schema(&mut self) -> MetadataResult<SchemaInfo> {
        match self {
            Self::Postgres(c) => c.gather_schema_mut(),
            Self::MySql(c) => c.gather_schema_mut(),
            Self::SQLite(c) => {
                crate::connector::DatabaseConnector::gather_schema(c)
            }
        }
    }

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if statistics queries fail.
    pub fn gather_statistics(
        &mut self,
        table: &str,
    ) -> MetadataResult<TableStats> {
        match self {
            Self::Postgres(c) => c.gather_statistics_mut(table),
            Self::MySql(c) => c.gather_statistics_mut(table),
            Self::SQLite(c) => {
                crate::connector::DatabaseConnector::gather_statistics(
                    c, table,
                )
            }
        }
    }

    /// Execute EXPLAIN on a query and parse the result.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if the EXPLAIN query or parsing fails.
    pub fn explain_query(
        &mut self,
        sql: &str,
    ) -> MetadataResult<ExplainPlan> {
        match self {
            Self::Postgres(c) => c.explain_query_mut(sql),
            Self::MySql(c) => c.explain_query_mut(sql),
            Self::SQLite(c) => {
                crate::connector::DatabaseConnector::explain_query(
                    c, sql,
                )
            }
        }
    }
}

/// Detect the database backend from a connection URL and create
/// the appropriate connector.
///
/// Supported URL schemes:
/// - `postgresql://` or `postgres://` -- `PostgreSQL`
/// - `mysql://` -- `MySQL`
/// - `sqlite://` or a file path ending in `.db`/`.sqlite`/`.sqlite3` -- `SQLite`
/// - `:memory:` -- `SQLite` in-memory
///
/// # Errors
///
/// Returns `MetadataError::Connection` if the scheme is unrecognized
/// or the connection fails.
pub fn connect(url: &str) -> MetadataResult<AnyConnector> {
    if url.starts_with("postgresql://") || url.starts_with("postgres://") {
        let connector = PostgresConnector::connect(url)?;
        return Ok(AnyConnector::Postgres(connector));
    }

    if url.starts_with("mysql://") {
        let connector = MySqlConnector::connect(url)?;
        return Ok(AnyConnector::MySql(connector));
    }

    if url.starts_with("sqlite://") {
        let path = url.strip_prefix("sqlite://").unwrap_or(url);
        let connector = if path == ":memory:" {
            SqliteConnector::open_in_memory()?
        } else {
            SqliteConnector::connect(path)?
        };
        return Ok(AnyConnector::SQLite(connector));
    }

    if url == ":memory:" {
        let connector = SqliteConnector::open_in_memory()?;
        return Ok(AnyConnector::SQLite(connector));
    }

    if has_sqlite_extension(url) {
        let connector = SqliteConnector::connect(url)?;
        return Ok(AnyConnector::SQLite(connector));
    }

    Err(MetadataError::Connection {
        message: format!(
            "unrecognized database URL: {url}. \
             Expected postgresql://, mysql://, sqlite://, \
             or a .db/.sqlite/.sqlite3 file path"
        ),
    })
}

/// Parse a connection URL and return the detected database kind
/// without actually connecting.
///
/// # Errors
///
/// Returns `MetadataError::Connection` if the scheme is unrecognized.
pub fn detect_kind(url: &str) -> MetadataResult<DatabaseKind> {
    if url.starts_with("postgresql://") || url.starts_with("postgres://") {
        return Ok(DatabaseKind::PostgreSQL);
    }
    if url.starts_with("mysql://") {
        return Ok(DatabaseKind::MySQL);
    }
    if url.starts_with("sqlite://")
        || url == ":memory:"
        || has_sqlite_extension(url)
    {
        return Ok(DatabaseKind::SQLite);
    }
    Err(MetadataError::Connection {
        message: format!(
            "unrecognized database URL scheme: {url}"
        ),
    })
}

fn has_sqlite_extension(url: &str) -> bool {
    Path::new(url)
        .extension()
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("db")
                || ext.eq_ignore_ascii_case("sqlite")
                || ext.eq_ignore_ascii_case("sqlite3")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_postgres_url() {
        assert_eq!(
            detect_kind("postgresql://user:pass@localhost/db").ok(),
            Some(DatabaseKind::PostgreSQL)
        );
        assert_eq!(
            detect_kind("postgres://localhost/db").ok(),
            Some(DatabaseKind::PostgreSQL)
        );
    }

    #[test]
    fn detect_mysql_url() {
        assert_eq!(
            detect_kind("mysql://user:pass@localhost/db").ok(),
            Some(DatabaseKind::MySQL)
        );
    }

    #[test]
    fn detect_sqlite_url() {
        assert_eq!(
            detect_kind("sqlite:///tmp/test.db").ok(),
            Some(DatabaseKind::SQLite)
        );
        assert_eq!(
            detect_kind(":memory:").ok(),
            Some(DatabaseKind::SQLite)
        );
        assert_eq!(
            detect_kind("/tmp/test.db").ok(),
            Some(DatabaseKind::SQLite)
        );
        assert_eq!(
            detect_kind("/tmp/test.sqlite").ok(),
            Some(DatabaseKind::SQLite)
        );
        assert_eq!(
            detect_kind("/tmp/test.sqlite3").ok(),
            Some(DatabaseKind::SQLite)
        );
    }

    #[test]
    fn detect_unknown_url() {
        assert!(detect_kind("redis://localhost").is_err());
    }

    #[test]
    fn connect_sqlite_memory() {
        let mut conn =
            connect(":memory:").expect("in-memory connect");
        assert_eq!(conn.kind(), DatabaseKind::SQLite);
        let schema = conn
            .gather_schema()
            .expect("gather empty schema");
        assert!(schema.tables.is_empty());
    }

    #[test]
    fn connect_sqlite_url_memory() {
        let mut conn =
            connect("sqlite://:memory:").expect("sqlite url connect");
        assert_eq!(conn.kind(), DatabaseKind::SQLite);
        let schema = conn
            .gather_schema()
            .expect("gather empty schema");
        assert!(schema.tables.is_empty());
    }

    #[test]
    fn connect_unknown_url() {
        let result = connect("redis://localhost");
        assert!(result.is_err());
    }

    #[test]
    fn any_connector_explain_sqlite() {
        let mut conn =
            connect(":memory:").expect("in-memory connect");

        if let AnyConnector::SQLite(ref c) = conn {
            c.connection()
                .execute_batch(
                    "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
                     INSERT INTO t VALUES (1, 'a');
                     ANALYZE;",
                )
                .expect("setup");
        }

        let plan = conn
            .explain_query("SELECT * FROM t WHERE id = 1")
            .expect("explain");
        assert!(plan.root.node_count() >= 1);
    }

    #[test]
    fn any_connector_gather_statistics_sqlite() {
        let mut conn =
            connect(":memory:").expect("in-memory connect");

        if let AnyConnector::SQLite(ref c) = conn {
            c.connection()
                .execute_batch(
                    "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);
                     INSERT INTO users VALUES (1, 'Alice');
                     INSERT INTO users VALUES (2, 'Bob');
                     ANALYZE;",
                )
                .expect("setup");
        }

        let stats = conn
            .gather_statistics("users")
            .expect("stats");
        assert_eq!(stats.table_name, "users");
        assert!(stats.row_count >= 2.0);
    }

    #[test]
    fn has_sqlite_extension_positive() {
        assert!(has_sqlite_extension("/tmp/test.db"));
        assert!(has_sqlite_extension("/tmp/test.sqlite"));
        assert!(has_sqlite_extension("/tmp/test.sqlite3"));
        assert!(has_sqlite_extension("test.DB"));
        assert!(has_sqlite_extension("test.SQLite"));
    }

    #[test]
    fn has_sqlite_extension_negative() {
        assert!(!has_sqlite_extension("postgresql://localhost"));
        assert!(!has_sqlite_extension("test.txt"));
        assert!(!has_sqlite_extension(":memory:"));
    }
}
