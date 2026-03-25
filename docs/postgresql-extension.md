# PostgreSQL Extension

Ra ships a native PostgreSQL extension (`ra_pg_extension`) that hooks
into the query planner, optimizing SELECT queries transparently using
Ra's 1,327+ transformation rules and equality saturation engine.

## Overview

The extension intercepts queries at plan time via PostgreSQL's
`planner_hook`, converts them to Ra's relational algebra
representation, optimizes using the e-graph engine, and guides
PostgreSQL's standard planner toward the optimal plan through
cost manipulation.

No application changes are needed. Existing queries, ORMs, connection
poolers, and monitoring tools continue to work.

### Architecture

```
PostgreSQL Backend Process
+-------------------------------------------------------+
|  SQL Query                                             |
|    |                                                   |
|    v                                                   |
|  Parse/Analyze (standard PostgreSQL)                   |
|    |                                                   |
|    v                                                   |
|  planner_hook  <-- ra_planner_hook() intercepts here   |
|    |                                                   |
|    +---> Is ra_planner.enabled = on?                   |
|    |     Is it a SELECT?                               |
|    |     Relations <= ra_planner.max_relations?         |
|    |                                                   |
|    +---> [YES] Ra Optimization Pipeline                |
|    |     1. query_parser: Query -> RelExpr             |
|    |     2. stats_bridge: pg_class/pg_statistic -> Stats|
|    |     3. ra-engine: e-graph optimization            |
|    |     4. cost_mapper: Ra Cost -> PG Cost            |
|    |     5. plan_converter: RelExpr -> plan advice     |
|    |     6. Confidence check >= min_confidence?        |
|    |        [YES] Apply advice via GUC manipulation    |
|    |        [NO]  Fall back to PG planner              |
|    |                                                   |
|    +---> [NO] call standard_planner() or prev hook     |
|    |                                                   |
|    v                                                   |
|  PlannedStmt returned to executor                      |
+-------------------------------------------------------+
```

## Installation

### Prerequisites

- PostgreSQL 13-18 (development headers required)
- Rust 1.85.0+
- pgrx 0.17.0

```bash
# Install pgrx CLI
cargo install cargo-pgrx --version 0.17.0
```

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
# Install shared library and SQL files
cargo pgrx install --pg-config /usr/bin/pg_config
```

Or install manually:

```bash
cp target/release/ra_pg_extension.so $(pg_config --pkglibdir)/
cp sql/ra_pg_extension--0.1.0.sql $(pg_config --sharedir)/extension/
cp ra_pg_extension.control $(pg_config --sharedir)/extension/
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

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `ra_planner.enabled` | bool | `on` | Master switch for Ra optimization |
| `ra_planner.min_confidence` | float | `0.9` | Minimum confidence to apply a plan (0.0-1.0) |
| `ra_planner.log_decisions` | bool | `off` | Log optimizer decisions to PG log |
| `ra_planner.max_relations` | integer | `12` | Max relations before fallback to native planner (1-100) |

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

## Usage

Once installed and loaded, Ra optimizes queries transparently:

```sql
-- Enable Ra optimization for the current session
SET ra_planner.enabled = on;

-- Run a query; Ra optimizes it transparently
SELECT *
FROM orders o
  JOIN customers c ON o.customer_id = c.id
  JOIN products p ON o.product_id = p.id
WHERE o.order_date > '2025-01-01'
  AND c.country = 'US';

-- Compare plans with and without Ra
SET ra_planner.enabled = on;
EXPLAIN (ANALYZE, BUFFERS) SELECT ...;

SET ra_planner.enabled = off;
EXPLAIN (ANALYZE, BUFFERS) SELECT ...;
```

### Confidence Score

The confidence score determines whether Ra's optimized plan replaces
the default. It combines two factors:

- **Improvement ratio** (70% weight): How much better Ra's estimated
  cost is compared to the original plan
- **Statistics coverage** (30% weight): What fraction of tables have
  catalog statistics available

Plans below `ra_planner.min_confidence` fall back to PostgreSQL's
native planner.

### Fallback Behavior

The extension falls back to the standard planner when:

1. `ra_planner.enabled` is `off`
2. The query references more tables than `ra_planner.max_relations`
3. Ra's confidence is below `ra_planner.min_confidence`
4. Ra encounters an unsupported SQL feature
5. An internal error occurs during optimization

The extension never makes queries worse -- at worst it has no effect.

## Plan Advice Strategy

Rather than constructing PostgreSQL `Plan` nodes directly, the
extension uses an advice-based approach for maintainability across
PostgreSQL versions:

1. Extract plan advice from the optimized `RelExpr` (join order,
   join methods, scan strategies)
2. Temporarily adjust PostgreSQL GUC parameters to bias the standard
   planner toward the advised plan (`enable_hashjoin`,
   `enable_mergejoin`, `enable_seqscan`, `random_page_cost`, etc.)
3. Call `standard_planner()` with the adjusted parameters
4. Restore original GUC values (RAII via `SavedPlannerGucs`)

Advice can also be formatted for `pg_hint_plan` compatibility:

```sql
/*+ Leading(orders customers) HashJoin(customers) SeqScan(orders) */
```

## Statistics Bridge

The extension reads PostgreSQL catalog data directly through
`SearchSysCache*` (no SPI, which is forbidden in planner hooks):

| Source | Data Extracted |
|--------|----------------|
| `pg_class` | `reltuples`, `relpages`, `relallvisible` |
| `pg_statistic` | `stadistinct`, `stanullfrac`, `stawidth`, correlation, MCV, histogram |
| `pg_attribute` | Column names, dropped column detection |
| `pg_index` | Index type, uniqueness, columns, size |
| `pg_constraint` | Foreign key relationships |

Additional MVCC statistics tracked:

- **HOT update ratio** -- fraction of heap-only-tuple updates
- **Dead tuple ratio** -- estimated from visibility map
- **Bloat factor** -- `size_on_disk / expected_data_size`

## Supported SQL Features

### Fully Supported

| Feature | Notes |
|---------|-------|
| SELECT queries | All standard SELECT patterns |
| JOINs | INNER, LEFT, RIGHT, FULL, SEMI, ANTI, CROSS |
| WHERE filters | All expression types |
| GROUP BY / HAVING | With aggregate functions |
| ORDER BY | ASC/DESC with NULLS FIRST/LAST |
| LIMIT / OFFSET | Standard and PostgreSQL syntax |
| DISTINCT | Full support |
| Aggregates | COUNT, SUM, AVG, MIN, MAX, STDDEV, VARIANCE, STRING_AGG, ARRAY_AGG |
| Window functions | ROW_NUMBER, RANK, DENSE_RANK, NTILE, LAG, LEAD, FIRST_VALUE, LAST_VALUE, NTH_VALUE |
| CTEs (WITH) | Non-recursive and recursive (with CYCLE) |
| Set operations | UNION, INTERSECT, EXCEPT (with ALL) |
| CASE expressions | Simple and searched CASE |

### Passed Through (Not Optimized)

INSERT, UPDATE, DELETE, DDL, and utility statements pass through
directly to PostgreSQL's standard planner without modification.

### Current Limitations

- Correlated subqueries are represented as placeholders
- NUMERIC constants are approximated as 0.0
- FieldSelect nodes lose field access information
- Statistics are gathered but the full FactsProvider mapping is not
  yet complete

## Debugging

### Decision Logging

```sql
SET ra_planner.log_decisions = on;

SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;
```

Log output:

```
LOG:  ra_planner: applied RA plan (confidence: 0.95, relations: 2)
LOG:  ra_planner: low confidence 0.45 < 0.90, using PG planner
LOG:  ra_planner: skipping query with 15 relations (max: 12)
WARNING:  ra_planner: optimization failed, using PG planner
```

### Checking Extension Status

```sql
SHOW ra_planner.enabled;
SHOW ra_planner.min_confidence;
SHOW ra_planner.max_relations;
```

## Performance Characteristics

| Query Type | Additional Planning Time | When Beneficial |
|------------|------------------------|-----------------|
| Single table | < 0.1ms | Not beneficial (overhead only) |
| 2-5 joins | 1-5ms | Complex join ordering |
| 6-12 joins | 5-20ms | Star schema, multi-way joins |
| Memory | ~10MB shared | Per-backend allocation |

Best suited for OLAP workloads with complex joins. For OLTP workloads
dominated by point lookups, disable the extension or raise the
confidence threshold.

## Testing

### Unit Tests (no PostgreSQL required)

```bash
cd crates/ra-pg-extension
cargo test --no-default-features
```

### Integration Tests (requires PostgreSQL)

```bash
cd crates/ra-pg-extension
cargo pgrx test pg17
```

## Source Modules

| Module | Purpose |
|--------|---------|
| `lib.rs` | Extension entry point, `_PG_init()` |
| `planner_hook.rs` | Planner hook, optimization pipeline |
| `query_parser.rs` | PostgreSQL Query trees to Ra RelExpr |
| `stats_bridge.rs` | Catalog access (pg_class, pg_statistic, pg_constraint) |
| `cost_mapper.rs` | Ra cost model to PostgreSQL cost calibration |
| `plan_converter.rs` | Plan advice extraction (join order, methods, scans) |
| `pg_constants.rs` | PostgreSQL cost defaults, OIDs |
| `extension_state.rs` | GUC registration, hardware profile, per-query state |

## PostgreSQL v19 Integration

PostgreSQL v19 introduces `pg_plan_advice`, a declarative plan advice
module with an advisor hook API. When available, Ra registers as an
advisor and supplies advice through the official API rather than GUC
manipulation. See [RFC 0002](../rfcs/0002-pgrx-extension.md) and
[RFC 0003](../rfcs/0003-plan-advice-integration.md) for details.

## Further Reading

- [Database Adapters](integrations/database-adapters.md) -- General
  integration architecture
- [Cost Models](guides/cost-models.md) -- Ra's multi-component cost
  model
- [RFC 0002: pgrx Extension](../rfcs/0002-pgrx-extension.md) --
  Design document
- [PostgreSQL Integration](integrations/postgresql.md) -- Detailed
  integration documentation
