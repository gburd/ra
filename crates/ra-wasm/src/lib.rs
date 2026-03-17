//! WASM database adapters for browser-based SQL execution.
//!
//! This crate provides a unified [`DatabaseAdapter`] trait over
//! `SQLite` WASM and `DuckDB` WASM, with connection pooling and
//! configurable storage backends (OPFS, `IndexedDB`, in-memory).
//!
//! # Architecture
//!
//! Rust code compiled to WASM calls into JavaScript bridge modules
//! (`sqlite_bridge.js`, `duckdb_bridge.js`) via `wasm-bindgen`.
//! The JS bridges handle WASM binary loading and expose a thin
//! synchronous API that the Rust adapters consume.
//!
//! ```text
//!  Rust (ra-wasm)
//!    |
//!    +-- SqliteAdapter --[wasm-bindgen]--> sqlite_bridge.js
//!    |                                       |
//!    |                                   @sqlite.org/sqlite-wasm
//!    |
//!    +-- DuckDbAdapter --[wasm-bindgen]--> duckdb_bridge.js
//!                                            |
//!                                        @duckdb/duckdb-wasm
//! ```

#![warn(missing_docs)]

pub mod adapter;
pub mod duckdb;
pub mod errors;
pub mod optimizer;
pub mod pool;
pub mod sqlite;
pub mod storage;

// Re-exports for convenience.
pub use adapter::{
    ColumnInfo, ConnectionConfig, DatabaseAdapter, DatabaseEngine, QueryResult, Value,
};
pub use errors::WasmDbError;
pub use optimizer::{WasmOptimizer, OptimizationResult, OptimizerConfig};
pub use pool::{ConnectionPool, PoolConfig, PooledConnection};
pub use storage::StorageBackend;
