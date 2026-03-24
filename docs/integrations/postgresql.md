# PostgreSQL Planner Extension

Ra integrates with PostgreSQL as a native extension (`ra_pg_extension`)
that hooks into the query planner. When a SELECT query arrives, the
extension converts it to Ra's relational algebra representation,
optimizes it using the e-graph engine, and guides PostgreSQL's standard
planner toward the optimized plan via cost manipulation.

## Current Status

**Status: Functional MVP (v0.1.0)**

The extension compiles against PostgreSQL 13--18 via
[pgrx](https://github.com/pgcentralfoundation/pgrx) 0.17.0. It hooks
`planner_hook`, intercepts SELECT queries, and applies Ra optimization
advice through GUC-based cost manipulation. DML statements (INSERT,
UPDATE, DELETE) and utility commands pass through unmodified.

### What Works

- Planner hook registration and chaining with previous hooks
- Query parsing: SELECT with joins, filters, projections, aggregates
- Window functions (ROW_NUMBER, RANK, DENSE_RANK, LAG, LEAD, etc.)
- Common table expressions (WITH, WITH RECURSIVE, cycle detection)
- Set operations (UNION, INTERSECT, EXCEPT -- with ALL variants)
- Subqueries, VALUES, function RTEs
- Full expression conversion (CASE, COALESCE, IN, EXISTS, etc.)
- Statistics gathering from `pg_class`, `pg_statistic` via syscache
  (no SPI -- safe inside planner hooks)
- Index metadata from `pg_index`, `pg_class` via catalog scans
- Foreign key discovery from `pg_constraint` catalog scans
- PostgreSQL-specific MVCC statistics (dead tuple ratio, bloat factor)
- Hardware-aware cost parameters (SSD vs HDD detection)
- Cost calibration between Ra and PostgreSQL cost models
- GUC variables for runtime configuration
- Confidence-based plan selection (only applies plans above threshold)
- Panic-safe fallback to standard planner on any error
- Integration tests covering combined features (CTEs + windows + set ops)

### What Does Not Work Yet

- `SimpleFactsProvider` delegates to `EmptyFactsProvider` -- pg_stats
  data is gathered but not yet fully mapped to Ra's `TableStats` /
  `ColumnStats` types for optimization
- Improvement factor estimation returns a fixed 0.8 (placeholder)
- No direct `PlannedStmt` construction -- the extension manipulates
  GUC parameters to *guide* the standard planner rather than replacing
  it
- NUMERIC constants are approximated as 0.0 during expression parsing
- Correlated subqueries (SubLink) are represented as placeholders
- `FieldSelect` nodes lose field access information
- No benchmarks against TPC-H or other standard workloads yet

## Architecture

### How Ra Integrates with PostgreSQL

```
PostgreSQL Backend Process
+---------------------------------------------------------+
|  SQL Query                                              |
|    |                                                    |
|    v                                                    |
|  Parse/Analyze (standard PostgreSQL)                    |
|    |                                                    |
|    v                                                    |
|  planner_hook  <-- ra_planner_hook() intercepts here    |
|    |                                                    |
|    +---> Is ra_planner.enabled = on?                    |
|    |     Is it a SELECT?                                |
|    |     Relations <= ra_planner.max_relations?          |
|    |                                                    |
|    +---> [YES] Ra Optimization Pipeline                 |
|    |     1. query_parser: Query -> RelExpr              |
|    |     2. stats_bridge: pg_class/pg_statistic -> Stats|
|    |     3. ra-engine: e-graph optimization             |
|    |     4. cost_mapper: Ra Cost -> PG Cost             |
|    |     5. plan_converter: RelExpr -> plan advice      |
|    |     6. Confidence check >= min_confidence?         |
|    |        [YES] Apply advice via GUC manipulation     |
|    |        [NO]  Fall back to PG planner               |
|    |                                                    |
|    +---> [NO] call standard_planner() or prev hook      |
|    |                                                    |
|    v                                                    |
|  PlannedStmt returned to executor                       |
+---------------------------------------------------------+
```

### Crate Dependencies

```
ra-pg-extension
  |-- pgrx 0.17.0 (PostgreSQL extension framework)
  |-- ra-core (RelExpr, Cost, Statistics, FactsProvider)
  |-- ra-engine (e-graph optimizer)
  |-- ra-hardware (hardware detection for cost tuning)
  |-- serde / serde_json (configuration)
  |-- tracing (structured logging)
```

### Source Modules

| Module              | Purpose                                           |
|---------------------|---------------------------------------------------|
| `lib.rs`            | Extension entry point, `_PG_init()`, GUC/hook setup |
| `planner_hook.rs`   | Planner hook: intercepts queries, runs optimization pipeline |
| `query_parser.rs`   | Converts PostgreSQL `Query` parse trees to Ra `RelExpr` |
| `stats_bridge.rs`   | Reads `pg_class`, `pg_statistic`, `pg_constraint` via syscache |
| `cost_mapper.rs`    | Calibrates Ra multi-component costs to PG startup/total costs |
| `plan_converter.rs` | Extracts plan advice (join order, methods, scans) from `RelExpr` |
| `pg_constants.rs`   | PostgreSQL cost defaults, GUC names, type/operator OIDs |
| `extension_state.rs`| GUC registration, hardware profile, per-query state |
| `integration_tests.rs` | pgrx integration tests (CTEs, windows, set ops, FKs) |

### Plan Advice Strategy

Rather than constructing PostgreSQL `Plan` nodes directly (which
requires deep C interop with version-specific struct layouts), the
extension uses an **advice-based approach**:

1. Extract plan advice from the optimized `RelExpr`:
   - Join order (left-to-right DFS of join tree)
   - Join methods (hash, merge, nested loop)
   - Scan strategies (sequential, index, bitmap)

2. Temporarily adjust PostgreSQL GUC parameters to bias the standard
   planner toward the advised plan:
   - `enable_hashjoin`, `enable_mergejoin`, `enable_nestloop`
   - `enable_seqscan`, `enable_indexscan`, `enable_bitmapscan`
   - `random_page_cost` (hardware-aware: SSD=1.0, HDD=4.0)

3. Call `standard_planner()` with the adjusted parameters.

4. Restore original GUC values (RAII via `SavedPlannerGucs`).

This approach is more maintainable across PostgreSQL versions and avoids
crashes from incorrect Plan node construction.

The advice can also be formatted for `pg_hint_plan` compatibility:
```sql
/*+ Leading(orders customers) HashJoin(customers) SeqScan(orders) */
```

### Statistics Bridge

The `stats_bridge` module reads PostgreSQL catalog data without using
SPI (which is forbidden inside planner hooks). All access goes through
`SearchSysCache*` and direct catalog scans:

| Source                  | Data Extracted                              |
|-------------------------|---------------------------------------------|
| `pg_class` (RELOID)    | `reltuples`, `relpages`, `relallvisible`, `relnatts` |
| `pg_statistic` (STATRELATTINH) | `stadistinct`, `stanullfrac`, `stawidth`, correlation, MCV values/frequencies, histogram bounds |
| `pg_attribute` (ATTNUM) | Column names, dropped column detection      |
| `pg_index` (INDEXRELID) | Index type (btree/hash/gin/gist/spgist/brin), uniqueness, columns, size |
| `pg_am`                 | Access method name resolution               |
| `pg_constraint`         | Foreign key relationships (conkey/confkey arrays) |

PostgreSQL-specific MVCC statistics are also tracked:
- **HOT update ratio** -- fraction of heap-only-tuple updates
- **Dead tuple ratio** -- estimated from `relallvisible` vs `relpages`
- **Bloat factor** -- `size_on_disk / expected_data_size`

### Cost Calibration

Ra uses a multi-component cost model (CPU, I/O, network, memory).
PostgreSQL uses a single-number model (startup cost + total cost).
The `CostCalibration` struct bridges these:

```
PG total = Ra.cpu * cpu_factor + Ra.io * io_factor + Ra.network * network_factor
```

Default factors use PostgreSQL's standard cost parameters:
- `cpu_factor` = 0.01 (`cpu_tuple_cost`)
- `io_factor` = 1.0 (`seq_page_cost`)
- `network_factor` = 0.5 (heuristic, no PG equivalent)

The calibration tracks estimation errors over time via running mean
absolute percentage error, enabling future self-tuning.

## Installation

### Prerequisites

- PostgreSQL 13--18 (development headers required)
- Rust toolchain (1.85.0+)
- pgrx 0.17.0 (`cargo install cargo-pgrx --version 0.17.0`)

### Build

```bash
cd crates/ra-pg-extension

# Initialize pgrx for your PostgreSQL version
cargo pgrx init --pg17 /usr/bin/pg_config

# Build the extension
cargo pgrx package --pg-config /usr/bin/pg_config
```

### Install

```bash
# Install the shared library and SQL files
cargo pgrx install --pg-config /usr/bin/pg_config

# Or manually copy to the PostgreSQL extension directory
cp target/release/ra_pg_extension.so $(pg_config --pkglibdir)/
cp sql/ra_pg_extension--0.1.0.sql $(pg_config --sharedir)/extension/
```

### Configure PostgreSQL

Add to `postgresql.conf`:

```ini
shared_preload_libraries = 'ra_pg_extension'
```

Restart PostgreSQL, then verify:

```sql
SHOW ra_planner.enabled;  -- should return 'on'
```

## Configuration

All parameters are GUC variables, settable per-session or globally:

| Parameter                    | Type    | Default | Range     | Description                           |
|------------------------------|---------|---------|-----------|---------------------------------------|
| `ra_planner.enabled`        | bool    | `on`    | on/off    | Master switch for Ra optimization     |
| `ra_planner.min_confidence` | float   | `0.9`   | 0.0--1.0  | Minimum confidence to apply a plan    |
| `ra_planner.log_decisions`  | bool    | `off`   | on/off    | Log optimizer decisions to PG log     |
| `ra_planner.max_relations`  | integer | `12`    | 1--100    | Max relations before fallback to PG   |

### Examples

```sql
-- Disable Ra for this session
SET ra_planner.enabled = off;

-- Lower confidence threshold (more aggressive optimization)
SET ra_planner.min_confidence = 0.7;

-- Enable decision logging for debugging
SET ra_planner.log_decisions = on;

-- Allow larger join graphs
SET ra_planner.max_relations = 20;
```

### Confidence Score

The confidence score determines whether Ra's optimized plan is applied.
It combines two factors:

- **Improvement ratio** (70% weight): How much better Ra's estimated
  cost is compared to the original plan
- **Statistics coverage** (30% weight): What fraction of tables in the
  query have catalog statistics available

Plans with confidence below `ra_planner.min_confidence` fall back to
PostgreSQL's standard planner.

## Debugging and Observability

### Decision Logging

Enable `ra_planner.log_decisions` to see Ra's decisions in the
PostgreSQL log:

```sql
SET ra_planner.log_decisions = on;

-- Run a query
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;
```

Log output examples:

```
LOG:  ra_planner: applied RA plan (confidence: 0.95, relations: 2): SELECT * FROM orders o JOIN...
LOG:  ra_planner: low confidence 0.45 < 0.90, using PG planner: SELECT * FROM...
LOG:  ra_planner: skipping query with 15 relations (max: 12): SELECT ...
LOG:  ra_planner: unsupported query shape, using PG planner: SELECT ...
WARNING:  ra_planner: optimization failed (Optimizer error: ...), using PG planner: ...
WARNING:  ra_planner: caught panic in planner hook, falling back to standard planner
```

### Checking Extension Status

```sql
-- Verify extension is loaded
SHOW ra_planner.enabled;

-- Check current settings
SHOW ra_planner.min_confidence;
SHOW ra_planner.max_relations;
SHOW ra_planner.log_decisions;
```

### EXPLAIN Analysis

Compare plans with and without Ra:

```sql
-- With Ra optimization
SET ra_planner.enabled = on;
EXPLAIN (ANALYZE, BUFFERS) SELECT ...;

-- Without Ra optimization
SET ra_planner.enabled = off;
EXPLAIN (ANALYZE, BUFFERS) SELECT ...;
```

## Supported SQL Features

### Fully Supported

| Feature              | Notes                                        |
|----------------------|----------------------------------------------|
| SELECT queries       | All standard SELECT patterns                 |
| JOINs                | INNER, LEFT, RIGHT, FULL, SEMI, ANTI, CROSS |
| WHERE filters        | All expression types                         |
| GROUP BY / HAVING    | With aggregate functions                     |
| ORDER BY             | ASC/DESC with NULLS FIRST/LAST              |
| LIMIT / OFFSET       | Standard and PostgreSQL syntax               |
| DISTINCT             | Full support                                 |
| Aggregates           | COUNT, SUM, AVG, MIN, MAX, STDDEV, VARIANCE, STRING_AGG, ARRAY_AGG |
| Window functions     | ROW_NUMBER, RANK, DENSE_RANK, PERCENT_RANK, NTILE, LAG, LEAD, FIRST_VALUE, LAST_VALUE, NTH_VALUE |
| Window frames        | ROWS, RANGE, GROUPS with all bound types     |
| CTEs (WITH)          | Non-recursive and recursive                  |
| Cycle detection      | CYCLE clause in recursive CTEs               |
| Set operations       | UNION, INTERSECT, EXCEPT (with ALL)          |
| Subqueries           | In FROM clause, as expressions (limited)     |
| VALUES               | Inline value lists                           |
| CASE expressions     | Simple and searched CASE                     |
| Type coercions       | RelabelType, CoerceViaIO, CoerceToDomain     |
| Operators            | All standard comparison, arithmetic, text ops across int2/int4/int8/float4/float8/numeric/text/date/timestamp |

### Passed Through (Not Optimized)

| Feature             | Behavior                                      |
|---------------------|-----------------------------------------------|
| INSERT/UPDATE/DELETE| Passed directly to standard planner           |
| DDL (CREATE, DROP)  | Passed directly to standard planner           |
| Utility statements  | Passed directly to standard planner           |
| Queries > max_relations | Passed directly to standard planner       |

### Limitations

| Feature                    | Current Status                           |
|----------------------------|------------------------------------------|
| Correlated subqueries      | Represented as placeholders              |
| NUMERIC constant parsing   | Approximated as 0.0                      |
| FieldSelect (record access)| Base expression preserved, field info lost|
| Full stats bridge          | Statistics gathered but FactsProvider uses defaults |

## Performance Benchmarks

No formal benchmarks have been run yet. The following template is ready
for results when available.

### TPC-H Queries (Scale Factor 1)

| Query | PostgreSQL (ms) | Ra (ms) | Speedup | Notes          |
|-------|----------------|---------|---------|----------------|
| Q1    | TBD            | TBD     | TBD     |                |
| Q2    | TBD            | TBD     | TBD     |                |
| Q3    | TBD            | TBD     | TBD     |                |
| Q4    | TBD            | TBD     | TBD     |                |
| Q5    | TBD            | TBD     | TBD     |                |
| Q6    | TBD            | TBD     | TBD     |                |
| Q7    | TBD            | TBD     | TBD     |                |
| Q8    | TBD            | TBD     | TBD     |                |
| Q9    | TBD            | TBD     | TBD     |                |
| Q10   | TBD            | TBD     | TBD     |                |

### Join Reordering (Star Schema)

| Relations | PostgreSQL (ms) | Ra (ms) | Speedup |
|-----------|----------------|---------|---------|
| 3-way     | TBD            | TBD     | TBD     |
| 5-way     | TBD            | TBD     | TBD     |
| 8-way     | TBD            | TBD     | TBD     |
| 12-way    | TBD            | TBD     | TBD     |

### Expected Strengths

Based on the optimization capabilities, Ra should perform well on:

- **Multi-way joins** (5+ tables): Ra's e-graph explores more join
  orderings than PostgreSQL's GEQO threshold (default 12 tables)
- **Join method selection**: Hardware-aware cost model can detect SSD
  and adjust `random_page_cost`, favoring index scans when appropriate
- **Complex aggregation queries**: Ra's rule-based optimizer applies
  predicate pushdown and projection pruning

### Expected Weaknesses

- **Simple queries** (1--2 tables): Optimization overhead without
  meaningful improvement
- **Queries with correlated subqueries**: Not fully supported in the
  parser, falls back to PostgreSQL
- **Very large join graphs** (>12 tables): Bails out by default to
  avoid excessive e-graph saturation time

## Development Roadmap

### Completed

- [x] pgrx extension skeleton with PG 13--18 feature flags
- [x] Planner hook registration and chaining
- [x] Query parser: full SELECT coverage (joins, aggregates, windows,
      CTEs, set operations)
- [x] Statistics bridge: syscache-based catalog access (no SPI)
- [x] Cost calibration between Ra and PostgreSQL models
- [x] Plan advice extraction and GUC-based application
- [x] Hardware-aware cost parameters
- [x] Foreign key discovery from pg_constraint
- [x] MVCC statistics (dead tuples, bloat)
- [x] Confidence-based plan selection
- [x] Integration tests

### Planned

- [ ] Full `FactsProvider` implementation mapping pg_stats to Ra types
- [ ] Direct `PlannedStmt` construction (bypass GUC manipulation)
- [ ] Improvement factor estimation from actual cost comparison
- [ ] Plan caching across identical query shapes
- [ ] Adaptive calibration from execution feedback
- [ ] TPC-H benchmark suite
- [ ] NUMERIC constant decoding via `numeric_out`
- [ ] Correlated subquery support
- [ ] `pg_plan_advice` GUC integration (PostgreSQL 19+)
- [ ] Extension packaging for pgxn/apt/yum

## Testing

### Unit Tests

Run pure Rust unit tests (no PostgreSQL required):

```bash
cd crates/ra-pg-extension
cargo test --no-default-features
```

Tests cover:
- Cost calibration arithmetic
- PostgreSQL array parsing
- Index definition parsing
- n_distinct encoding interpretation
- Operator/aggregate/join type OID mapping
- Plan advice extraction and formatting
- Table name extraction and relation counting

### Integration Tests

Run pgrx integration tests (requires PostgreSQL):

```bash
cd crates/ra-pg-extension
cargo pgrx test pg17
```

Integration tests verify end-to-end query execution:
- CTEs with window functions
- Recursive CTEs with cycle detection
- Set operations with aggregation
- Foreign key join optimization
- Multi-level CTE nesting
- Window frames (ROWS BETWEEN ... AND ...)
- LAG/LEAD with CTEs
- All features combined (CTEs + windows + set ops + FKs)

## Further Reading

- [Database Adapters](database-adapters.md) -- General database
  integration architecture
- [Platform Architecture](../features/platform-architecture.md) --
  System-wide crate relationships
- [Cost Models](../features/cost-models.md) -- Ra's multi-component
  cost model
