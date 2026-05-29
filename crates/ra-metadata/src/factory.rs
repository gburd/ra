//! Connection factory: create the right connector from a URL string.
//!
//! Detects the database backend from the URL scheme and creates
//! the appropriate connector. Wraps backend-specific connectors
//! in [`AnyConnector`] to provide a uniform mutable interface.

use std::path::Path;

use crate::connector::MetadataResult;
use crate::error::MetadataError;
use crate::explain::ExplainPlan;
use crate::schema::{DatabaseKind, SchemaInfo, TableStats};

#[cfg(feature = "mysql-support")]
use crate::mysql::MySqlConnector;
#[cfg(feature = "postgres-support")]
use crate::postgres::PostgresConnector;
#[cfg(feature = "sqlite-support")]
use crate::sqlite::SqliteConnector;

#[cfg(feature = "duckdb-support")]
use crate::duckdb::DuckDBConnector;
#[cfg(feature = "oracle-support")]
use crate::oracle::OracleConnector;
#[cfg(feature = "sqlserver-support")]
use crate::sqlserver::SqlServerConnector;

/// A backend-agnostic connector that delegates to the detected backend.
///
/// Unlike [`DatabaseConnector`](crate::connector::DatabaseConnector),
/// this owns the connection mutably and can perform all operations
/// without the `_mut` suffix workaround.
pub enum AnyConnector {
    /// `PostgreSQL` backend (requires `postgres-support` feature).
    #[cfg(feature = "postgres-support")]
    Postgres(PostgresConnector),
    /// `MySQL` backend (requires `mysql-support` feature).
    #[cfg(feature = "mysql-support")]
    MySql(MySqlConnector),
    /// `SQLite` backend (requires `sqlite-support` feature).
    #[cfg(feature = "sqlite-support")]
    SQLite(SqliteConnector),
    /// `DuckDB` backend (requires `duckdb-support` feature).
    #[cfg(feature = "duckdb-support")]
    DuckDB(DuckDBConnector),
    /// SQL Server backend (requires `sqlserver-support` feature).
    #[cfg(feature = "sqlserver-support")]
    SqlServer(Box<SqlServerConnector>),
    /// Oracle backend (requires `oracle-support` feature).
    #[cfg(feature = "oracle-support")]
    Oracle(OracleConnector),
}

impl AnyConnector {
    /// Return the database kind.
    pub fn kind(&self) -> DatabaseKind {
        match self {
            #[cfg(feature = "postgres-support")]
            Self::Postgres(_) => DatabaseKind::PostgreSQL,
            #[cfg(feature = "mysql-support")]
            Self::MySql(_) => DatabaseKind::MySQL,
            #[cfg(feature = "sqlite-support")]
            Self::SQLite(_) => DatabaseKind::SQLite,
            #[cfg(feature = "duckdb-support")]
            Self::DuckDB(_) => DatabaseKind::DuckDB,
            #[cfg(feature = "sqlserver-support")]
            Self::SqlServer(_) => DatabaseKind::SqlServer,
            #[cfg(feature = "oracle-support")]
            Self::Oracle(_) => DatabaseKind::Oracle,
        }
    }

    /// Gather full schema metadata.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if catalog queries fail.
    pub fn gather_schema(&mut self) -> MetadataResult<SchemaInfo> {
        match self {
            #[cfg(feature = "postgres-support")]
            Self::Postgres(c) => c.gather_schema_mut(),
            #[cfg(feature = "mysql-support")]
            Self::MySql(c) => c.gather_schema_mut(),
            #[cfg(feature = "sqlite-support")]
            Self::SQLite(c) => crate::connector::DatabaseConnector::gather_schema(c),
            #[cfg(feature = "duckdb-support")]
            Self::DuckDB(c) => c.gather_schema_mut(),
            #[cfg(feature = "sqlserver-support")]
            Self::SqlServer(c) => c.gather_schema_mut(),
            #[cfg(feature = "oracle-support")]
            Self::Oracle(c) => c.gather_schema_mut(),
        }
    }

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if statistics queries fail.
    pub fn gather_statistics(&mut self, table: &str) -> MetadataResult<TableStats> {
        match self {
            #[cfg(feature = "postgres-support")]
            Self::Postgres(c) => c.gather_statistics_mut(table),
            #[cfg(feature = "mysql-support")]
            Self::MySql(c) => c.gather_statistics_mut(table),
            #[cfg(feature = "sqlite-support")]
            Self::SQLite(c) => crate::connector::DatabaseConnector::gather_statistics(c, table),
            #[cfg(feature = "duckdb-support")]
            Self::DuckDB(c) => c.gather_statistics_mut(table),
            #[cfg(feature = "sqlserver-support")]
            Self::SqlServer(c) => c.gather_statistics_mut(table),
            #[cfg(feature = "oracle-support")]
            Self::Oracle(c) => c.gather_statistics_mut(table),
        }
    }

    /// Execute EXPLAIN on a query and parse the result.
    ///
    /// # Errors
    ///
    /// Returns `MetadataError` if the EXPLAIN query or
    /// parsing fails.
    pub fn explain_query(&mut self, sql: &str) -> MetadataResult<ExplainPlan> {
        match self {
            #[cfg(feature = "postgres-support")]
            Self::Postgres(c) => c.explain_query_mut(sql),
            #[cfg(feature = "mysql-support")]
            Self::MySql(c) => c.explain_query_mut(sql),
            #[cfg(feature = "sqlite-support")]
            Self::SQLite(c) => crate::connector::DatabaseConnector::explain_query(c, sql),
            #[cfg(feature = "duckdb-support")]
            Self::DuckDB(c) => c.explain_query_mut(sql),
            #[cfg(feature = "sqlserver-support")]
            Self::SqlServer(c) => c.explain_query_mut(sql),
            #[cfg(feature = "oracle-support")]
            Self::Oracle(c) => c.explain_query_mut(sql),
        }
    }
}

/// Detect the database backend from a connection URL and create
/// the appropriate connector.
///
/// Supported URL schemes:
/// - `postgresql://` or `postgres://` -- `PostgreSQL`
/// - `mysql://` -- `MySQL`
/// - `sqlite://` or a file path ending in
///   `.db`/`.sqlite`/`.sqlite3` -- `SQLite`
/// - `:memory:` -- `SQLite` in-memory
/// - `duckdb://` -- `DuckDB` (requires `duckdb-support` feature)
/// - `sqlserver://` or `mssql://` -- SQL Server (requires
///   `sqlserver-support` feature)
/// - `oracle://` -- Oracle (requires `oracle-support` feature)
///
/// Redact the password component of a database connection URL so
/// it is safe to embed in error messages and logs.
///
/// Connection strings routinely carry credentials
/// (`postgresql://user:secret@host/db`). Echoing them verbatim
/// into errors or logs discloses the password. This replaces the
/// password with `****`, preserving the rest of the URL for
/// diagnostics. URLs without credentials, bare file paths, and
/// `:memory:` are returned unchanged.
#[must_use]
pub fn redact_url(url: &str) -> String {
    // Locate the authority section after `scheme://`.
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let authority_start = scheme_end + 3;
    // The authority ends at the first `/`, `?`, or end of string.
    let authority_end = url[authority_start..]
        .find(['/', '?'])
        .map_or(url.len(), |i| authority_start + i);
    let authority = &url[authority_start..authority_end];

    // Credentials, if any, precede the last `@` in the authority.
    let Some(at) = authority.rfind('@') else {
        return url.to_string();
    };
    let creds = &authority[..at];
    // Mask everything after the first `:` (the password).
    let Some(colon) = creds.find(':') else {
        // userinfo without a password — nothing to redact.
        return url.to_string();
    };
    let user = &creds[..colon];
    format!(
        "{}{}:****@{}",
        &url[..authority_start],
        user,
        &url[authority_start + at + 1..],
    )
}

/// Detect the database backend from a connection URL and create
/// the appropriate connector.
///
/// # Errors
///
/// Returns `MetadataError::Connection` if the scheme is
/// unrecognized or the connection fails.
pub fn connect(url: &str) -> MetadataResult<AnyConnector> {
    #[cfg(feature = "postgres-support")]
    if url.starts_with("postgresql://") || url.starts_with("postgres://") {
        let connector = PostgresConnector::connect(url)?;
        return Ok(AnyConnector::Postgres(connector));
    }

    #[cfg(feature = "mysql-support")]
    if url.starts_with("mysql://") {
        let connector = MySqlConnector::connect(url)?;
        return Ok(AnyConnector::MySql(connector));
    }

    #[cfg(feature = "sqlite-support")]
    if url.starts_with("sqlite://") {
        let path = url.strip_prefix("sqlite://").unwrap_or(url);
        let connector = if path == ":memory:" {
            SqliteConnector::open_in_memory()?
        } else {
            SqliteConnector::connect(path)?
        };
        return Ok(AnyConnector::SQLite(connector));
    }

    #[cfg(feature = "sqlite-support")]
    if url == ":memory:" {
        let connector = SqliteConnector::open_in_memory()?;
        return Ok(AnyConnector::SQLite(connector));
    }

    #[cfg(feature = "duckdb-support")]
    if url.starts_with("duckdb://") {
        let path = url.strip_prefix("duckdb://").unwrap_or(url);
        let connector = if path == ":memory:" {
            DuckDBConnector::open_in_memory()?
        } else {
            DuckDBConnector::connect(path)?
        };
        return Ok(AnyConnector::DuckDB(connector));
    }

    #[cfg(feature = "duckdb-support")]
    if has_duckdb_extension(url) {
        let connector = DuckDBConnector::connect(url)?;
        return Ok(AnyConnector::DuckDB(connector));
    }

    #[cfg(feature = "sqlserver-support")]
    if url.starts_with("sqlserver://") || url.starts_with("mssql://") {
        let connector = SqlServerConnector::connect(url)?;
        return Ok(AnyConnector::SqlServer(Box::new(connector)));
    }

    #[cfg(feature = "oracle-support")]
    if url.starts_with("oracle://") {
        let connector = OracleConnector::connect(url)?;
        return Ok(AnyConnector::Oracle(connector));
    }

    #[cfg(feature = "sqlite-support")]
    if has_sqlite_extension(url) {
        let connector = SqliteConnector::connect(url)?;
        return Ok(AnyConnector::SQLite(connector));
    }

    Err(MetadataError::Connection {
        message: format!(
            "unrecognized database URL: {url}. \
             Expected postgresql://, mysql://, sqlite://, \
             duckdb://, sqlserver://, oracle://, \
             or a .db/.sqlite/.sqlite3 file path"
        ),
    })
}

/// Parse a connection URL and return the detected database kind
/// without actually connecting.
///
/// # Errors
///
/// Returns `MetadataError::Connection` if the scheme is
/// unrecognized.
pub fn detect_kind(url: &str) -> MetadataResult<DatabaseKind> {
    if url.starts_with("postgresql://") || url.starts_with("postgres://") {
        return Ok(DatabaseKind::PostgreSQL);
    }
    if url.starts_with("mysql://") {
        return Ok(DatabaseKind::MySQL);
    }
    if url.starts_with("sqlite://") || url == ":memory:" || has_sqlite_extension(url) {
        return Ok(DatabaseKind::SQLite);
    }
    if url.starts_with("duckdb://") {
        return Ok(DatabaseKind::DuckDB);
    }
    #[cfg(feature = "duckdb-support")]
    if has_duckdb_extension(url) {
        return Ok(DatabaseKind::DuckDB);
    }
    if url.starts_with("sqlserver://") || url.starts_with("mssql://") {
        return Ok(DatabaseKind::SqlServer);
    }
    if url.starts_with("oracle://") {
        return Ok(DatabaseKind::Oracle);
    }
    if url.starts_with("monetdb://") {
        return Ok(DatabaseKind::MonetDB);
    }
    Err(MetadataError::Connection {
        message: format!("unrecognized database URL scheme: {url}"),
    })
}

fn has_sqlite_extension(url: &str) -> bool {
    Path::new(url).extension().is_some_and(|ext| {
        ext.eq_ignore_ascii_case("db")
            || ext.eq_ignore_ascii_case("sqlite")
            || ext.eq_ignore_ascii_case("sqlite3")
    })
}

#[cfg(feature = "duckdb-support")]
fn has_duckdb_extension(url: &str) -> bool {
    Path::new(url)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("duckdb"))
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_masks_password() {
        assert_eq!(
            redact_url("postgresql://user:secret@localhost/db"),
            "postgresql://user:****@localhost/db"
        );
        assert_eq!(
            redact_url("mysql://admin:p@ss:word@host:3306/app?ssl=true"),
            "mysql://admin:****@host:3306/app?ssl=true"
        );
    }

    #[test]
    fn redact_url_leaves_credential_free_urls_unchanged() {
        assert_eq!(
            redact_url("postgresql://localhost/db"),
            "postgresql://localhost/db"
        );
        // userinfo without a password is not a secret.
        assert_eq!(
            redact_url("postgres://user@host/db"),
            "postgres://user@host/db"
        );
        // bare file paths and :memory: have no authority.
        assert_eq!(redact_url("/var/lib/db.sqlite"), "/var/lib/db.sqlite");
        assert_eq!(redact_url(":memory:"), ":memory:");
    }

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
        assert_eq!(detect_kind(":memory:").ok(), Some(DatabaseKind::SQLite));
        assert_eq!(detect_kind("/tmp/test.db").ok(), Some(DatabaseKind::SQLite));
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
    fn detect_duckdb_url() {
        assert_eq!(
            detect_kind("duckdb:///tmp/test.duckdb").ok(),
            Some(DatabaseKind::DuckDB)
        );
        assert_eq!(
            detect_kind("duckdb://:memory:").ok(),
            Some(DatabaseKind::DuckDB)
        );
    }

    #[test]
    fn detect_sqlserver_url() {
        assert_eq!(
            detect_kind("sqlserver://sa:pass@localhost/mydb").ok(),
            Some(DatabaseKind::SqlServer)
        );
        assert_eq!(
            detect_kind("mssql://sa:pass@localhost/mydb").ok(),
            Some(DatabaseKind::SqlServer)
        );
    }

    #[test]
    fn detect_oracle_url() {
        assert_eq!(
            detect_kind("oracle://scott:tiger@host:1521/ORCL").ok(),
            Some(DatabaseKind::Oracle)
        );
    }

    #[test]
    fn detect_monetdb_url() {
        assert_eq!(
            detect_kind("monetdb://monetdb:monetdb@localhost/demo").ok(),
            Some(DatabaseKind::MonetDB)
        );
    }

    #[test]
    fn detect_unknown_url() {
        assert!(detect_kind("redis://localhost").is_err());
    }

    #[test]
    fn connect_sqlite_memory() {
        let mut conn = connect(":memory:").expect("in-memory connect");
        assert_eq!(conn.kind(), DatabaseKind::SQLite);
        let schema = conn.gather_schema().expect("gather empty schema");
        assert!(schema.tables.is_empty());
    }

    #[test]
    fn connect_sqlite_url_memory() {
        let mut conn = connect("sqlite://:memory:").expect("sqlite url connect");
        assert_eq!(conn.kind(), DatabaseKind::SQLite);
        let schema = conn.gather_schema().expect("gather empty schema");
        assert!(schema.tables.is_empty());
    }

    #[test]
    fn connect_unknown_url() {
        let result = connect("redis://localhost");
        assert!(result.is_err());
    }

    #[test]
    fn any_connector_explain_sqlite() {
        let mut conn = connect(":memory:").expect("in-memory connect");

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
        let mut conn = connect(":memory:").expect("in-memory connect");

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

        let stats = conn.gather_statistics("users").expect("stats");
        assert_eq!(stats.table_name, "users");
        assert!(stats.row_count >= 2.0);
    }

    #[cfg(feature = "duckdb-support")]
    #[test]
    fn connect_duckdb_memory() {
        let mut conn = connect("duckdb://:memory:").expect("duckdb in-memory connect");
        assert_eq!(conn.kind(), DatabaseKind::DuckDB);
        let schema = conn.gather_schema().expect("gather empty schema");
        assert!(schema.tables.is_empty());
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
