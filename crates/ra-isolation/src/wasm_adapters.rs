//! Direct `ra-wasm` adapter integration for isolation testing.
//!
//! Provides convenience functions to wrap `ra_wasm::SqliteAdapter`
//! and `ra_wasm::DuckDbAdapter` as `ra_isolation::DatabaseAdapter`
//! implementations using the [`wasm_bridge`](crate::wasm_bridge)
//! callback bridge.
//!
//! Requires the `wasm` feature flag.

use crate::adapter::DatabaseAdapter;
use crate::executor::AdapterFactory;
use crate::wasm_bridge::{Backend, RawResult, WasmBridgeAdapter};

/// Factory function that opens a `ra_wasm` adapter for a session.
pub type WasmAdapterOpener = Box<dyn Fn(&str) -> Result<Box<dyn ra_wasm::DatabaseAdapter>, String>>;

/// Wrap a `ra_wasm::DatabaseAdapter` as an isolation
/// `DatabaseAdapter` using the callback bridge.
///
/// Converts between `ra_wasm`'s typed `Value` results and
/// `ra_isolation`'s string-based results.
#[must_use]
pub fn wrap_wasm_adapter(
    inner: Box<dyn ra_wasm::DatabaseAdapter>,
    backend: Backend,
) -> Box<dyn DatabaseAdapter> {
    let executor = wasm_adapter_to_executor(inner);
    Box::new(WasmBridgeAdapter::new(backend, executor))
}

/// Create an `AdapterFactory` that opens `SQLite` WASM
/// connections for each isolation test session.
///
/// The `open_fn` is called once per session to produce a
/// `ra_wasm::DatabaseAdapter`. If opening fails, the resulting
/// adapter returns errors on every SQL call.
#[must_use]
pub fn sqlite_wasm_factory(open_fn: WasmAdapterOpener) -> AdapterFactory {
    Box::new(move |session_name: &str| -> Box<dyn DatabaseAdapter> {
        match open_fn(session_name) {
            Ok(inner) => wrap_wasm_adapter(inner, Backend::Sqlite),
            Err(msg) => {
                let executor =
                    Box::new(move |_sql: &str| -> Result<RawResult, String> { Err(msg.clone()) });
                Box::new(WasmBridgeAdapter::new(Backend::Sqlite, executor))
            }
        }
    })
}

/// Create an `AdapterFactory` that opens `DuckDB` WASM
/// connections for each isolation test session.
///
/// See [`sqlite_wasm_factory`] for details.
#[must_use]
pub fn duckdb_wasm_factory(open_fn: WasmAdapterOpener) -> AdapterFactory {
    Box::new(move |session_name: &str| -> Box<dyn DatabaseAdapter> {
        match open_fn(session_name) {
            Ok(inner) => wrap_wasm_adapter(inner, Backend::DuckDb),
            Err(msg) => {
                let executor =
                    Box::new(move |_sql: &str| -> Result<RawResult, String> { Err(msg.clone()) });
                Box::new(WasmBridgeAdapter::new(Backend::DuckDb, executor))
            }
        }
    })
}

/// Wrapper to make a non-Send `ra_wasm::DatabaseAdapter` usable
/// in the `Send`-requiring `SqlExecutor` type.
///
/// # Safety
///
/// WASM runs single-threaded, so the adapter is never accessed
/// from multiple threads. This wrapper is only used behind the
/// `wasm` feature gate, which implies a WASM target.
struct SendWrapper(Box<dyn ra_wasm::DatabaseAdapter>);

// SAFETY: WASM execution is single-threaded. The wrapped adapter
// is never moved between threads.
unsafe impl Send for SendWrapper {}

impl SendWrapper {
    fn execute(&self, sql: &str) -> Result<ra_wasm::QueryResult, String> {
        self.0.execute(sql).map_err(|e| e.to_string())
    }
}

/// Convert a `ra_wasm::DatabaseAdapter` into a `SqlExecutor`
/// callback.
///
/// The returned closure executes SQL through the WASM adapter
/// and converts the typed results to string format.
fn wasm_adapter_to_executor(
    adapter: Box<dyn ra_wasm::DatabaseAdapter>,
) -> crate::wasm_bridge::SqlExecutor {
    let wrapper = SendWrapper(adapter);

    Box::new(move |sql: &str| -> Result<RawResult, String> {
        let result = wrapper.execute(sql)?;

        let columns = result.columns.iter().map(|c| c.name.clone()).collect();

        let rows = result
            .rows
            .iter()
            .map(|row| row.iter().map(std::string::ToString::to_string).collect())
            .collect();

        Ok(RawResult {
            columns,
            rows,
            rows_affected: result.rows_affected,
        })
    })
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Mock `ra_wasm::DatabaseAdapter` for testing without WASM.
    #[derive(Debug)]
    struct MockWasmAdapter {
        result: Option<ra_wasm::QueryResult>,
        error: Option<String>,
    }

    impl MockWasmAdapter {
        fn with_result(result: ra_wasm::QueryResult) -> Self {
            Self {
                result: Some(result),
                error: None,
            }
        }

        fn succeeding() -> Self {
            Self::with_result(ra_wasm::QueryResult {
                columns: vec![ra_wasm::ColumnInfo {
                    name: "ok".into(),
                    type_name: None,
                }],
                rows: vec![vec![ra_wasm::Value::Integer(1)]],
                rows_affected: 0,
            })
        }
    }

    impl ra_wasm::DatabaseAdapter for MockWasmAdapter {
        fn execute(&self, _sql: &str) -> ra_wasm::errors::Result<ra_wasm::QueryResult> {
            if let Some(ref msg) = self.error {
                return Err(ra_wasm::WasmDbError::Query(msg.clone()));
            }
            Ok(self.result.clone().unwrap_or_else(|| ra_wasm::QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: 0,
            }))
        }

        fn query(&self, sql: &str) -> ra_wasm::errors::Result<ra_wasm::QueryResult> {
            self.execute(sql)
        }

        fn execute_with_params(
            &self,
            sql: &str,
            _params: &[ra_wasm::Value],
        ) -> ra_wasm::errors::Result<ra_wasm::QueryResult> {
            self.execute(sql)
        }

        fn query_with_params(
            &self,
            sql: &str,
            _params: &[ra_wasm::Value],
        ) -> ra_wasm::errors::Result<ra_wasm::QueryResult> {
            self.execute(sql)
        }

        fn close(&self) -> ra_wasm::errors::Result<()> {
            Ok(())
        }

        fn engine(&self) -> ra_wasm::DatabaseEngine {
            ra_wasm::DatabaseEngine::Sqlite
        }

        fn is_open(&self) -> bool {
            true
        }
    }

    #[test]
    fn wrap_wasm_adapter_converts_types() {
        let mock = MockWasmAdapter::with_result(ra_wasm::QueryResult {
            columns: vec![
                ra_wasm::ColumnInfo {
                    name: "id".into(),
                    type_name: Some("INT".into()),
                },
                ra_wasm::ColumnInfo {
                    name: "val".into(),
                    type_name: None,
                },
            ],
            rows: vec![vec![
                ra_wasm::Value::Integer(42),
                ra_wasm::Value::Text("hello".into()),
            ]],
            rows_affected: 0,
        });

        let mut adapter = wrap_wasm_adapter(Box::new(mock), Backend::Sqlite);
        let result = adapter.execute("SELECT 42").unwrap();
        assert_eq!(result.columns, vec!["id", "val"]);
        assert_eq!(result.rows[0], vec!["42", "hello"]);
    }

    #[test]
    fn wrap_wasm_adapter_null_values() {
        let mock = MockWasmAdapter::with_result(ra_wasm::QueryResult {
            columns: vec![ra_wasm::ColumnInfo {
                name: "x".into(),
                type_name: None,
            }],
            rows: vec![vec![ra_wasm::Value::Null]],
            rows_affected: 0,
        });

        let mut adapter = wrap_wasm_adapter(Box::new(mock), Backend::Sqlite);
        let result = adapter.execute("SELECT NULL").unwrap();
        assert_eq!(result.rows[0], vec!["NULL"]);
    }

    #[test]
    fn wrap_wasm_adapter_backend_info() {
        let mock = MockWasmAdapter::succeeding();
        let adapter = wrap_wasm_adapter(Box::new(mock), Backend::DuckDb);
        assert_eq!(adapter.backend_name(), "duckdb");
        assert_eq!(adapter.isolation_level_name(), "snapshot isolation");
    }

    #[test]
    fn sqlite_factory_creates_adapters() {
        let factory = sqlite_wasm_factory(Box::new(|_name| {
            Ok(Box::new(MockWasmAdapter::succeeding()) as Box<dyn ra_wasm::DatabaseAdapter>)
        }));

        let mut adapter = factory("s1");
        let result = adapter.execute("SELECT 1").unwrap();
        assert_eq!(result.columns, vec!["ok"]);
        assert_eq!(adapter.backend_name(), "sqlite");
    }

    #[test]
    fn duckdb_factory_creates_adapters() {
        let factory = duckdb_wasm_factory(Box::new(|_name| {
            Ok(Box::new(MockWasmAdapter::succeeding()) as Box<dyn ra_wasm::DatabaseAdapter>)
        }));

        let mut adapter = factory("s1");
        let result = adapter.execute("SELECT 1").unwrap();
        assert_eq!(result.columns, vec!["ok"]);
        assert_eq!(adapter.backend_name(), "duckdb");
    }

    #[test]
    fn factory_handles_open_failure() {
        let factory = sqlite_wasm_factory(Box::new(|_name| Err("connection refused".into())));

        let mut adapter = factory("s1");
        let result = adapter.execute("SELECT 1");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("connection refused"),
            "expected error message, got: {err_msg}"
        );
    }

    #[test]
    fn factory_in_executor() {
        use crate::executor::TestExecutor;
        use crate::spec_parser;

        let input = r#"
session "s1"
{
    step "read"
    {
        SELECT 1;
    }
}

session "s2"
{
    step "write"
    {
        INSERT INTO t VALUES (1);
    }
}

permutation
{
    s1:read
    s2:write
}
"#;
        let spec = spec_parser::parse(input).unwrap_or_else(|e| panic!("parse failed: {e}"));

        let factory = sqlite_wasm_factory(Box::new(|_name| {
            Ok(Box::new(MockWasmAdapter::succeeding()) as Box<dyn ra_wasm::DatabaseAdapter>)
        }));

        let mut executor = TestExecutor::new(spec);
        let result = executor.run(&factory).unwrap_or_else(|e| {
            panic!("execution failed: {e}");
        });
        assert!(result.passed);
        assert_eq!(result.permutation_results.len(), 1);
    }
}
