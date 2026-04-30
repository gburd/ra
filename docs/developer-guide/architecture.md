# RA Architecture

This document describes the system architecture of the RA (Relational Algebra) optimizer.

## Overview

RA is a multi-layered query optimization system that uses equality saturation (via `egg`) to generate optimal query execution plans. The system comprises:

- **Parser layer** - Multi-dialect SQL parsing with vendor-specific extensions
- **Core layer** - Relational algebra AST and type system
- **Engine layer** - E-graph-based optimization with 50+ rewrite rules
- **Adapter layer** - Database-specific statistics gathering and execution
- **Web layer** - REST API and React frontend for visualization

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Frontend (React)                         │
│  ┌──────────────┐  ┌─────────────┐  ┌────────────────────────┐ │
│  │ SQL Editor   │  │ Plan Viewer │  │ Comparison Dashboard   │ │
│  │ (Monaco)     │  │ (D3.js)     │  │ (Multi-engine)         │ │
│  └──────────────┘  └─────────────┘  └────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                               │
                               │ HTTP/JSON
                               │
┌─────────────────────────────────────────────────────────────────┐
│                      Backend (Rocket)                            │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ API Layer                                                 │  │
│  │ /api/explain | /api/optimize | /api/compare | /api/share │  │
│  └──────────────────────────────────────────────────────────┘  │
│                               │                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ra-parser: SQL → RelExpr                                 │  │
│  │ - Multi-dialect parsing (PostgreSQL, MySQL, Oracle, etc.)│  │
│  │ - Extension support (pgvector, PostGIS, TimescaleDB)     │  │
│  │ - Profile-based dialect detection                        │  │
│  └──────────────────────────────────────────────────────────┘  │
│                               │                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ra-engine: Optimization via egg                          │  │
│  │ - E-graph construction from RelExpr                      │  │
│  │ - Equality saturation with 50+ rules                     │  │
│  │ - Cost-based extraction                                  │  │
│  │ - Federated/distributed query optimization               │  │
│  └──────────────────────────────────────────────────────────┘  │
│                               │                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ ra-adapters: Database Integration                        │  │
│  │ - PostgreSQL | MySQL | SQLite | DuckDB adapters         │  │
│  │ - Statistics gathering (row counts, histograms)          │  │
│  │ - Schema introspection (tables, indexes, constraints)    │  │
│  └──────────────────────────────────────────────────────────┘  │
│                               │                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ Redis Cache                                              │  │
│  │ - Query result caching                                   │  │
│  │ - Share URL storage                                      │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                               │
                               │ SQL execution
                               │
┌─────────────────────────────────────────────────────────────────┐
│              External Databases (PostgreSQL, MySQL, etc.)        │
└─────────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. ra-core

Fundamental types and traits for the relational algebra system.

**Key modules:**
- `algebra.rs` - Relational algebra AST (`RelExpr` enum)
- `expr.rs` - Scalar expressions (literals, columns, operators)
- `cost.rs` - Cost model traits
- `statistics.rs` - Statistics types (histograms, cardinality estimates)
- `facts.rs` - `FactsProvider` trait for metadata
- `rule.rs` - Rewrite rule traits

**Core types:**

```rust
pub enum RelExpr {
    Scan { table: String, alias: Option<String> },
    Filter { input: Box<RelExpr>, predicate: Expr },
    Project { input: Box<RelExpr>, columns: Vec<Expr> },
    Join { left: Box<RelExpr>, right: Box<RelExpr>, on: Expr, kind: JoinKind },
    Aggregate { input: Box<RelExpr>, group_by: Vec<Expr>, aggs: Vec<AggExpr> },
    Sort { input: Box<RelExpr>, order_by: Vec<OrderByExpr> },
    Limit { input: Box<RelExpr>, limit: usize, offset: Option<usize> },
    // ... 30+ additional operators
}

pub enum Expr {
    Literal(Value),
    Column { table: Option<String>, name: String },
    BinaryOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    UnaryOp { op: UnaryOp, expr: Box<Expr> },
    Function { name: String, args: Vec<Expr> },
    Cast { expr: Box<Expr>, data_type: DataType },
    // ... additional expression types
}
```

### 2. ra-parser

Multi-dialect SQL parser with extension support.

**Key modules:**
- `parser/mod.rs` - Parser entry point and orchestration
- `grammar/` - SQL grammar definitions
- `profile/` - Dialect profiles and feature detection
- `sql_to_relexpr.rs` - SQL AST → `RelExpr` conversion

**Profile system:**

Profiles define which SQL features are supported for a given database:

```toml
# profiles/vendors/postgresql-17.toml
[profile]
name = "postgresql-17"
vendor = "postgresql"
version = "17"

[features]
sql_92 = true
sql_2016 = true
sql_2023 = true
lateral_joins = true
window_functions = true
recursive_cte = true

[operators]
contains = ["@>", "<@", "&&", "@?", "@@"]
```

**Profile composition:**

Extensions can be added to base profiles:

```rust
// Load PostgreSQL 17 with pgvector extension
let profile = ParserProfile::load("postgresql-17+pgvector")?;

// Multi-extension composition
let profile = ParserProfile::load("postgresql-17+postgis+timescaledb")?;
```

**Dialect inference:**

The parser can automatically detect SQL dialects:

```rust
let (profile, confidence) = ParserProfile::infer(sql)?;
// Detects PostgreSQL from ARRAY[...], @>, etc.
```

### 3. ra-engine

E-graph-based optimization engine using `egg`.

**Key modules:**
- `egraph.rs` - E-graph construction and optimization
- `analysis.rs` - E-graph analysis for tracking properties
- `rewrite.rs` - 50+ rewrite rules
- `cost.rs` - Cost models (integrated, federated, network)
- `extract.rs` - Best plan extraction
- `memo.rs` - Structural hash-based plan caching

**Optimization pipeline:**

```
SQL text
  ↓
Parser (ra-parser)
  ↓
RelExpr (ra-core)
  ↓
E-graph construction (egg)
  ↓
Equality saturation (apply rewrite rules)
  ↓
Cost-based extraction (find cheapest equivalent)
  ↓
Optimized RelExpr
```

**Rewrite rule example:**

```rust
// Predicate pushdown through join
rewrite!(
    "filter-pushdown-join";
    "(Filter ?input (And ?pred1 ?pred2))" =>
    "(Filter (Filter ?input ?pred1) ?pred2)"
)
```

**E-graph analysis:**

The engine tracks properties during optimization:

```rust
pub struct RelAnalysis {
    // Tables referenced by this expression
    tables: HashSet<String>,
    // Output schema (column names and types)
    schema: Vec<(String, DataType)>,
    // Estimated cardinality
    cardinality: Option<f64>,
}
```

**Cost model:**

```rust
pub trait CostFn {
    fn cost(&self, expr: &RelExpr, facts: &dyn FactsProvider) -> f64;
}

// Example: scan cost = rows * page_size / throughput
fn scan_cost(table: &str, facts: &dyn FactsProvider) -> f64 {
    let stats = facts.table_stats(table);
    stats.row_count as f64 * 0.001 // 1ms per 1000 rows
}
```

### 4. ra-adapters

Database adapter layer for statistics and execution.

**Adapter trait:**

```rust
pub trait DatabaseAdapter: Send + Sync {
    fn connect(&mut self, connection_string: &str) -> Result<(), AdapterError>;
    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError>;
    fn gather_column_stats(&self, table: &str) -> Result<HashMap<String, ColumnStats>, AdapterError>;
    fn get_schema_info(&self) -> Result<SchemaInfo, AdapterError>;
    fn get_capabilities(&self) -> Result<DatabaseCapabilities, AdapterError>;
    fn sql_dialect(&self) -> SqlDialect;
    fn as_facts_provider(&self) -> &dyn FactsProvider;
}
```

**Statistics gathering:**

Adapters query database system catalogs to gather optimization metadata:

```sql
-- PostgreSQL adapter queries pg_stats
SELECT
    attname AS column_name,
    n_distinct,
    null_frac,
    correlation,
    most_common_vals,
    most_common_freqs
FROM pg_stats
WHERE tablename = $1;
```

**FactsProvider integration:**

Adapters implement `FactsProvider` to supply stats to the optimizer:

```rust
impl FactsProvider for PostgresAdapter {
    fn table_stats(&self, table: &str) -> Option<CoreTableStats> {
        self.statistics.get(table).map(|stats| CoreTableStats {
            row_count: stats.row_count,
            page_count: stats.page_count,
            // ... other fields
        })
    }
}
```

## Data Flow

### Query Optimization Flow

```
1. SQL Input
   "SELECT * FROM users WHERE age > 25"
   ↓
2. Parser (ra-parser)
   - Dialect detection (PostgreSQL detected)
   - Parse to SQL AST
   - Convert to RelExpr
   ↓
3. RelExpr
   Filter {
     input: Scan { table: "users" },
     predicate: BinaryOp { op: Gt, left: Column("age"), right: Literal(25) }
   }
   ↓
4. E-graph Construction
   - Add RelExpr to e-graph
   - Run equality saturation (50+ rules)
   - Generate equivalent plans
   ↓
5. E-graph Analysis
   - Track table references
   - Propagate schema information
   - Estimate cardinality
   ↓
6. Cost-based Extraction
   - Compute cost for each e-class
   - Extract minimum-cost plan
   ↓
7. Optimized RelExpr
   IndexScan {
     table: "users",
     index: "users_age_idx",
     condition: age > 25
   }
```

### API Request Flow

```
Frontend                      Backend                    Database
   │                             │                           │
   │ POST /api/explain           │                           │
   │ { sql, engine }             │                           │
   ├────────────────────────────>│                           │
   │                             │                           │
   │                             │ Parse SQL (ra-parser)     │
   │                             │ Detect dialect            │
   │                             │                           │
   │                             │ Get adapter for engine    │
   │                             │ Connect to database       │
   │                             ├──────────────────────────>│
   │                             │                           │
   │                             │         EXPLAIN SQL       │
   │                             │<──────────────────────────┤
   │                             │                           │
   │                             │ Parse plan output         │
   │                             │ Extract cost metrics      │
   │                             │                           │
   │   { plan, cost, metrics }   │                           │
   │<────────────────────────────┤                           │
   │                             │                           │
   │ Render visualization        │                           │
   │                             │                           │
```

## Technology Stack

### Core

| Component | Technology | Purpose |
|-----------|-----------|---------|
| Optimization | egg 0.9 | E-graph equality saturation |
| Parsing | ra-sql-parser | SQL parsing (custom fork) |
| Execution | tokio 1.x | Async runtime |
| Database | postgres 0.19 | PostgreSQL client |
| Database | stoolap 0.3 | Stoolap client |

### Development

| Tool | Purpose |
|------|---------|
| cargo | Rust build system |
| clippy | Rust linter |
| rustfmt | Rust formatter |

## Performance Characteristics

### Optimization Performance

- **E-graph saturation:** 10-100ms for typical queries
- **Large joins (8+ tables):** 500ms-2s with genetic algorithms
- **Rule application:** 50+ rules applied per iteration
- **Convergence:** Typically 3-5 iterations until fixed point

### Memory Usage

- **E-graph size:** ~1MB per 1000 e-nodes
- **Memo table:** Structural hashing reduces memory by 60%
- **Plan cache:** LRU cache with configurable capacity

### Scalability

- **Concurrent requests:** 100+ queries/second on single instance
- **Rate limiting:** 100 requests/minute per IP
- **Connection pooling:** 10 connections per database adapter

## Extension Points

### Adding a New Database Adapter

Implement the `DatabaseAdapter` trait:

```rust
pub struct NewDbAdapter {
    connection: Option<NewDbConnection>,
    statistics: HashMap<String, TableStats>,
}

impl DatabaseAdapter for NewDbAdapter {
    fn connect(&mut self, conn_str: &str) -> Result<(), AdapterError> {
        // Connect to database
    }

    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError> {
        // Query system catalogs
    }

    // ... implement other methods
}
```

See [parsers.md](parsers.md) for detailed instructions.

### Adding Rewrite Rules

Add rules to `ra-engine/src/rewrite.rs`:

```rust
rewrite!(
    "my-custom-rule";
    "(Filter ?input ?pred)" => "(MyOptimizedOp ?input ?pred)"
    if some_condition
)
```

### Adding SQL Extensions

Create extension profile in `ra-parser/profiles/extensions/`:

```toml
[profile]
name = "my-extension"
extends = "postgresql-17"

[operators]
custom_ops = ["<~>", "@@", "??"]

[functions]
custom_funcs = ["my_func", "another_func"]
```

## Security Considerations

- **SQL injection:** Parser validates input before execution
- **Rate limiting:** 100 requests/minute per IP
- **CORS:** Configurable origin whitelist
- **Secrets:** Environment variables only (never committed)
- **Redis:** Unauthenticated in development, authenticated in production

## Testing Strategy

- **Unit tests:** Per-module tests in `ra-core`, `ra-parser`, `ra-engine`
- **Integration tests:** End-to-end optimizer tests in `ra-engine/tests/`
- **Property tests:** `proptest` for parser and optimizer invariants
- **Regression tests:** SQL query corpus in `ra-regression/`
- **Benchmarks:** `criterion` benchmarks in `ra-engine/benches/`

## Further Reading

- [parsers.md](parsers.md) - Parser system and adding database engines
- [contributing.md](contributing.md) - Development setup and guidelines
- [RFCs](../rfcs/) - Design documents for major features
