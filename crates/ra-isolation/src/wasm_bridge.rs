//! Bridge adapters for connecting WASM database backends to the
//! isolation testing framework.
//!
//! This module provides `WasmBridgeAdapter`, a generic wrapper that
//! adapts any SQL-executing callback into an
//! [`ra_isolation::adapter::DatabaseAdapter`]. It also contains
//! database-specific lock monitoring queries for `SQLite` and `DuckDB`.
//!
//! # Usage
//!
//! The bridge is backend-agnostic: provide a closure that executes SQL
//! and returns a `Vec<Vec<String>>` result set, and the bridge handles
//! lock monitoring, blocking detection, and error translation.
//!
//! ```ignore
//! use ra_isolation::wasm_bridge::{WasmBridgeAdapter, Backend};
//!
//! let adapter = WasmBridgeAdapter::new(
//!     Backend::Sqlite,
//!     Box::new(|sql| { /* execute via wasm */ }),
//! );
//! ```

use crate::adapter::{
    AdapterError, DatabaseAdapter, LockDetail, LockState,
    QueryResult,
};

/// Identifies the database backend for lock monitoring queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `SQLite` WASM backend.
    Sqlite,
    /// `DuckDB` WASM backend.
    DuckDb,
}

impl Backend {
    fn name(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::DuckDb => "duckdb",
        }
    }

    fn isolation_level(self) -> &'static str {
        match self {
            Self::Sqlite => "serializable",
            Self::DuckDb => "snapshot isolation",
        }
    }
}

/// Raw result from executing SQL through the WASM bridge.
#[derive(Debug, Clone)]
pub struct RawResult {
    /// Column names.
    pub columns: Vec<String>,
    /// Row data as strings.
    pub rows: Vec<Vec<String>>,
    /// Rows affected count.
    pub rows_affected: u64,
}

/// Callback type for executing SQL through a WASM backend.
///
/// Takes a SQL string and returns either a `RawResult` or an error
/// message string.
pub type SqlExecutor =
    Box<dyn FnMut(&str) -> Result<RawResult, String> + Send>;

/// Bridges a WASM database backend to the isolation testing
/// `DatabaseAdapter` trait.
///
/// Wraps a SQL execution callback and adds lock monitoring and
/// blocking detection on top.
pub struct WasmBridgeAdapter {
    backend: Backend,
    executor: SqlExecutor,
    blocked: bool,
}

impl std::fmt::Debug for WasmBridgeAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmBridgeAdapter")
            .field("backend", &self.backend)
            .field("blocked", &self.blocked)
            .finish_non_exhaustive()
    }
}

impl WasmBridgeAdapter {
    /// Create a new bridge adapter for the given backend.
    #[must_use]
    pub fn new(backend: Backend, executor: SqlExecutor) -> Self {
        Self {
            backend,
            executor,
            blocked: false,
        }
    }
}

impl DatabaseAdapter for WasmBridgeAdapter {
    fn execute(
        &mut self,
        sql: &str,
    ) -> Result<QueryResult, AdapterError> {
        let raw = (self.executor)(sql).map_err(|msg| {
            if msg.to_lowercase().contains("deadlock") {
                self.blocked = false;
                return AdapterError::Deadlock;
            }
            if msg.to_lowercase().contains("timeout")
                || msg.to_lowercase().contains("lock")
            {
                self.blocked = true;
            }
            AdapterError::QueryError { message: msg }
        })?;
        self.blocked = false;
        Ok(raw_to_query_result(&raw))
    }

    fn lock_state(&self) -> Result<LockState, AdapterError> {
        // Lock state queries require mutation (to call executor),
        // but the trait takes &self. Return empty state here;
        // callers should use LockMonitor::refresh which calls
        // the adapter through a session.
        Ok(LockState {
            held: vec![],
            waiting: vec![],
        })
    }

    fn is_blocked(&self) -> bool {
        self.blocked
    }

    fn isolation_level_name(&self) -> &'static str {
        self.backend.isolation_level()
    }

    fn backend_name(&self) -> &str {
        self.backend.name()
    }
}

/// Convert a `RawResult` to an isolation `QueryResult`.
fn raw_to_query_result(raw: &RawResult) -> QueryResult {
    QueryResult {
        columns: raw.columns.clone(),
        rows: raw.rows.clone(),
        rows_affected: raw.rows_affected,
    }
}

/// SQL to query lock status on `SQLite`.
///
/// `SQLite` uses database-level locking. The `PRAGMA lock_status`
/// command returns the lock state of each attached database.
#[must_use]
pub fn sqlite_lock_query() -> &'static str {
    "PRAGMA lock_status"
}

/// Parse `SQLite` `PRAGMA lock_status` output into lock details.
///
/// Each row has format: `database | status` where status is one of
/// `unlocked`, `shared`, `reserved`, `pending`, `exclusive`.
#[must_use]
pub fn parse_sqlite_locks(
    rows: &[Vec<String>],
) -> Vec<LockDetail> {
    rows.iter()
        .filter_map(|row| {
            if row.len() < 2 {
                return None;
            }
            let status = row[1].to_lowercase();
            if status == "unlocked" {
                return None;
            }
            Some(LockDetail {
                resource: row[0].clone(),
                mode: row[1].clone(),
                granted: status != "pending",
            })
        })
        .collect()
}

/// SQL to query lock status on `DuckDB`.
///
/// `DuckDB` supports snapshot isolation and uses MVCC, so explicit
/// lock contention is rare. This query checks for active
/// transactions via internal metadata.
#[must_use]
pub fn duckdb_lock_query() -> &'static str {
    "SELECT * FROM duckdb_locks()"
}

/// Parse `DuckDB` lock query output into lock details.
///
/// `DuckDB`'s `duckdb_locks()` returns columns:
/// `database`, `schema`, `table`, `lock_type`, `granted`.
#[must_use]
pub fn parse_duckdb_locks(
    rows: &[Vec<String>],
) -> Vec<LockDetail> {
    rows.iter()
        .filter_map(|row| {
            if row.len() < 5 {
                return None;
            }
            let resource = if row[2].is_empty() {
                format!("{}.{}", row[0], row[1])
            } else {
                format!("{}.{}.{}", row[0], row[1], row[2])
            };
            let granted = row[4].to_lowercase() == "true"
                || row[4] == "t"
                || row[4] == "1";
            Some(LockDetail {
                resource,
                mode: row[3].clone(),
                granted,
            })
        })
        .collect()
}

/// Create a `LockState` by executing the appropriate lock query
/// for the given backend using a SQL executor.
///
/// # Errors
///
/// Returns an error string if the lock query fails.
pub fn query_lock_state(
    backend: Backend,
    executor: &mut dyn FnMut(&str) -> Result<RawResult, String>,
) -> Result<LockState, String> {
    let sql = match backend {
        Backend::Sqlite => sqlite_lock_query(),
        Backend::DuckDb => duckdb_lock_query(),
    };

    let raw = executor(sql)?;
    let details = match backend {
        Backend::Sqlite => parse_sqlite_locks(&raw.rows),
        Backend::DuckDb => parse_duckdb_locks(&raw.rows),
    };

    let held: Vec<LockDetail> =
        details.iter().filter(|d| d.granted).cloned().collect();
    let waiting: Vec<LockDetail> =
        details.iter().filter(|d| !d.granted).cloned().collect();

    Ok(LockState { held, waiting })
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn make_executor(
        result: RawResult,
    ) -> SqlExecutor {
        Box::new(move |_sql| Ok(result.clone()))
    }

    fn empty_result() -> RawResult {
        RawResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
        }
    }

    #[test]
    fn bridge_adapter_executes_sql() {
        let raw = RawResult {
            columns: vec!["id".into(), "val".into()],
            rows: vec![vec!["1".into(), "100".into()]],
            rows_affected: 0,
        };
        let mut adapter =
            WasmBridgeAdapter::new(Backend::Sqlite, make_executor(raw));

        let result = adapter.execute("SELECT * FROM t").unwrap();
        assert_eq!(result.columns, vec!["id", "val"]);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0], vec!["1", "100"]);
    }

    #[test]
    fn bridge_adapter_detects_deadlock() {
        let executor: SqlExecutor = Box::new(|_sql| {
            Err("ERROR: deadlock detected".into())
        });
        let mut adapter =
            WasmBridgeAdapter::new(Backend::DuckDb, executor);

        let result = adapter.execute("UPDATE t SET v = 1");
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), AdapterError::Deadlock),
            "expected deadlock error"
        );
    }

    #[test]
    fn bridge_adapter_tracks_blocking() {
        let executor: SqlExecutor = Box::new(|_sql| {
            Err("lock timeout exceeded".into())
        });
        let mut adapter =
            WasmBridgeAdapter::new(Backend::Sqlite, executor);

        let _ = adapter.execute("UPDATE t SET v = 1");
        assert!(adapter.is_blocked());
    }

    #[test]
    fn bridge_adapter_clears_blocking_on_success() {
        let mut call_count = 0u32;
        let executor: SqlExecutor = Box::new(move |_sql| {
            call_count += 1;
            if call_count == 1 {
                Err("lock timeout".into())
            } else {
                Ok(empty_result())
            }
        });
        let mut adapter =
            WasmBridgeAdapter::new(Backend::Sqlite, executor);

        let _ = adapter.execute("blocked");
        assert!(adapter.is_blocked());

        let _ = adapter.execute("ok");
        assert!(!adapter.is_blocked());
    }

    #[test]
    fn backend_names() {
        assert_eq!(Backend::Sqlite.name(), "sqlite");
        assert_eq!(Backend::DuckDb.name(), "duckdb");
    }

    #[test]
    fn parse_sqlite_locks_unlocked() {
        let rows = vec![vec!["main".into(), "unlocked".into()]];
        let locks = parse_sqlite_locks(&rows);
        assert!(locks.is_empty());
    }

    #[test]
    fn parse_sqlite_locks_shared() {
        let rows = vec![
            vec!["main".into(), "shared".into()],
            vec!["temp".into(), "unlocked".into()],
        ];
        let locks = parse_sqlite_locks(&rows);
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].resource, "main");
        assert_eq!(locks[0].mode, "shared");
        assert!(locks[0].granted);
    }

    #[test]
    fn parse_sqlite_locks_pending() {
        let rows = vec![vec!["main".into(), "pending".into()]];
        let locks = parse_sqlite_locks(&rows);
        assert_eq!(locks.len(), 1);
        assert!(!locks[0].granted);
    }

    #[test]
    fn parse_duckdb_locks_basic() {
        let rows = vec![vec![
            "mydb".into(),
            "public".into(),
            "orders".into(),
            "write".into(),
            "true".into(),
        ]];
        let locks = parse_duckdb_locks(&rows);
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].resource, "mydb.public.orders");
        assert_eq!(locks[0].mode, "write");
        assert!(locks[0].granted);
    }

    #[test]
    fn parse_duckdb_locks_not_granted() {
        let rows = vec![vec![
            "db".into(),
            "main".into(),
            "t".into(),
            "exclusive".into(),
            "false".into(),
        ]];
        let locks = parse_duckdb_locks(&rows);
        assert_eq!(locks.len(), 1);
        assert!(!locks[0].granted);
    }

    #[test]
    fn query_lock_state_sqlite() {
        let mut executor = |_sql: &str| -> Result<RawResult, String> {
            Ok(RawResult {
                columns: vec![
                    "database".into(),
                    "status".into(),
                ],
                rows: vec![
                    vec!["main".into(), "exclusive".into()],
                    vec!["temp".into(), "unlocked".into()],
                ],
                rows_affected: 0,
            })
        };

        let state =
            query_lock_state(Backend::Sqlite, &mut executor).unwrap();
        assert_eq!(state.held.len(), 1);
        assert_eq!(state.held[0].resource, "main");
        assert!(state.waiting.is_empty());
    }

    #[test]
    fn wasm_bridge_in_executor() {
        use crate::executor::TestExecutor;
        use crate::spec_parser;

        let input = r#"
session "s1"
{
    step "read"
    {
        SELECT * FROM t;
    }
}

permutation
{
    s1:read
}
"#;
        let spec = spec_parser::parse(input)
            .unwrap_or_else(|e| panic!("parse failed: {e}"));

        let factory: crate::executor::AdapterFactory =
            Box::new(|_name: &str| -> Box<dyn DatabaseAdapter> {
                let raw = RawResult {
                    columns: vec!["id".into()],
                    rows: vec![vec!["1".into()]],
                    rows_affected: 0,
                };
                Box::new(WasmBridgeAdapter::new(
                    Backend::Sqlite,
                    make_executor(raw),
                ))
            });

        let mut executor = TestExecutor::new(spec);
        let result = executor.run(&factory).unwrap_or_else(|e| {
            panic!("execution failed: {e}");
        });
        assert!(result.passed);
    }
}
