//! Database adapter trait for abstracting database connections.
//!
//! The isolation testing framework operates against different database
//! backends through this trait. Each backend (`SQLite`, `DuckDB`, `PostgreSQL`)
//! implements `DatabaseAdapter` to provide session-level operations.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Result of executing a SQL statement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueryResult {
    /// Column names in the result set.
    pub columns: Vec<String>,
    /// Rows, each containing string representations of values.
    pub rows: Vec<Vec<String>>,
    /// Number of rows affected (for DML statements).
    pub rows_affected: u64,
}

impl fmt::Display for QueryResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.columns.is_empty() {
            writeln!(f, "{}", self.columns.join(" | "))?;
            writeln!(
                f,
                "{}",
                self.columns
                    .iter()
                    .map(|c| "-".repeat(c.len()))
                    .collect::<Vec<_>>()
                    .join("-+-")
            )?;
            for row in &self.rows {
                writeln!(f, "{}", row.join(" | "))?;
            }
        }
        if self.rows_affected > 0 {
            writeln!(f, "({} rows affected)", self.rows_affected)?;
        }
        Ok(())
    }
}

/// Error from a database operation.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// The database returned an error for a SQL statement.
    #[error("query error: {message}")]
    QueryError {
        /// Error message from the database.
        message: String,
    },

    /// Connection to the database failed.
    #[error("connection error: {message}")]
    ConnectionError {
        /// Error message.
        message: String,
    },

    /// The operation timed out (e.g., waiting for a lock).
    #[error("timeout after {millis}ms")]
    Timeout {
        /// Milliseconds before timeout.
        millis: u64,
    },

    /// A deadlock was detected by the database.
    #[error("deadlock detected")]
    Deadlock,
}

/// Information about locks held by a session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockState {
    /// Locks currently held.
    pub held: Vec<LockDetail>,
    /// Locks currently waited on.
    pub waiting: Vec<LockDetail>,
}

/// Detail about a single lock.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockDetail {
    /// The table or resource being locked.
    pub resource: String,
    /// Lock mode (e.g., "shared", "exclusive").
    pub mode: String,
    /// Whether the lock has been granted.
    pub granted: bool,
}

/// Abstraction over a database connection for isolation testing.
///
/// Each session in an isolation test gets its own adapter instance.
/// Implementations manage the underlying connection and transaction
/// state.
pub trait DatabaseAdapter: fmt::Debug + Send {
    /// Execute a SQL statement and return the result.
    ///
    /// # Errors
    ///
    /// Returns `AdapterError` if the statement fails.
    fn execute(&mut self, sql: &str)
        -> Result<QueryResult, AdapterError>;

    /// Query the current lock state for this session.
    ///
    /// # Errors
    ///
    /// Returns `AdapterError` if the lock query fails.
    fn lock_state(&self) -> Result<LockState, AdapterError>;

    /// Check whether this session is currently blocked waiting for
    /// a lock.
    fn is_blocked(&self) -> bool;

    /// Return the database-specific name for the isolation level.
    fn isolation_level_name(&self) -> &'static str;

    /// Return the database backend name (e.g., "sqlite", "duckdb").
    fn backend_name(&self) -> &str;
}

/// An in-memory adapter for testing the framework itself without a
/// real database.
#[derive(Debug)]
pub struct MockAdapter {
    name: String,
    results: Vec<QueryResult>,
    call_index: usize,
    blocked: bool,
}

impl MockAdapter {
    /// Create a new mock adapter with a name and predetermined results.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        results: Vec<QueryResult>,
    ) -> Self {
        Self {
            name: name.into(),
            results,
            call_index: 0,
            blocked: false,
        }
    }

    /// Set the blocked state for testing.
    pub fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }
}

impl DatabaseAdapter for MockAdapter {
    fn execute(
        &mut self,
        _sql: &str,
    ) -> Result<QueryResult, AdapterError> {
        if self.call_index < self.results.len() {
            let result = self.results[self.call_index].clone();
            self.call_index += 1;
            Ok(result)
        } else {
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
            })
        }
    }

    fn lock_state(&self) -> Result<LockState, AdapterError> {
        Ok(LockState {
            held: vec![],
            waiting: vec![],
        })
    }

    fn is_blocked(&self) -> bool {
        self.blocked
    }

    fn isolation_level_name(&self) -> &'static str {
        "serializable"
    }

    fn backend_name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_result() -> QueryResult {
        QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
        }
    }

    fn result_with_data() -> QueryResult {
        QueryResult {
            columns: vec!["id".into(), "name".into()],
            rows: vec![
                vec!["1".into(), "alice".into()],
                vec!["2".into(), "bob".into()],
            ],
            rows_affected: 0,
        }
    }

    fn dml_result() -> QueryResult {
        QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 3,
        }
    }

    // -- QueryResult Display --

    #[test]
    fn query_result_display_with_columns() {
        let r = result_with_data();
        let s = format!("{r}");
        assert!(s.contains("id | name"));
        assert!(s.contains("1 | alice"));
        assert!(s.contains("2 | bob"));
    }

    #[test]
    fn query_result_display_empty() {
        let r = empty_result();
        let s = format!("{r}");
        assert!(s.is_empty());
    }

    #[test]
    fn query_result_display_dml() {
        let r = dml_result();
        let s = format!("{r}");
        assert!(s.contains("3 rows affected"));
    }

    // -- AdapterError Display --

    #[test]
    fn adapter_error_query() {
        let e = AdapterError::QueryError {
            message: "syntax error".into(),
        };
        assert_eq!(e.to_string(), "query error: syntax error");
    }

    #[test]
    fn adapter_error_connection() {
        let e = AdapterError::ConnectionError {
            message: "refused".into(),
        };
        assert_eq!(e.to_string(), "connection error: refused");
    }

    #[test]
    fn adapter_error_timeout() {
        let e = AdapterError::Timeout { millis: 5000 };
        assert_eq!(e.to_string(), "timeout after 5000ms");
    }

    #[test]
    fn adapter_error_deadlock() {
        let e = AdapterError::Deadlock;
        assert_eq!(e.to_string(), "deadlock detected");
    }

    // -- MockAdapter --

    #[test]
    fn mock_adapter_returns_results_in_order() {
        let r1 = result_with_data();
        let r2 = dml_result();
        let mut adapter = MockAdapter::new("test", vec![r1.clone(), r2.clone()]);

        let got1 = adapter.execute("SELECT 1").unwrap();
        assert_eq!(got1, r1);

        let got2 = adapter.execute("INSERT INTO t VALUES (1)").unwrap();
        assert_eq!(got2, r2);
    }

    #[test]
    fn mock_adapter_returns_empty_after_exhausting_results() {
        let mut adapter = MockAdapter::new("test", vec![result_with_data()]);
        let _ = adapter.execute("SELECT 1").unwrap();
        let got = adapter.execute("SELECT 2").unwrap();
        assert!(got.columns.is_empty());
        assert!(got.rows.is_empty());
    }

    #[test]
    fn mock_adapter_lock_state() {
        let adapter = MockAdapter::new("test", vec![]);
        let lock = adapter.lock_state().unwrap();
        assert!(lock.held.is_empty());
        assert!(lock.waiting.is_empty());
    }

    #[test]
    fn mock_adapter_blocked() {
        let mut adapter = MockAdapter::new("test", vec![]);
        assert!(!adapter.is_blocked());
        adapter.set_blocked(true);
        assert!(adapter.is_blocked());
        adapter.set_blocked(false);
        assert!(!adapter.is_blocked());
    }

    #[test]
    fn mock_adapter_isolation_level() {
        let adapter = MockAdapter::new("test", vec![]);
        assert_eq!(adapter.isolation_level_name(), "serializable");
    }

    #[test]
    fn mock_adapter_backend_name() {
        let adapter = MockAdapter::new("my_db", vec![]);
        assert_eq!(adapter.backend_name(), "my_db");
    }

    // -- LockState / LockDetail --

    #[test]
    fn lock_detail_equality() {
        let l1 = LockDetail {
            resource: "users".into(),
            mode: "exclusive".into(),
            granted: true,
        };
        let l2 = l1.clone();
        assert_eq!(l1, l2);
    }

    #[test]
    fn lock_state_default_behavior() {
        let state = LockState {
            held: vec![LockDetail {
                resource: "t".into(),
                mode: "shared".into(),
                granted: true,
            }],
            waiting: vec![],
        };
        assert_eq!(state.held.len(), 1);
        assert!(state.waiting.is_empty());
    }
}
