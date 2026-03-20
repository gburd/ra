# RFC 0026: WASM Database Integration

**Status:** Accepted
**Implemented:** Prior to 2026-03
**Commit:** Various

## Summary

Implemented WebAssembly adapters for SQLite and DuckDB, enabling browser-based SQL execution with the RA optimizer. The system provides a unified database interface with connection pooling and multiple storage backends (OPFS, IndexedDB, in-memory).

## Motivation

Browser-based data analysis is increasingly important:
- Zero-installation analytics tools
- Client-side data processing for privacy
- Offline-capable applications
- Educational SQL environments

Challenges addressed:
- Running full databases in browsers
- Persistent storage in web environment
- Performance optimization in WASM
- Cross-database compatibility

## Technical Design

### Architecture

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

### Database Adapter Trait

Unified interface for different engines:
```rust
pub trait DatabaseAdapter: Send + Sync {
    async fn connect(config: ConnectionConfig) -> Result<Self>;
    async fn execute(&self, sql: &str) -> Result<QueryResult>;
    async fn prepare(&self, sql: &str) -> Result<PreparedStatement>;
    fn engine(&self) -> DatabaseEngine;
}
```

### Storage Backends

**OPFS (Origin Private File System):**
- Persistent storage
- File-based databases
- Best performance
- Requires HTTPS

**IndexedDB:**
- Key-value storage
- Wider browser support
- Slower than OPFS
- Works on HTTP

**In-Memory:**
- No persistence
- Fastest performance
- Good for demos
- Limited by browser RAM

### Connection Pooling

Manage concurrent connections:
```rust
pub struct ConnectionPool {
    connections: Vec<Arc<dyn DatabaseAdapter>>,
    available: Arc<Mutex<VecDeque<usize>>>,
    config: PoolConfig,
}

impl ConnectionPool {
    pub async fn acquire(&self) -> PooledConnection {
        // Wait for available connection
        // Return wrapped connection with auto-release
    }
}
```

### WASM Optimizer Integration

Run RA optimizer in browser:
```rust
#[wasm_bindgen]
pub struct WasmOptimizer {
    engine: OptimizationEngine,
    stats: StatsProvider,
}

#[wasm_bindgen]
impl WasmOptimizer {
    pub fn optimize(&self, sql: &str) -> OptimizationResult {
        // Parse SQL
        // Convert to algebra
        // Run optimization
        // Return optimized plan
    }
}
```

### JavaScript Bridge

Minimal JS glue for WASM binaries:
```javascript
// sqlite_bridge.js
export async function initSqlite(config) {
    const sqlite3 = await sqlite3InitModule();
    const db = new sqlite3.oo1.DB(config.path, config.mode);
    return {
        exec: (sql) => db.exec(sql),
        close: () => db.close()
    };
}
```

## Implementation

### Key Files

- `crates/ra-wasm/src/adapter.rs`
  - `DatabaseAdapter` trait
  - Common types and interfaces

- `crates/ra-wasm/src/sqlite.rs`
  - SQLite WASM adapter
  - OPFS/IndexedDB support

- `crates/ra-wasm/src/duckdb.rs`
  - DuckDB WASM adapter
  - Columnar operations

- `crates/ra-wasm/src/optimizer.rs`
  - WASM optimizer bindings
  - Plan serialization

- `crates/ra-wasm/src/pool.rs`
  - Connection pooling
  - Resource management

### Build Configuration

```toml
[dependencies]
wasm-bindgen = "0.2"
web-sys = "0.3"
js-sys = "0.3"

[profile.release]
opt-level = "z"  # Size optimization
lto = true
codegen-units = 1
```

### Bundle Size

Optimized for web delivery:
- SQLite WASM: ~1.5MB gzipped
- DuckDB WASM: ~3MB gzipped
- RA Optimizer: ~500KB gzipped
- Lazy loading supported

## Usage

### Browser Setup

```html
<script type="module">
import { initRA } from './ra-wasm.js';

const ra = await initRA({
    engine: 'sqlite',
    storage: 'opfs',
    poolSize: 4
});

const result = await ra.execute(`
    SELECT * FROM users
    WHERE age > 21
`);
</script>
```

### TypeScript API

```typescript
interface RAWasm {
    execute(sql: string): Promise<QueryResult>;
    optimize(sql: string): Promise<OptimizedPlan>;
    loadData(table: string, data: any[]): Promise<void>;
}
```

## Testing

Browser-based test suite:
- WASM compilation tests
- Storage backend tests
- Query execution tests
- Performance benchmarks
- Memory leak detection

Test infrastructure:
- Playwright for browser automation
- Vitest for unit tests
- Memory profiling tools
- Size budget tracking

## Performance

Benchmark results (Chrome 120):
- TPC-H SF0.1: 2-5x slower than native
- Simple queries: < 50ms overhead
- Large results: streaming support
- Memory usage: ~100MB for SF1

Optimizations:
- SIMD when available
- Shared memory arrays
- Lazy loading
- Result streaming

## Browser Compatibility

Minimum requirements:
- Chrome 95+ (OPFS support)
- Firefox 90+ (IndexedDB only)
- Safari 15.2+ (limited OPFS)
- Edge 95+

Feature detection:
```javascript
const hasOPFS = 'storage' in navigator;
const hasSharedArrayBuffer = typeof SharedArrayBuffer !== 'undefined';
const hasSimd = WebAssembly.validate(new Uint8Array([...]));
```

## Use Cases

**Data Exploration:**
- CSV/JSON file analysis
- Local data processing
- Interactive dashboards

**Education:**
- SQL learning environments
- Query optimization tutorials
- Database course tools

**Privacy-First Analytics:**
- Client-side aggregation
- No server round-trips
- GDPR compliance

**Offline Applications:**
- PWAs with local databases
- Sync when online
- Conflict resolution

## Security Considerations

- Runs in browser sandbox
- No filesystem access (except OPFS)
- Cross-origin isolation required
- CSP headers recommended

## References

- SQLite WASM Documentation
- DuckDB WASM Architecture
- WebAssembly System Interface (WASI)
- Origin Private File System API

## Future Work

- WebGPU acceleration
- Shared workers for parallelism
- WebTransport for streaming
- WASM component model
- Native filesystem access API