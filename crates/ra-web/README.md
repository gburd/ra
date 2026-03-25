# Ra Web - SQL Query Optimization Explorer

REST API server for SQL query optimization, visualization, and analysis.

## Features

- Query plan optimization via Ra engine (e-graph based)
- Multi-database comparison (PostgreSQL, MySQL, DuckDB, SQLite)
- SQL dialect translation
- Rewrite rule tracing
- Query sharing
- WebSocket support for real-time isolation testing
- Rate limiting and CORS

## Quick Start

### Development

```bash
cargo run --bin ra-web
# Server starts on http://localhost:8000
```

### With Frontend

```bash
# Terminal 1: Backend
cargo run --bin ra-web

# Terminal 2: Frontend
cd crates/ra-web-ui && pnpm dev
# UI available at http://localhost:5173
```

### Production

```bash
# Build frontend
cd crates/ra-web-ui && pnpm build

# Run backend serving the SvelteKit build
STATIC_DIR=crates/ra-web-ui/build cargo run --bin ra-web --release
# Full app at http://localhost:8000
```

## API Endpoints

### Query Optimization

- `POST /api/visualize` - Parse and optimize SQL, return plan tree with costs
  - Request: `{ "sql": "SELECT ...", "hardware_profile": "nvme" }`
  - Response: `{ "plan": {...}, "total_cost": 123.45, "rules_applied": [...] }`

- `POST /api/optimize` - Optimize a RelExpr directly
  - Request: `{ "expr": { "Scan": { "table": "users" } } }`
  - Response: `{ "original": {...}, "optimized": {...}, "rules_applied": 15 }`

### Multi-Database Comparison

- `POST /api/compare-plans` - Compare Ra vs PostgreSQL, MySQL, DuckDB
  - Request: `{ "sql": "SELECT ...", "hardware_profile": "ssd" }`
  - Response: `{ "plans": [{ "optimizer": "Ra", "total_cost": 100, ... }], "summary": { "cheapest": "Ra", "costs": [...] } }`

- `POST /api/compare` - Compare execution results across engines
  - Request: `{ "sql": "SELECT 1", "engines": ["sqlite", "duckdb"] }`
  - Response: `{ "results": [...], "matching": true }`

### Query Execution

- `POST /api/execute` - Execute SQL query
  - Request: `{ "sql": "SELECT ...", "engine": "sqlite" }`
  - Response: `{ "columns": [...], "rows": [...], "rows_affected": 0, "engine": "sqlite" }`

- `POST /api/explain` - EXPLAIN / EXPLAIN ANALYZE
  - Request: `{ "sql": "SELECT ...", "engine": "duckdb", "analyze": true }`
  - Response: `{ "plan": "...", "engine": "duckdb", "analyzed": true }`

### Dialect Translation

- `POST /api/translate` - Translate SQL between dialects
  - Request: `{ "sql": "SELECT NOW()", "from": "pg", "to": "mysql" }`
  - Response: `{ "from": "pg", "to": "mysql", "original": "...", "translated": "..." }`

### Metadata

- `GET /api/rules` - List all optimizer rewrite rules
  - Response: `{ "count": 150, "rules": ["join-commutativity", ...] }`

### Query Sharing

- `POST /api/share` - Save query and get shareable ID
  - Request: `{ "sql": "SELECT 42" }`
  - Response: `{ "id": "abc123" }`

- `GET /api/share/:id` - Retrieve shared query
  - Response: `{ "sql": "SELECT 42" }`

### Synthesis

- `POST /api/synthesize` - Natural language to SQL
  - Request: `{ "query": "find all users over 25", "schema": "..." }`
  - Response: `{ "sql": "SELECT ...", "rel_expr": {...}, "warnings": [] }`

### Isolation Testing

- `POST /api/isolation/parse` - Parse an isolation test spec
  - Request: `{ "spec": "..." }`
  - Response: `{ "sessions": [...], "steps": [...] }`

- `POST /api/isolation/run` - Run isolation test
- `WS /ws/isolation` - WebSocket for live isolation testing

### Demos

- `GET /api/demos` - List available interactive demos
- `GET /api/demos/staleness-impact` - Stale statistics impact
- `GET /api/demos/hardware-plan` - Hardware-aware planning
- `GET /api/demos/join-algorithm` - Join algorithm selection
- `GET /api/demos/aggregation-strategy` - Aggregation strategies
- `GET /api/demos/index-selection` - Index recommendation
- `GET /api/demos/subquery-unnesting` - Subquery optimization
- `GET /api/demos/parallel-query` - Parallel execution
- `GET /api/demos/gpu-offloading` - GPU acceleration
- `GET /api/demos/distributed-query` - Distributed planning
- `GET /api/demos/cost-calibration` - Cost model calibration

### Utilities

- `GET /health` - Health check (exempt from rate limiting)

## Configuration

Environment variables:

| Variable         | Default     | Description                      |
|------------------|-------------|----------------------------------|
| `ROCKET_PORT`    | 8000        | Server port                      |
| `ROCKET_ADDRESS` | 0.0.0.0     | Bind address                     |
| `STATIC_DIR`     | `static/`   | Static files directory for SPA   |

Rate limiting: 100 requests per 60 seconds per IP address. Health
endpoint is exempt.

CORS: Allows all origins with `Cross-Origin-Embedder-Policy` and
`Cross-Origin-Opener-Policy` headers for SharedArrayBuffer support.

## Architecture

```
src/
  main.rs           Rocket launch, route mounting, SPA fallback
  cors.rs           CORS fairing (allows all origins)
  errors.rs         Typed API error responses (AppError, ApiResult)
  rate_limit.rs     Per-IP rate limiting fairing + RateGuard
  websocket.rs      WebSocket handler for isolation testing
  api/
    visualize.rs    Plan tree visualization and multi-optimizer comparison
    optimize.rs     RelExpr optimization via ra-engine::Optimizer
    execute.rs      SQL execution against sqlite/duckdb
    explain.rs      EXPLAIN / EXPLAIN ANALYZE
    compare.rs      Cross-engine result comparison
    translate.rs    SQL dialect translation via ra-dialect
    rules.rs        Optimizer rule listing
    share.rs        Query sharing (in-memory ShareStore)
    isolation.rs    Isolation level testing
    demos.rs        Interactive optimization demos (set 1)
    demos2.rs       Interactive optimization demos (set 2)
```

### Key Data Flow

```
SQL string
  -> ra_parser::sql_to_relexpr()      Parse to RelExpr
  -> relexpr_to_visual()              Convert to VisualPlanNode tree
  -> serde_json::to_value()           Serialize to JSON response
```

The `visualize.rs` module converts `RelExpr` variants into
`VisualPlanNode` structs with operator names, costs, row estimates,
and detail key-value pairs. Comparison endpoints build separate plan
trees for each optimizer (Ra, PostgreSQL, MySQL, DuckDB).

## Static Files

The server serves static files from:
1. `STATIC_DIR` environment variable (production)
2. `crates/ra-web/static/` relative to `CARGO_MANIFEST_DIR` (dev)

An SPA fallback route (rank 100) serves `index.html` for any path
not matched by API routes or static files, enabling client-side routing.

## Dependencies

| Crate           | Purpose                          |
|-----------------|----------------------------------|
| `ra-core`       | Algebra types (RelExpr, Expr)    |
| `ra-parser`     | SQL to RelExpr parsing           |
| `ra-engine`     | Query optimizer (e-graph based)  |
| `ra-compiler`   | Query compilation                |
| `ra-dialect`    | Multi-dialect SQL translation    |
| `ra-isolation`  | Isolation level testing          |
| `ra-synthesis`  | Natural language to SQL          |
| `ra-stats`      | Table/column statistics          |
| `ra-hardware`   | Hardware profile detection       |
| `rocket`        | Web framework                    |
| `rocket_ws`     | WebSocket support                |

## Testing

```bash
cargo test --package ra-web
```

29 tests covering all API endpoints: request/response validation,
error handling, rate limiting, CORS headers, SPA fallback, and
query sharing round-trips.
