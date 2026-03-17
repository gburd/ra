//! `SQLite` WASM adapter.
//!
//! Wraps the `@sqlite.org/sqlite-wasm` JavaScript module behind
//! the [`DatabaseAdapter`] trait. The JS side must load the WASM
//! binary and expose a thin API that this module calls via
//! `wasm-bindgen` extern blocks.

use std::cell::Cell;

use wasm_bindgen::prelude::*;

use crate::adapter::{
    ColumnInfo, ConnectionConfig, DatabaseAdapter, DatabaseEngine, QueryResult, Value,
};
use crate::errors::{Result, WasmDbError};

// JS interop bindings for the SQLite WASM glue layer.
// These must be provided by a companion JS module that loads
// @sqlite.org/sqlite-wasm and exposes the functions below.
#[wasm_bindgen(module = "/js/sqlite_bridge.js")]
extern "C" {
    #[wasm_bindgen(js_name = "sqliteOpen", catch)]
    fn sqlite_open(config_json: &str) -> std::result::Result<u32, JsValue>;

    #[wasm_bindgen(js_name = "sqliteExec", catch)]
    fn sqlite_exec(handle: u32, sql: &str) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "sqliteQuery", catch)]
    fn sqlite_query(handle: u32, sql: &str) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "sqliteExecParams", catch)]
    fn sqlite_exec_params(
        handle: u32,
        sql: &str,
        params_json: &str,
    ) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "sqliteQueryParams", catch)]
    fn sqlite_query_params(
        handle: u32,
        sql: &str,
        params_json: &str,
    ) -> std::result::Result<String, JsValue>;

    #[wasm_bindgen(js_name = "sqliteClose", catch)]
    fn sqlite_close(handle: u32) -> std::result::Result<(), JsValue>;
}

/// A connection to a `SQLite` WASM database.
#[derive(Debug)]
pub struct SqliteAdapter {
    handle: u32,
    open: Cell<bool>,
}

impl SqliteAdapter {
    /// Open a new `SQLite` WASM connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the JS bridge fails to open the database.
    pub fn open(config: &ConnectionConfig) -> Result<Self> {
        if config.engine != DatabaseEngine::Sqlite {
            return Err(WasmDbError::Init(format!(
                "SqliteAdapter requires Sqlite engine, got {}",
                config.engine
            )));
        }
        let config_json = serde_json::to_string(config)?;
        let handle = sqlite_open(&config_json).map_err(|e| WasmDbError::Init(js_err_msg(&e)))?;
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

impl DatabaseAdapter for SqliteAdapter {
    fn execute(&self, sql: &str) -> Result<QueryResult> {
        self.check_open()?;
        let json = sqlite_exec(self.handle, sql).map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn query(&self, sql: &str) -> Result<QueryResult> {
        self.check_open()?;
        let json =
            sqlite_query(self.handle, sql).map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn execute_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        self.check_open()?;
        let params_json = serde_json::to_string(params)?;
        let json = sqlite_exec_params(self.handle, sql, &params_json)
            .map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn query_with_params(&self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        self.check_open()?;
        let params_json = serde_json::to_string(params)?;
        let json = sqlite_query_params(self.handle, sql, &params_json)
            .map_err(|e| WasmDbError::Query(js_err_msg(&e)))?;
        parse_query_result(&json)
    }

    fn close(&self) -> Result<()> {
        if !self.open.get() {
            return Ok(());
        }
        sqlite_close(self.handle).map_err(|e| WasmDbError::Connection(js_err_msg(&e)))?;
        self.open.set(false);
        Ok(())
    }

    fn engine(&self) -> DatabaseEngine {
        DatabaseEngine::Sqlite
    }

    fn is_open(&self) -> bool {
        self.open.get()
    }
}

impl Drop for SqliteAdapter {
    fn drop(&mut self) {
        if self.open.get() {
            let _ = sqlite_close(self.handle);
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

/// Build column info from a list of column names.
#[must_use]
pub fn columns_from_names(names: &[String]) -> Vec<ColumnInfo> {
    names
        .iter()
        .map(|name| ColumnInfo {
            name: name.clone(),
            type_name: None,
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn columns_from_names_empty() {
        let cols = columns_from_names(&[]);
        assert!(cols.is_empty());
    }

    #[test]
    fn columns_from_names_multiple() {
        let names: Vec<String> = vec!["id".into(), "name".into(), "age".into()];
        let cols = columns_from_names(&names);
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[0].name, "id");
        assert_eq!(cols[1].name, "name");
        assert_eq!(cols[2].name, "age");
        assert!(cols[0].type_name.is_none());
    }

    #[test]
    fn parse_query_result_valid() {
        let json = r#"{
            "columns": [{"name": "x", "type_name": null}],
            "rows": [[{"Integer": 1}]],
            "rows_affected": 0
        }"#;
        let result = parse_query_result(json);
        assert!(result.is_ok());
        let r = result.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.row_count(), 1);
        assert_eq!(r.column_count(), 1);
    }

    #[test]
    fn parse_query_result_invalid_json() {
        let result = parse_query_result("not json");
        assert!(result.is_err());
    }
}
