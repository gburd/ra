# WASM Database Integration

This document describes the ra-wasm crate, which provides unified
access to SQLite and DuckDB compiled to WebAssembly.

## Overview

The ra-wasm crate enables running SQL databases entirely in the
browser or any WASM runtime. It wraps two database engines behind
a common `DatabaseAdapter` trait, with connection pooling and
configurable storage backends.

## Architecture

```
Rust (ra-wasm)
  |
  +-- SqliteAdapter --[wasm-bindgen]--> sqlite_bridge.js
  |                                       |
  |                                   @sqlite.org/sqlite-wasm
  |
  +-- DuckDbAdapter --[wasm-bindgen]--> duckdb_bridge.js
                                          |
                                      @duckdb/duckdb-wasm
```

Rust code compiled to WASM calls into JavaScript bridge modules
via `wasm-bindgen`. The JS bridges handle WASM binary loading and
expose a synchronous API that the Rust adapters consume.

## Supported Engines

### SQLite WASM

- Source: `@sqlite.org/sqlite-wasm`
- Full SQL-92 support with SQLite extensions
- Single-writer, multiple-reader concurrency
- Storage: in-memory, OPFS, IndexedDB

### DuckDB WASM

- Source: `@duckdb/duckdb-wasm`
- Columnar analytical engine
- Vectorized execution
- Storage: in-memory, OPFS

## Usage

### Creating Connections

```rust
use ra_wasm::{ConnectionConfig, DatabaseAdapter, DatabaseEngine};
use ra_wasm::storage::StorageBackend;

// In-memory SQLite
let config = ConnectionConfig::sqlite_memory();

// In-memory DuckDB
let config = ConnectionConfig::duckdb_memory();

// Persistent SQLite with OPFS
let config = ConnectionConfig {
    engine: DatabaseEngine::Sqlite,
    database_name: Some("mydb".into()),
    storage: StorageBackend::Opfs,
    read_only: false,
};
```

### Executing Queries

```rust
let adapter = SqliteAdapter::open(config)?;

let result = adapter.execute(
    "SELECT name, age FROM users WHERE age > 21"
)?;

for row in &result.rows {
    println!("{}: {}", row[0], row[1]);
}
```

### Connection Pooling

```rust
use ra_wasm::{ConnectionPool, PoolConfig};

let pool_config = PoolConfig {
    max_connections: 4,
    idle_timeout_ms: 30_000,
};

let pool = ConnectionPool::new(
    ConnectionConfig::sqlite_memory(),
    pool_config,
)?;

let conn = pool.acquire()?;
let result = conn.execute("SELECT 1")?;
```

## Storage Backends

| Backend   | Persistence | Browser Support | Use Case           |
|-----------|-------------|------------------|--------------------|
| Memory    | No          | All              | Testing, ephemeral |
| OPFS      | Yes         | Chrome 102+      | Production         |
| IndexedDB | Yes         | All              | Fallback           |

### OPFS (Origin Private File System)

OPFS provides a high-performance filesystem API within the browser
sandbox. It supports synchronous access from Web Workers, making it
suitable for database files.

### IndexedDB

IndexedDB is a fallback for browsers that lack OPFS support. It has
higher latency than OPFS but wider browser compatibility.

## Data Types

The `Value` enum maps between Rust and database types:

```rust
pub enum Value {
    Null,
    Integer(i64),
    Float(f64),
    Text(String),
    Blob(Vec<u8>),
    Boolean(bool),
}
```

## Query Results

```rust
pub struct QueryResult {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Value>>,
    pub rows_affected: u64,
}

pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
}
```

## Error Handling

```rust
use ra_wasm::WasmDbError;

match adapter.execute(sql) {
    Ok(result) => { /* process result */ }
    Err(WasmDbError::ConnectionFailed(msg)) => { /* ... */ }
    Err(WasmDbError::QueryFailed(msg)) => { /* ... */ }
    Err(WasmDbError::TypeError(msg)) => { /* ... */ }
    Err(WasmDbError::StorageError(msg)) => { /* ... */ }
}
```

## Integration with Isolation Testing

The ra-isolation crate can use ra-wasm adapters to run isolation
tests in WASM environments:

```rust
use ra_isolation::wasm_bridge::WasmBridgeAdapter;

let bridge = WasmBridgeAdapter::new(sqlite_adapter);
let executor = TestExecutor::new(bridge);
```

This enables testing transaction isolation behavior of SQLite and
DuckDB compiled to WebAssembly, verifying that isolation guarantees
hold in the WASM environment.

## Limitations

- **No native threads** -- WASM does not support native threads.
  Concurrency relies on Web Workers and SharedArrayBuffer.
- **Memory limits** -- WASM memory is bounded by browser limits
  (typically 2-4 GB).
- **No direct filesystem** -- All persistence goes through OPFS
  or IndexedDB; no direct file system access.
- **SQLite WAL mode** -- WAL mode requires OPFS with shared memory
  support (Chrome 104+).

## References

- SQLite WASM: https://sqlite.org/wasm/doc/trunk/index.md
- DuckDB WASM: https://duckdb.org/docs/api/wasm/overview
- OPFS spec: https://fs.spec.whatwg.org/
- wasm-bindgen: https://rustwasm.github.io/docs/wasm-bindgen/
