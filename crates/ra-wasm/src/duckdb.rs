//! `DuckDB` WASM adapter.
//!
//! Wraps the `@duckdb/duckdb-wasm` JavaScript module behind the
//! [`DatabaseAdapter`] trait. The JS side must load the `DuckDB` WASM
//! bundles and expose the thin API declared in the extern block.

use std::cell::Cell;

use wasm_bindgen::prelude::*;

use crate::adapter::{ConnectionConfig, DatabaseAdapter, DatabaseEngine, QueryResult, Value};
use crate::errors::{Result, WasmDbError};

// JS interop bindings for the DuckDB WASM glue layer.
#[wasm_bindgen(module = "/js/duckdb_bridge.js")]
extern "C" {
    #[wasm_bindgen(js_name = "duckdbOpen", catch)]
    fn duckdb_open(config_json: &str) -> std::result::Result<u32, JsValue>;

    #[wasm_bindgen(js_name = "duckdbExec", catch)]
    fn duckdb_exec(handle: u32, sql: &str) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "duckdbQuery", catch)]
    fn duckdb_query(handle: u32, sql: &str) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "duckdbExecParams", catch)]
    fn duckdb_exec_params(
        handle: u32,
        sql: &str,
        params_json: &str,
    ) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "duckdbQueryParams", catch)]
    fn duckdb_query_params(
        handle: u32,
        sql: &str,
        params_json: &str,
    ) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "duckdbClose", catch)]
    fn duckdb_close(handle: u32) -> std::result::Result<(), JsValue>;
}

/// A connection to a `DuckDB` WASM database.
#[derive(Debug)]
pub struct DuckDbAdapter {
    handle: u32,
    open: Cell<bool>,
}

impl DuckDbAdapter {
    /// Open a new `DuckDB` WASM connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the JS bridge fails to open the database.
    pub fn open(config: &ConnectionConfig) -> Result<Self> {
        if config.engine != DatabaseEngine::DuckDb {
            return Err(WasmDbError::Init(format!(
                "DuckDbAdapter requires DuckDb engine, got {}",
                config.engine
            )));
        }
        let config_json = serde_json::to_string(config)?;
        let handle = duckdb_open(&config_json).map_err(|e| WasmDbError::Init(js_err_msg(&e)))?;
        Ok(Self {
            handle,
            open: Cell::new(true),
        })
    }

    fn check_open(&self) -> Result<()> {
        if self.open.get() {
            Ok(())
        } else {
            Err(WasmDbError::Connection("connection is closed".into()))
        }
    }
}

impl DatabaseAdapter for DuckDbAdapter {
    fn execute(&self, sql: &str) -> Result<QueryResult> {
        self.check_open()?;
        let json = duckdb_exec(self.handle, sql).map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn query(&self, sql: &str) -> Result<QueryResult> {
        self.check_open()?;
        let json =
            duckdb_query(self.handle, sql).map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn execute_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        self.check_open()?;
        let params_json = serde_json::to_string(params)?;
        let json = duckdb_exec_params(self.handle, sql, &params_json)
            .map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn query_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        self.check_open()?;
        let params_json = serde_json::to_string(params)?;
        let json = duckdb_query_params(self.handle, sql, &params_json)
            .map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn close(&self) -> Result<()> {
        if !self.open.get() {
            return Ok(());
        }
        duckdb_close(self.handle).map_err(|e| WasmDbError::Connection(js_err_msg(&e)))?;
        self.open.set(false);
        Ok(())
    }

    fn engine(&self) -> DatabaseEngine {
        DatabaseEngine::DuckDb
    }

    fn is_open(&self) -> bool {
        self.open.get()
    }
}

impl Drop for DuckDbAdapter {
    fn drop(&mut self) {
        if self.open.get() {
            let _ = duckdb_close(self.handle);
        }
    }
}

fn parse_query_result(json: &str) -> Result<QueryResult> {
    serde_json::from_str(json)
        .map_err(|e| WasmDbError::Serialization(format!("failed to parse query result: {e}")))
}

fn js_err_msg(val: &JsValue) -> String {
    val.as_string()
        .or_else(|| {
            js_sys::JSON::stringify(val)
                .ok()
                .and_then(|s| s.as_string())
        })
        .unwrap_or_else(|| format!("{val:?}"))
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_result_valid() {
        let json = r#"{
            "columns": [
                {"name": "x", "type_name": null},
                {"name": "y", "type_name": "TEXT"}
            ],
            "rows": [
                [{"Integer": 1}, {"Text": "hello"}],
                [{"Integer": 2}, {"Text": "world"}]
            ],
            "rows_affected": 0
        }"#;
        let result = parse_query_result(json);
        assert!(result.is_ok());
        let r = result.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.row_count(), 2);
        assert_eq!(r.column_count(), 2);
    }

    #[test]
    fn parse_query_result_empty() {
        let json = r#"{
            "columns": [],
            "rows": [],
            "rows_affected": 5
        }"#;
        let result = parse_query_result(json);
        assert!(result.is_ok());
        let r = result.unwrap_or_else(|e| panic!("{e}"));
        assert!(r.is_empty());
        assert_eq!(r.rows_affected, 5);
    }

    #[test]
    fn parse_query_result_invalid() {
        let result = parse_query_result("{bad json}");
        assert!(result.is_err());
    }
}
