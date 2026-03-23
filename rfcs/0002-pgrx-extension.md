# RFC 0002: pgrx PostgreSQL Extension

- Start Date: 2026-03-20
- Author: RA Contributors
- Status: Draft
- RFC Number: 0002
- Tracking: Phase 4 of deployment plan

---

## Summary

Build a native PostgreSQL extension using the pgrx framework that embeds
the Ra optimizer directly inside PostgreSQL. The extension intercepts
query planning via `planner_hook`, converts PostgreSQL's internal
structures to Ra's relational algebra representation, applies 1,327+
transformation rules, and returns improved plans to the executor. When
PostgreSQL v19 is available, the extension additionally integrates with
`pg_plan_advice` to supply advice through the committed advisor hook API.

## Motivation

### Why integrate into PostgreSQL directly?

Ra's optimizer currently runs as an external tool. Users must export
queries, run optimization separately, and interpret results. This
workflow is impractical for production use. A PostgreSQL extension
eliminates this friction by optimizing queries transparently at plan
time.

### Benefits of in-process integration

**Zero serialization overhead.** An external optimizer must serialize
query text, parse it, optimize, serialize the result, and send it back.
The planner hook approach operates on in-memory structures within the
PostgreSQL backend process, avoiding all serialization costs.

**Access to PostgreSQL statistics.** The extension reads `pg_stats`,
`pg_class`, and `pg_statistic` directly through SPI, obtaining
histogram data, correlation values, and distinct-value counts that an
external tool would need a separate connection to retrieve.

**Transparent to applications.** No application code changes required.
Existing queries, ORMs, connection poolers, and monitoring tools
continue to work. The optimization happens below the SQL interface.

**Feedback loop.** The extension captures `EXPLAIN ANALYZE` data after
execution and feeds it back to calibrate Ra's cost model, improving
future optimization decisions over time.

### Use cases

- **Production OLAP workloads.** Complex analytical queries with many
  joins benefit from Ra's advanced join reordering, predicate pushdown,
  and cost-based access path selection.
- **A/B testing.** Compare Ra-optimized plans against PostgreSQL's
  native optimizer on the same workload to validate improvements before
  full deployment.
- **Plan regression prevention.** Detect when PostgreSQL's planner
  chooses a worse plan after ANALYZE or version upgrades, and steer it
  back to a known-good plan shape.
- **Cost model calibration.** Collect execution feedback across a fleet
  of PostgreSQL instances to build accurate cost models for Ra's
  optimizer.

## Guide-level explanation

### Installation

Build and install the extension:

```bash
# Build for the PostgreSQL version installed on this system
cargo pgrx install --release

# Or build for a specific version
cargo pgrx install --release --pg-config /usr/lib/postgresql/17/bin/pg_config
```

Load the extension in PostgreSQL:

```sql
CREATE EXTENSION ra_planner;
```

For system-wide activation across all databases, add the extension to
`shared_preload_libraries`:

```ini
# postgresql.conf
shared_preload_libraries = 'ra_pg_extension'
```

### Basic usage

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

-- Compare plans side by side
EXPLAIN (COSTS, ANALYZE)
SELECT * FROM orders JOIN customers USING (customer_id)
WHERE order_date > '2025-01-01';
```

Ra intercepts the query, builds a `RelExpr` representation, applies
optimization rules (join reordering, predicate pushdown, scan method
selection), and either replaces the plan or injects advice that steers
PostgreSQL's planner toward the better plan shape.

### Configuration

The extension exposes GUC variables for runtime control:

| GUC Parameter | Type | Default | Description |
|---|---|---|---|
| `ra_planner.enabled` | bool | `on` | Master switch for Ra optimization |
| `ra_planner.min_confidence` | float | `0.9` | Minimum confidence to apply advice (0.0-1.0) |
| `ra_planner.log_decisions` | bool | `off` | Log optimizer decisions to PostgreSQL log |
| `ra_planner.max_relations` | int | `12` | Maximum join count before fallback to native planner |

All GUCs are `Userset` context, meaning any session can change them
without superuser privileges.

```sql
-- Conservative: only apply advice when Ra is highly confident
SET ra_planner.min_confidence = 0.95;

-- Debug: log all optimizer decisions
SET ra_planner.log_decisions = on;

-- Handle large star-schema queries
SET ra_planner.max_relations = 20;
```

### Fallback behavior

The extension always falls back to PostgreSQL's native planner when:

1. `ra_planner.enabled` is `off`
2. The query references more tables than `ra_planner.max_relations`
3. Ra's optimization confidence is below `ra_planner.min_confidence`
4. Ra encounters an unsupported SQL feature (e.g., custom scan providers)
5. An internal error occurs during optimization

This guarantees that the extension never makes queries worse -- at worst
it has no effect.

## Reference-level explanation

### Architecture

```
PostgreSQL Query
    |
    v
planner_hook (ra_planner_hook)
    |
    v
1. Query -> Ra RelExpr conversion     [plan_converter.rs]
2. pg_stats -> Ra Statistics           [stats_bridge.rs]
3. Ra optimizer (1,327+ rules)         [ra-engine]
4. Ra plan -> advice string            [plan_converter.rs]
5. Cost calibration                    [cost_mapper.rs]
    |
    v
PostgreSQL Planner (with advice applied)
    |
    v
PostgreSQL Executor
    |
    v
Feedback loop (execution stats -> cost model calibration)
```

### Crate structure

The extension lives in `crates/ra-pg-extension/` and depends on:

```toml
[dependencies]
pgrx = "0.17"
ra-core = { path = "../ra-core" }
```

Feature flags select the target PostgreSQL version:

```toml
[features]
default = ["pg17"]
pg14 = ["pgrx/pg14"]
pg15 = ["pgrx/pg15"]
pg16 = ["pgrx/pg16"]
pg17 = ["pgrx/pg17"]
```

### Components

#### 1. Planner Hook (`planner_hook.rs`)

The main entry point. Intercepts every query that passes through the
PostgreSQL planner:

```rust
#[pg_guard]
unsafe extern "C" fn ra_planner_hook(
    parse: *mut pg_sys::Query,
    query_string: *const c_char,
    cursor_options: i32,
    bound_params: *mut pg_sys::ParamListInfoData,
) -> *mut pg_sys::PlannedStmt {
    // 1. Check if extension is enabled (GUC fast path)
    // 2. Count rtable entries; bail if above max_relations
    // 3. Extract table names from range table
    // 4. Gather pg_stats for referenced tables
    // 5. Build Ra RelExpr from query string
    // 6. Run Ra optimizer with gathered statistics
    // 7. If confidence >= threshold, apply advice
    // 8. Fall through to standard_planner or previous hook
}
```

Hook chaining is preserved: the extension saves and chains to any
previously installed `planner_hook`, enabling coexistence with other
extensions (e.g., `pg_hint_plan`, `citus`).

#### 2. Statistics Bridge (`stats_bridge.rs`)

Reads PostgreSQL catalog views and converts to Ra's statistics format:

- **`pg_class.reltuples`** -> Ra `Statistics.row_count`
- **`pg_stats.n_distinct`** -> Ra `ColumnStats.distinct_count`
  (interprets PostgreSQL's positive/negative encoding)
- **`pg_stats.null_frac`** -> Ra `ColumnStats.null_fraction`
- **`pg_stats.avg_width`** -> Ra `ColumnStats.avg_length`
- **`pg_stats.correlation`** -> Ra index scan cost adjustment
- **`pg_stats.most_common_vals/freqs`** -> Ra MCV list (future)
- **`pg_stats.histogram_bounds`** -> Ra histogram (future)

Statistics are gathered once per planning cycle via SPI queries and
cached in `RaOptimizerState`.

The `n_distinct` encoding follows PostgreSQL conventions:
- Positive values: absolute distinct-value count
- Negative values: fraction of table size (e.g., -1.0 = all rows
  distinct, -0.5 = half as many distinct values as rows)

#### 3. Plan Converter (`plan_converter.rs`)

Bidirectional conversion between Ra's `RelExpr` and PostgreSQL's plan
structures. Rather than constructing raw `Plan` nodes (which requires
deep C interop and is fragile across PG versions), the converter
extracts advice from the optimized `RelExpr`:

**Ra RelExpr -> Advice extraction:**
- Walk the join tree left-to-right DFS to extract join ordering
- Map `JoinType` variants to physical join method preferences
- Map scan nodes to scan strategy preferences
- Emit `PlanAdviceSet` containing `JOIN_ORDER`, join methods, and
  scan methods

**Output formats:**
- `pg_plan_advice` format: `JOIN_ORDER(o c) HASH_JOIN(c) SEQ_SCAN(o)`
- `pg_hint_plan` format: `/*+ Leading(o c) HashJoin(c) SeqScan(o) */`

Supported advice types:

| Ra Operator | pg_plan_advice Tag | pg_hint_plan Hint |
|---|---|---|
| Join (Inner/Outer) | `HASH_JOIN(rel)` | `HashJoin(rel)` |
| Join (Semi/Anti) | `NESTED_LOOP(rel)` | `NestLoop(rel)` |
| Join tree order | `JOIN_ORDER(a b c)` | `Leading(a b c)` |
| Sequential scan | `SEQ_SCAN(rel)` | `SeqScan(rel)` |
| Index scan | `INDEX_SCAN(rel idx)` | `IndexScan(rel idx)` |
| Bitmap scan | `BITMAP_HEAP_SCAN(rel)` | `BitmapScan(rel)` |

#### 4. Cost Mapper (`cost_mapper.rs`)

Bridges Ra's multi-component cost model to PostgreSQL's
startup-cost/total-cost model:

```
PostgreSQL cost = Ra.cpu * cpu_factor
                + Ra.io  * io_factor
                + Ra.net * network_factor
```

Default calibration factors align with PostgreSQL defaults:
- `cpu_factor = 0.01` (maps to `cpu_tuple_cost`)
- `io_factor = 1.0` (maps to `seq_page_cost`)
- `network_factor = 0.5`

Startup/total cost decomposition uses operator-specific fractions:

| Operator | Startup Fraction |
|---|---|
| Sequential scan | 0.0 |
| Index scan | 0.01 |
| Sort | 0.9 |
| Hash join (build) | 0.5 |
| Merge join | 0.4 |
| Nested loop | 0.0 |
| Hash aggregate | 0.9 |
| Sorted aggregate | 0.01 |

The mapper tracks estimation accuracy over time via online mean
absolute percentage error, enabling automatic recalibration.

#### 5. Extension State (`extension_state.rs`)

Per-query state that flows between hook invocations:

```rust
pub struct RaOptimizerState {
    pub query_string: String,
    pub optimized_plan: Option<RelExpr>,
    pub ra_cost: Option<Cost>,
    pub statistics: Vec<(String, Statistics)>,
    pub confidence: f64,
    pub plan_applied: bool,
}
```

GUC registration happens at `_PG_init()` time and exposes all
configuration variables to `SET`/`SHOW`/`ALTER SYSTEM`.

### PostgreSQL v19 integration

PostgreSQL v19 introduces infrastructure that significantly improves
extension integration:

**Committed hooks (October 2025):**
- `planner_setup_hook` -- called after `PlannerGlobal` initialization
- `planner_shutdown_hook` -- called before `PlannerGlobal` destruction
- `extendplan.h` -- private extension state in `PlannerGlobal`,
  `PlannerInfo`, and `RelOptInfo`
- `ExplainState` extensibility -- custom EXPLAIN options

**Committed module (March 12, 2026):**
- `pg_plan_advice` -- declarative plan advice mini-language with 19
  advice tag types
- Advisor hook API (`pg_plan_advice_add_advisor`) for programmatic
  advice injection
- `pg_stash_advice` -- persistent advice storage in DSM, auto-applied
  by query ID

The extension detects v19 at compile time and uses the richer API when
available:

```rust
// v19+: Register as a pg_plan_advice advisor
#[cfg(feature = "pg19")]
fn register_advisor() {
    let add_advisor_fn = load_external_function(
        "pg_plan_advice",
        "pg_plan_advice_add_advisor",
        true, null_mut()
    );
    (*add_advisor_fn)(ra_advisor_callback);
}

// v19+: Advisor callback receives full query context
#[cfg(feature = "pg19")]
extern "C" fn ra_advisor_callback(
    glob: *mut PlannerGlobal,
    parse: *mut Query,
    query_string: *const c_char,
    cursor_options: c_int,
    es: *mut ExplainState,
) -> *mut c_char {
    // Run Ra optimizer, return advice string or NULL to defer
}
```

For PostgreSQL 14-18, the extension uses the existing `planner_hook`
and can optionally emit `pg_hint_plan` compatible hints.

### Build and packaging

```bash
# Development build and test
cargo pgrx test pg17

# Release build
cargo pgrx package --pg-config /usr/lib/postgresql/17/bin/pg_config

# Install directly
cargo pgrx install --release
```

The `cargo pgrx package` command produces:
- Shared library (`.so`/`.dylib`)
- Extension control file (`ra_pg_extension.control`)
- SQL migration scripts (`ra_pg_extension--0.1.0.sql`)

CI produces RPM and DEB packages for PostgreSQL 15, 16, 17 using a
Dockerized build matrix.

### Error handling

The extension follows a defensive strategy:

- **SPI failures:** Log a warning and fall back to the standard
  planner. Statistics gathering failures never cause query failures.
- **Optimizer panics:** Caught by pgrx `#[pg_guard]` and converted
  to PostgreSQL errors. The query fails with a clear error message
  rather than crashing the backend.
- **Null pointer safety:** Every pointer from PostgreSQL is checked
  before dereference. Range table, query tree, and plan tree access
  all have null guards.
- **Memory management:** Per-query state is allocated in Rust (owned)
  and freed when the planner hook returns. No PostgreSQL `palloc`
  leaks into parent memory contexts.

### Performance considerations

**Planning overhead:**
- Simple queries (single table): < 0.1ms additional planning time
- Medium queries (2-5 joins): 1-5ms additional planning time
- Complex queries (6-12 joins): 5-20ms additional planning time
- Memory: ~10MB shared memory for optimizer state

**When overhead is justified:**
- Complex join queries where Ra finds a better join order (2-10x
  execution speedup)
- Star-schema queries beyond PostgreSQL's DP join threshold (12 tables)
- Queries where Ra detects missing predicate pushdown opportunities
- Repeated queries that benefit from adaptive cost calibration

**When to disable:**
- OLTP workloads dominated by single-table point lookups
- Prepared statements with trivial plans
- Queries already at PostgreSQL's plan optimality

## Drawbacks

**Build complexity.** pgrx requires PostgreSQL development headers and
a compatible Rust toolchain. The extension must be built separately for
each PostgreSQL major version. CI must maintain a build matrix.

**Plan conversion fragility.** PostgreSQL's internal plan structures
change between major versions. The advice-based approach (rather than
direct Plan node construction) mitigates this, but the statistics
bridge still reads catalog views whose schema could change.

**Memory overhead.** Embedding Ra's optimizer in the PostgreSQL backend
process adds memory consumption. Each backend process that activates the
extension allocates optimizer state. In connection-pooling scenarios
with many backends, this multiplies.

**Regression risk.** If Ra makes poor optimization choices, queries may
perform worse. The confidence threshold and fallback mechanism mitigate
this, but edge cases may slip through. The feedback loop helps catch
regressions over time, but initial deployments face a cold-start
problem.

**Superuser requirement.** Installing the extension requires superuser
privileges. In managed PostgreSQL environments (RDS, Cloud SQL), this
may not be available.

**pgrx version coupling.** The extension depends on pgrx, which must
track PostgreSQL internal changes. pgrx version updates may require
code changes in the extension.

## Rationale and alternatives

### Why this design?

The planner hook approach provides maximum control with minimum user
friction:

1. **Transparent operation.** No SQL rewriting, no application changes,
   no middleware required.
2. **In-process.** Zero network overhead, direct access to statistics.
3. **Composable.** Works alongside other planner extensions through
   hook chaining.
4. **Graduated adoption.** Start with advisory mode (confidence
   threshold), graduate to full optimization as trust builds.

### Alternative 1: pg_plan_advice only (RFC 0003)

Supply advice strings without a full planner hook. Use the v19
`pg_plan_advice` API exclusively.

**Pros:**
- Less invasive; PostgreSQL validates all plans
- Safer for production; PostgreSQL makes the final decision
- No direct Plan node manipulation

**Cons:**
- Advice is coarser than full plan control
- Some optimizations cannot be expressed as advice tags
- Requires PostgreSQL v19+ (not yet GA)

**Verdict:** Complementary, not mutually exclusive. RFC 0002 provides
the planner hook infrastructure. RFC 0003 adds the `pg_plan_advice`
integration layer on top. The extension supports both modes.

### Alternative 2: Foreign Data Wrapper (FDW)

Proxy queries through Ra using a Foreign Data Wrapper.

**Pros:**
- Simpler implementation; uses standard FDW API
- No planner hook needed

**Cons:**
- High latency (query text serialization/deserialization)
- Cannot optimize plans for local tables
- Adds a network hop even for local connections

**Verdict:** Rejected. FDW overhead defeats the purpose of transparent
optimization.

### Alternative 3: External optimizer process

Run Ra as a separate daemon. PostgreSQL sends queries via a custom
protocol; Ra returns optimized plans.

**Pros:**
- Isolated process; crashes don't affect PostgreSQL
- Can serve multiple PostgreSQL instances

**Cons:**
- Serialization overhead (query text round-trip)
- Additional deployment complexity (separate process, health checks)
- No direct access to PostgreSQL statistics
- Latency for every query

**Verdict:** Rejected for the primary integration path. May be useful
as a secondary architecture for centralized optimization across a
fleet.

### Alternative 4: pg_hint_plan integration only

Generate `pg_hint_plan` hints and inject them via SQL comments.

**Pros:**
- Works on PostgreSQL 9.6+; widest compatibility
- Mature, well-tested extension

**Cons:**
- Requires rewriting SQL text (prepending hint comments)
- Non-standard hint syntax
- Cannot express all optimization decisions
- Maintenance burden tracking `pg_hint_plan` compatibility

**Verdict:** Supported as a fallback for PostgreSQL 14-18 users, but
not the primary integration path.

### Impact of not doing this

Without a PostgreSQL extension, Ra remains an offline analysis tool.
Users must manually export queries, run optimization, interpret results,
and apply changes. This limits adoption to teams with dedicated database
engineers and excludes the majority of PostgreSQL users who need
transparent optimization.

## Prior art

### PostgreSQL extensions

**pg_hint_plan** -- Community extension that injects optimizer hints
via SQL comments (`/*+ HashJoin(t1 t2) */`). Demonstrates that external
plan guidance is both feasible and valuable in PostgreSQL. Limitation:
requires modifying SQL text.

**Citus** -- Uses planner hooks for distributed query planning. Shows
that planner hook extensions can be production-grade at scale. Citus
replaces the entire planner for distributed queries, similar to the
full-replacement mode in this RFC.

**pg_plan_advice** (PostgreSQL v19) -- Robert Haas's committed
contrib module for declarative plan advice. Provides the official
PostgreSQL mechanism for external optimizer integration. The advisor
hook API (`pg_plan_advice_add_advisor`) is designed for extensions
like Ra. See `research/pg_plan_advice-v19.md` for full analysis.

**Adaptive Query Optimization (Aqo)** -- PostgreSQL extension that
uses machine learning to correct cardinality estimation errors. Similar
feedback loop concept: learn from execution statistics to improve
future plans. Aqo modifies costs rather than injecting plan shapes.

### External optimizers

**Apache Calcite** -- Pluggable optimizer used by Hive, Drill, Flink,
and others. Similar rule-based optimization approach, but runs as an
external service with serialized query representations. Calcite's
adapter pattern is analogous to Ra's dialect system, but Calcite pays
serialization overhead that the in-process extension avoids.

### Commercial databases

**Oracle SQL Plan Management** -- Stores known-good execution plans and
prevents plan regressions. Oracle's approach is more conservative
(replay known plans) versus Ra's approach (actively search for better
plans). Both share the concept of confidence-gated optimization.

**SQL Server Plan Guides** -- Attaches hints to queries by matching
query text. Similar to `pg_plan_advice` but text-matching based.

### Key insights from prior art

1. Confidence thresholds are essential. Aqo, Oracle SPM, and this RFC
   all gate optimization on confidence to prevent regressions.
2. Advice-based integration is safer than plan replacement. Both
   `pg_hint_plan` and `pg_plan_advice` demonstrate that guiding the
   native planner is more robust than replacing it.
3. Feedback loops are necessary for production use. Aqo and Oracle SPM
   both learn from execution; static optimization is insufficient.
4. Hook chaining is mandatory. Extensions must coexist. Citus,
   `pg_hint_plan`, and this extension all preserve previous hooks.

## Unresolved questions

### Design questions (resolve before merge)

- **Which PostgreSQL versions to support at launch?** The extension
  currently builds for PostgreSQL 14-17. Supporting v14 adds maintenance
  burden for an end-of-life version. Recommendation: launch with
  PostgreSQL 15-17 support, add v19 when GA.

- **Should the extension support read-only advisory mode?** A mode
  that logs what it would do without affecting plans. Useful for
  evaluation. Current GUC structure supports this via
  `min_confidence = 1.0` (effectively disabling advice application).

- **How to handle self-joins?** PostgreSQL's range table may contain
  the same table multiple times with different aliases. The advice
  extraction must use aliases (not table names) to disambiguate.
  Current implementation handles this correctly.

### Implementation questions (resolve during development)

- **Histogram and MCV conversion.** The statistics bridge currently
  reads `n_distinct`, `null_frac`, and `avg_width`. Full histogram
  and most-common-values conversion would improve Ra's cardinality
  estimates but adds complexity. Defer to Phase 4.3.

- **pg_plan_advice advisor registration.** The v19 advisor hook API
  uses `load_external_function` to dynamically load `pg_plan_advice`.
  This requires `pg_plan_advice` to be installed. Need a graceful
  fallback when the module is not available.

- **Custom scan provider support.** PostgreSQL allows extensions to
  register custom scan types (e.g., Citus, TimescaleDB). Ra does not
  model these. The extension should detect custom scans and fall back
  to the native planner for those subtrees.

### Out of scope

- **Background worker for proactive optimization.** A daemon that
  monitors `pg_stat_statements` and pre-optimizes slow queries.
  Deferred to a future RFC.
- **Distributed PostgreSQL support.** Integration with Citus or
  PostgreSQL-native partitioning for cross-node optimization.
- **Machine learning cost models.** Replacing calibration factors
  with learned models.

## Future possibilities

### Integration with pg_plan_advice (RFC 0003)

The planner hook extension and `pg_plan_advice` integration are
complementary:

- **Phase 1 (this RFC):** Direct planner hook with cost manipulation
- **Phase 2 (RFC 0003):** Add `pg_plan_advice` advisor registration
  for v19+, using the committed advisor hook API
- **Phase 3:** Background worker that writes optimized advice to
  `pg_stash_advice` for cluster-wide auto-application

### Execution feedback loop

After query execution, capture `EXPLAIN ANALYZE` metrics:
- Actual vs estimated row counts per operator
- Actual vs estimated execution time
- Buffer hit/miss ratios
- I/O timing

Feed these back to calibrate Ra's cost model. Over time, the extension
learns the characteristics of the specific hardware and workload,
producing increasingly accurate cost estimates.

### Statistics learning

Beyond PostgreSQL's built-in statistics, Ra could detect missing
extended statistics and recommend `CREATE STATISTICS` commands:
- Functional dependencies between columns
- Multi-column distinct-value counts
- Expression statistics for computed predicates

### Cost model auto-tuning

Use execution feedback to automatically adjust calibration factors.
When Ra's predicted costs diverge from actual execution costs, adjust
`cpu_factor`, `io_factor`, and `network_factor` using online gradient
descent.

### Index recommendation

Analyze query patterns and recommend index creation:
- Detect sequential scans that would benefit from indexes
- Identify composite index opportunities from multi-predicate queries
- Suggest partial indexes based on common filter patterns

### Extension marketplace distribution

Package the extension for distribution via:
- PGXN (PostgreSQL Extension Network)
- trunk (managed extension installer)
- apt/yum repositories for major Linux distributions
- Docker images with pre-installed extensions

## References

- `research/pg_plan_advice-v19.md` -- PostgreSQL v19 plan advice
  analysis, committed infrastructure, advisor hook API
- `research/postgres-planner-gaps.md` -- Gap analysis between
  PostgreSQL planner features and Ra's rule set
- `rfcs/0003-plan-advice-integration.md` -- Companion RFC for
  `pg_plan_advice` integration
- pgrx documentation: https://github.com/pgcentralfoundation/pgrx
- PostgreSQL planner hooks: `src/include/optimizer/planner.h`,
  `src/include/optimizer/paths.h`
- PostgreSQL v19 `pg_plan_advice` commit: `5883ff30` (Robert Haas,
  2026-03-12)
- Phase 4 implementation plan (tasks #116-#120)
