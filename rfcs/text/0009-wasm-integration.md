# RFC 0009: WASM Database Integration

- **Status:** Implemented
- **Type:** Retroactive
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

A WASM-based database integration layer that runs SQLite and DuckDB
entirely in the browser, enabling the RA web UI to execute real SQL
queries, compare optimizer plans against actual execution, and provide
interactive demonstrations without a server backend.

## Motivation

The RA web UI needed to execute SQL queries against real databases to
demonstrate optimization effects. Running databases server-side would
require infrastructure and limit accessibility. WASM-compiled
databases run entirely in the browser, making the web UI self-contained
and deployable as a static site.

Two engines were chosen to cover different workload profiles: SQLite
for transactional/OLTP patterns and DuckDB for analytical/OLAP
patterns.

## What Was Built

### Architecture

```
Rust (ra-wasm) --[wasm-bindgen]--> JavaScript bridges
  |                                      |
  +-- SqliteAdapter --------> @sqlite.org/sqlite-wasm
  +-- DuckDbAdapter --------> @duckdb/duckdb-wasm
```

Rust code compiled to WASM calls JavaScript bridge modules via
`wasm-bindgen`. The bridges handle WASM binary loading and expose
a synchronous API consumed by the Rust adapters.

### DatabaseAdapter Trait

Both engines implement a common trait:

- `execute(sql)` -- run a query and return results
- `explain(sql)` -- return the query plan
- `load_schema(ddl)` -- set up tables and indexes
- `get_statistics()` -- extract table/column statistics

### Connection Pooling

Connections are pooled per engine with configurable limits. The pool
handles WASM initialization overhead (loading the binary, compiling
the module) once and reuses connections.

### Storage Backends

- **In-memory**: fastest, data lost on page refresh
- **OPFS** (Origin Private File System): persistent, good performance
- **IndexedDB**: persistent, wider browser support, slower

### Crate

`ra-wasm` provides adapters, connection pooling, and the bridge
JavaScript modules.

## Key Design Decisions

- `wasm-bindgen` chosen over `wasm-pack` for finer control over the
  JS interface
- Two databases rather than one to demonstrate optimizer behavior
  across different execution engines (row-oriented vs columnar)
- Storage backend is configurable per connection to support both
  ephemeral demos and persistent workbenches

## Prior Art

- sql.js (SQLite compiled to WASM via Emscripten)
- DuckDB WASM (official DuckDB WASM build)
- Postgres WASM (PGlite) -- not used due to size constraints

## References

- `docs/wasm-databases.md` -- full documentation
- `crates/ra-wasm/` -- WASM adapters and bridges
- `web/` -- web UI consuming the WASM databases
