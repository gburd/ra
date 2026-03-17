//! Database adapter trait for unified WASM database access.
//!
//! Provides a common interface over `SQLite` WASM and `DuckDB` WASM,
//! allowing the rest of the system to execute SQL queries without
//! caring which engine backs a given connection.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::errors::Result;
use crate::storage::StorageBackend;

/// Identifies which WASM database engine to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DatabaseEngine {
    /// `SQLite` compiled to WASM via `@sqlite.org/sqlite-wasm`.
    Sqlite,
    /// `DuckDB` compiled to WASM via `@duckdb/duckdb-wasm`.
    DuckDb,
}

impl fmt::Display for DatabaseEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite => write!(f, "SQLite"),
            Self::DuckDb => write!(f, "DuckDB"),
        }
    }
}

/// Configuration for opening a database connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Which engine to use.
    pub engine: DatabaseEngine,
    /// Optional database name (for persistence).
    pub database_name: Option<String>,
    /// Storage backend for persistence.
    pub storage: StorageBackend,
    /// Whether this connection is read-only.
    pub read_only: bool,
}

impl ConnectionConfig {
    /// Create a config for an in-memory `SQLite` database.
    #[must_use]
    pub fn sqlite_memory() -> Self {
        Self {
            engine: DatabaseEngine::Sqlite,
            database_name: None,
            storage: StorageBackend::Memory,
            read_only: false,
        }
    }

    /// Create a config for an in-memory `DuckDB` database.
    #[must_use]
    pub fn duckdb_memory() -> Self {
        Self {
            engine: DatabaseEngine::DuckDb,
            database_name: None,
            storage: StorageBackend::Memory,
            read_only: false,
        }
    }

    /// Create a config for a persistent `SQLite` database.
    #[must_use]
    pub fn sqlite_persistent(name: impl Into<String>) -> Self {
        Self {
            engine: DatabaseEngine::Sqlite,
            database_name: Some(name.into()),
            storage: StorageBackend::Opfs,
            read_only: false,
        }
    }

    /// Create a config for a persistent `DuckDB` database.
    #[must_use]
    pub fn duckdb_persistent(name: impl Into<String>) -> Self {
        Self {
            engine: DatabaseEngine::DuckDb,
            database_name: Some(name.into()),
            storage: StorageBackend::Opfs,
            read_only: false,
        }
    }
}

/// A single column value in a query result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// SQL NULL.
    Null,
    /// Integer value.
    Integer(i64),
    /// Floating-point value.
    Float(f64),
    /// Text value.
    Text(String),
    /// Binary blob.
    Blob(Vec<u8>),
    /// Boolean value.
    Boolean(bool),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Integer(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Text(s) => write!(f, "{s}"),
            Self::Blob(b) => write!(f, "<blob({} bytes)>", b.len()),
            Self::Boolean(b) => write!(f, "{b}"),
        }
    }
}

/// Metadata about a column in a result set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Database-reported type name (may vary by engine).
    pub type_name: Option<String>,
}

/// The result of executing a SQL query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Column metadata.
    pub columns: Vec<ColumnInfo>,
    /// Row data, each row being a vector of values aligned
    /// with `columns`.
    pub rows: Vec<Vec<Value>>,
    /// Number of rows affected (for INSERT/UPDATE/DELETE).
    pub rows_affected: u64,
}

impl QueryResult {
    /// Number of rows returned.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Number of columns in the result.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Return true if the result contains no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A handle to a single database connection.
///
/// Implementations wrap either `sqlite-wasm` or `duckdb-wasm`
/// connections behind a uniform API. Each method maps to
/// a call into JavaScript via `wasm-bindgen`.
pub trait DatabaseAdapter: fmt::Debug {
    /// Execute a SQL statement that modifies data (DDL/DML).
    ///
    /// # Errors
    ///
    /// Returns an error if the SQL is invalid or execution fails.
    fn execute(&self, sql: &str) -> Result<QueryResult>;

    /// Execute a SQL query that returns rows.
    ///
    /// # Errors
    ///
    /// Returns an error if the SQL is invalid or execution fails.
    fn query(&self, sql: &str) -> Result<QueryResult>;

    /// Execute a SQL statement with positional parameters.
    ///
    /// Parameters are passed as JSON-serialized values.
    ///
    /// # Errors
    ///
    /// Returns an error on failure.
    fn execute_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult>;

    /// Execute a SQL query with positional parameters.
    ///
    /// # Errors
    ///
    /// Returns an error on failure.
    fn query_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult>;

    /// Close the connection, releasing resources.
    ///
    /// # Errors
    ///
    /// Returns an error if close fails.
    fn close(&self) -> Result<()>;

    /// Return which engine this adapter wraps.
    fn engine(&self) -> DatabaseEngine;

    /// Return true if the connection is still open.
    fn is_open(&self) -> bool;
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn engine_display() {
        assert_eq!(DatabaseEngine::Sqlite.to_string(), "SQLite");
        assert_eq!(DatabaseEngine::DuckDb.to_string(), "DuckDB");
    }

    #[test]
    fn engine_equality() {
        assert_eq!(DatabaseEngine::Sqlite, DatabaseEngine::Sqlite);
        assert_ne!(DatabaseEngine::Sqlite, DatabaseEngine::DuckDb);
    }

    #[test]
    fn engine_serde_roundtrip() {
        for engine in [DatabaseEngine::Sqlite, DatabaseEngine::DuckDb] {
            let json = serde_json::to_string(&engine).expect("serialize");
            let out: DatabaseEngine = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(engine, out);
        }
    }

    #[test]
    fn config_sqlite_memory() {
        let c = ConnectionConfig::sqlite_memory();
        assert_eq!(c.engine, DatabaseEngine::Sqlite);
        assert_eq!(c.storage, StorageBackend::Memory);
        assert!(c.database_name.is_none());
        assert!(!c.read_only);
    }

    #[test]
    fn config_duckdb_memory() {
        let c = ConnectionConfig::duckdb_memory();
        assert_eq!(c.engine, DatabaseEngine::DuckDb);
        assert_eq!(c.storage, StorageBackend::Memory);
        assert!(c.database_name.is_none());
    }

    #[test]
    fn config_sqlite_persistent() {
        let c = ConnectionConfig::sqlite_persistent("test.db");
        assert_eq!(c.engine, DatabaseEngine::Sqlite);
        assert_eq!(c.storage, StorageBackend::Opfs);
        assert_eq!(c.database_name.as_deref(), Some("test.db"));
    }

    #[test]
    fn config_duckdb_persistent() {
        let c = ConnectionConfig::duckdb_persistent("a.db");
        assert_eq!(c.engine, DatabaseEngine::DuckDb);
        assert_eq!(c.storage, StorageBackend::Opfs);
        assert_eq!(c.database_name.as_deref(), Some("a.db"));
    }

    #[test]
    fn config_serde_roundtrip() {
        let c = ConnectionConfig::sqlite_persistent("mydb");
        let json = serde_json::to_string(&c).expect("serialize");
        let out: ConnectionConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(out.engine, c.engine);
        assert_eq!(out.database_name, c.database_name);
        assert_eq!(out.storage, c.storage);
        assert_eq!(out.read_only, c.read_only);
    }

    #[test]
    fn value_display() {
        assert_eq!(Value::Null.to_string(), "NULL");
        assert_eq!(Value::Integer(42).to_string(), "42");
        assert_eq!(Value::Float(3.14).to_string(), "3.14");
        assert_eq!(Value::Text("hello".into()).to_string(), "hello");
        assert_eq!(Value::Blob(vec![1, 2, 3]).to_string(), "<blob(3 bytes)>");
        assert_eq!(Value::Boolean(true).to_string(), "true");
    }

    #[test]
    fn value_serde_roundtrip() {
        let values = vec![
            Value::Null,
            Value::Integer(123),
            Value::Float(2.718),
            Value::Text("test".into()),
            Value::Blob(vec![0xDE, 0xAD]),
            Value::Boolean(false),
        ];
        let json = serde_json::to_string(&values).expect("serialize");
        let out: Vec<Value> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(values, out);
    }

    #[test]
    fn query_result_empty() {
        let r = QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
        };
        assert_eq!(r.row_count(), 0);
        assert_eq!(r.column_count(), 0);
        assert!(r.is_empty());
    }

    #[test]
    fn query_result_with_data() {
        let r = QueryResult {
            columns: vec![
                ColumnInfo {
                    name: "id".into(),
                    type_name: Some("INTEGER".into()),
                },
                ColumnInfo {
                    name: "name".into(),
                    type_name: Some("TEXT".into()),
                },
            ],
            rows: vec![
                vec![Value::Integer(1), Value::Text("Alice".into())],
                vec![Value::Integer(2), Value::Text("Bob".into())],
            ],
            rows_affected: 0,
        };
        assert_eq!(r.row_count(), 2);
        assert_eq!(r.column_count(), 2);
        assert!(!r.is_empty());
    }

    #[test]
    fn query_result_serde_roundtrip() {
        let r = QueryResult {
            columns: vec![ColumnInfo {
                name: "count".into(),
                type_name: None,
            }],
            rows: vec![vec![Value::Integer(42)]],
            rows_affected: 0,
        };
        let json = serde_json::to_string(&r).expect("serialize");
        let out: QueryResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(out.row_count(), 1);
        assert_eq!(out.columns[0].name, "count");
    }
}
