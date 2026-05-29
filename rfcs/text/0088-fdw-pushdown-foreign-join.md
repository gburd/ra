# RFC 0088: FDW Pushdown for `FOREIGN_JOIN` Plan Advice

- Start Date: 2026-05-29
- Author: gregburd
- Status: Draft
- Tracking Issue: TBD

## Summary

Honor `pg_plan_advice`'s `FOREIGN_JOIN((a b))` directive by
producing PostgreSQL Foreign Data Wrapper (FDW) pushdown plans
when the targeted relations are foreign tables on the same
remote server. Today Ra parses and validates the directive but
the validator unconditionally classifies it as `failed`
because Ra's `RelExpr` doesn't distinguish foreign tables from
local tables, has no deparse path, and emits no `T_ForeignScan`
plan node. This RFC scopes the work to convert that
validation-only behavior into real pushdown.

## Motivation

### Workload demand

This work is justified by **cross-database OLAP and federated
analytics workloads**, not by general-purpose plan-advice
completeness. Specific scenarios:

1. **Cross-region warehouse replicas**. A retail analytics
   team's primary OLAP fact tables live in a `us-west-2`
   PostgreSQL warehouse exposed via `postgres_fdw` to the
   `us-east-1` reporting cluster where dimension tables and
   denormalized aggregates live. Joins between them are
   common — `SELECT ... FROM fact_sales f JOIN
   dim_product p ON f.sku = p.sku WHERE f.date = ...`. PG's
   default planner sometimes pulls all rows of `fact_sales`
   to the local node before joining. With `postgres_fdw`'s
   pushdown enabled, PG's planner *can* push the join
   foreign-side, but only when the join is "safe" by its
   internal heuristics (foreign-table check, join-clause
   shippability, etc.). Plan-advice's `FOREIGN_JOIN((f p))`
   is meant to override the heuristic when the user knows
   the join is safe and PG's path-cost calculation got the
   wrong answer.
2. **Sharded fact tables via `postgres_fdw`**. Multi-region
   sharding patterns where each shard is exposed as a foreign
   table. Aggregations across shards routinely benefit from
   pushdown so each shard does its share of the work locally.
3. **`file_fdw` over Parquet**. Read-only Parquet datasets
   exposed via `file_fdw` (or `parquet_fdw`) where pushing
   the projection / filter / join into the FDW reduces
   row materialization significantly.

### Why Ra needs to do this work explicitly

PG's planner already supports FDW pushdown via the
`GetForeignJoinPaths` hook. But Ra replaces PG's planner
wholesale via `planner_hook` — meaning that when Ra's hook
is active, PG's `add_paths_to_joinrel` never runs, and the
FDW author's `GetForeignJoinPaths` is never called. The
pushdown logic isn't somewhere we can opt into; we have to
build the equivalent.

Without this RFC, Ra users running federated workloads with
`ra_planner.enabled = on` will see plans that materialize
foreign tables locally even when `postgres_fdw` could push
the join. They have to choose between Ra's planning-time
advantage (~89× on TPC-H SF=0.01) and PG's FDW pushdown.
Once this RFC lands, they can have both.

### Out of scope

- General federation across non-PG databases (Trino-style).
  This RFC is scoped to PG's FDW API as it exists today.
- New FDW APIs. We use what's already in
  `~/src/postgres/src/include/foreign/`.
- Cross-server joins (joining tables on different remote
  servers). PG's planner doesn't push these either; the FDW
  contract requires both sides on the same server.

## Guide-level explanation

After this RFC is implemented, the user-visible behavior is:

```sql
-- Foreign tables on the same remote server.
CREATE EXTENSION postgres_fdw;
CREATE SERVER warehouse FOREIGN DATA WRAPPER postgres_fdw
  OPTIONS (host 'warehouse.example.com', dbname 'analytics');
CREATE USER MAPPING FOR CURRENT_USER SERVER warehouse
  OPTIONS (user 'reporter', password '***');
CREATE FOREIGN TABLE fact_sales (...)
  SERVER warehouse OPTIONS (table_name 'fact_sales');
CREATE FOREIGN TABLE dim_product (...)
  SERVER warehouse OPTIONS (table_name 'dim_product');

-- Without advice, Ra's plan honors PG's join enumeration
-- but emits a local hash join (suboptimal):
EXPLAIN SELECT ... FROM fact_sales f JOIN dim_product p
  ON f.sku = p.sku WHERE f.date >= ...;
-- Hash Join
--   -> Foreign Scan on fact_sales f
--   -> Foreign Scan on dim_product p

-- With advice, Ra produces a single Foreign Scan that
-- represents the deparsed join, executed remotely:
SET ra_planner.plan_advice = 'FOREIGN_JOIN((f p))';
EXPLAIN SELECT ... FROM fact_sales f JOIN dim_product p
  ON f.sku = p.sku WHERE f.date >= ...;
-- Foreign Scan
--   Relations: (fact_sales f) INNER JOIN (dim_product p)
--   Remote SQL: SELECT ... FROM fact_sales f INNER JOIN
--               dim_product p ON f.sku = p.sku WHERE ...

-- And EXPLAIN(PLAN_ADVICE) reports the success:
EXPLAIN (PLAN_ADVICE) SELECT ...;
-- Supplied Plan Advice:
--   FOREIGN_JOIN((f p))    matched
```

When pushdown isn't applicable (different servers, mixed
foreign/local, FDW doesn't support join pushdown), the
validator continues to flag the advice as `failed` so EXPLAIN
honestly reports the situation.

## Reference-level explanation

### Phase 1: foreign-table awareness in `RelExpr` and metadata

`RelExpr::Scan { table, alias }` doesn't carry foreign-table
identity today. We add a sidecar map alongside `TableMap`:

```rust
// crates/ra-pg-extension/src/foreign_table_map.rs
pub struct ForeignTableMap {
    /// Per-alias metadata for relations that resolve to
    /// foreign tables. Aliases that aren't foreign are
    /// absent from the map.
    pub by_alias: HashMap<String, ForeignTableInfo>,
}

pub struct ForeignTableInfo {
    pub server_oid: pg_sys::Oid,
    pub server_name: String,
    /// Whether the FDW for this server claims to support
    /// `GetForeignJoinPaths`. Read from the FDW handler's
    /// `routine.GetForeignJoinPaths` symbol.
    pub supports_join_pushdown: bool,
}
```

The map is built during `PlannedStmt` construction by walking
the query's range table and probing each RTE for
`relkind = 'f'` (foreign table). We never modify `RelExpr`
itself, preserving the invariant that the algebra stays
pure-logical.

Honest scope: foreign-table identity is *only* attached at
plan-builder time, not during e-graph optimization. This
matches the sidecar pattern from RFC 0087.

### Phase 2: foreign-join eligibility check

```rust
// crates/ra-pg-extension/src/foreign_join.rs
pub struct ForeignJoinCandidate<'a> {
    pub left: &'a RelExpr,
    pub right: &'a RelExpr,
    pub join_type: JoinType,
    pub condition: &'a Expr,
    pub server_oid: pg_sys::Oid,
}

/// Determine whether `left JOIN right ON cond` is safely
/// foreign-pushable on a single server. Returns the
/// candidate when:
/// - every leaf scan in `left` and `right` resolves to a
///   foreign table on the same server,
/// - the join condition references only columns from those
///   foreign tables (no local-side correlation),
/// - the FDW for that server supports join pushdown.
/// Returns `None` otherwise.
pub fn check_foreign_join<'a>(
    join: &'a RelExpr,
    map: &ForeignTableMap,
) -> Option<ForeignJoinCandidate<'a>>;
```

The eligibility check fires only when `FOREIGN_JOIN` advice
targets the join's aliases. It never auto-applies to joins
without explicit advice — auto-pushdown decisions belong to
PG's path-costing logic, which we're not replicating.

### Phase 3: deparse path

`postgres_fdw` and many other FDWs expose deparse functions
that turn a list of `ForeignPath` plus join clauses into
remote SQL. We can't call those directly because they
require the `RelOptInfo` / `PlannerInfo` machinery that PG
populates and we don't.

The pragmatic path: implement minimal SQL deparse in Ra
itself, scoped to the join shapes plan-advice covers.

```rust
// crates/ra-pg-extension/src/foreign_deparse.rs
pub struct DeparseContext<'a> {
    pub server_dialect: SqlDialect,
    pub foreign_table_names: HashMap<String, String>, // alias -> remote name
}

pub fn deparse_foreign_join(
    candidate: &ForeignJoinCandidate,
    ctx: &DeparseContext,
) -> Result<String, DeparseError>;
```

Initially supports:

- `INNER`, `LEFT OUTER`, `RIGHT OUTER`, `FULL OUTER`
- equi-join clauses (`AND`-of-`Col = Col`)
- column projection (selecting only required columns)
- pushed-down filters (`Filter` parents that reference
  foreign-only columns)

Bigger shapes (subqueries, window functions, aggregation)
defer to a follow-up RFC. The deparse is a constrained
subset because plan-advice only ever targets specific join
shapes — not whole-query rewriting.

The dialect uses `ra-dialect`'s existing PostgreSQL printer
since `postgres_fdw` is the dominant FDW. Other dialects can
be added per FDW.

### Phase 4: `T_ForeignScan` plan-builder integration

```rust
// crates/ra-pg-extension/src/plan_builder.rs
unsafe fn build_foreign_scan_join(
    &mut self,
    candidate: &ForeignJoinCandidate,
) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
    // 1. Allocate T_ForeignScan
    // 2. Set fs_relids to the bitmapset of foreign-table RTEs
    //    (foreign_join_relids field on ForeignScan).
    // 3. Set fs_server = candidate.server_oid
    // 4. Set fdw_exprs with the join clause OpExprs
    //    (FDW BeginForeignScan reads them)
    // 5. fdw_private contains the deparsed remote SQL string
    //    encoded as a List of String nodes — postgres_fdw
    //    expects this in the same format its own planner
    //    emits.
    // 6. Cost: estimated row count from foreign-server
    //    statistics (or fallback to local stats), plus
    //    network-roundtrip overhead.
}
```

The `fdw_private` format is FDW-specific. For
`postgres_fdw` we match the layout in
`contrib/postgres_fdw/postgres_fdw.h::FdwScanPrivateIndex`
exactly so the FDW's own `BeginForeignScan` can consume our
synthesized plan node without changes.

### Phase 5: `FOREIGN_JOIN` advice plumbing

The existing `PhysicalChoices::join_for(alias)` is extended
with `JoinInnerStrategy::ForeignJoin(server_oid)` (currently
just `ForeignJoin` with no payload). The plan-builder's
`build_join` arm dispatches to `build_foreign_scan_join`
when the candidate passes the eligibility check. Otherwise
it falls back to HashJoin and the validator records the
advice as `failed` — same as today.

### Phase 6: validator integration

`classify_foreign_join` in `plan_advice_validate.rs` is
extended:

```rust
fn classify_foreign_join(
    item: &AdviceItem,
    plan: &RelExpr,
    foreign_map: &ForeignTableMap,
) -> FeedbackFlags {
    // Today: always FAILED.
    // After this RFC:
    // - check_foreign_join_eligibility(item.targets, plan,
    //   foreign_map) ->
    //     Eligible    -> MATCH_FULL (the plan-builder will
    //                    produce the foreign scan)
    //     Mixed sides -> MATCH_PARTIAL | FAILED
    //     Wrong server-> FAILED + INAPPLICABLE
    //     Not foreign -> FAILED + INAPPLICABLE
    //     ...
}
```

### Phase 7: cost integration

The cost model needs to know that foreign scans pay network
roundtrip. Update `Cost::network` and add per-server
network-cost configuration via GUC
`ra_planner.foreign_network_cost_factor` (default 1.0).

### Implementation phases (timeline estimate)

| Phase | Description | Estimate |
|-------|-------------|---------|
| 1 | `ForeignTableMap` + relkind probe | 1 week |
| 2 | Eligibility check | 0.5 week |
| 3 | Minimal deparse (PG dialect) | 1.5 weeks |
| 4 | `T_ForeignScan` builder + `fdw_private` | 1 week |
| 5 | Advice plumbing | 0.5 week |
| 6 | Validator integration | 0.5 week |
| 7 | Cost integration + tests | 1 week |
| **Total** | | **6 weeks** |

Add 2 weeks for integration testing across multiple FDW
versions and 1 week for RFC review and revisions: **9 weeks
calendar**.

### Test plan

1. **Unit tests** (no PG required):
   - Eligibility check on synthetic `ForeignTableMap` and
     `RelExpr`.
   - Deparse golden tests against expected SQL strings.
   - Validator classification (all four feedback states).

2. **Integration tests** (`pgrx`, requires PG with
   `postgres_fdw`):
   - Two-table foreign join, advice honored.
   - Mixed local + foreign join, advice failed.
   - Different-server foreign join, advice failed.
   - FDW that doesn't support join pushdown, advice
     failed.
   - Equi-join, semi-join, left/right/full outer, cross
     join.
   - Pushed-down filters and projections.

3. **Differential tests** vs PG's planner with
   `ra_planner.enabled = off`:
   - Same query under both planners produces same result
     set.
   - Plan-shape diff: Ra produces foreign join; PG produces
     foreign join via its own pushdown path. Result-set
     equivalence is the contract; plan shape is informative.

4. **Benchmark**:
   - TPC-H Q3-style query against `postgres_fdw` warehouse,
     measure end-to-end latency with and without the advice.
     Expected: 10-100× speedup when pushdown is honored
     (matches `postgres_fdw` documented behavior).

## Drawbacks

- **6+ weeks of engineering**. This is the largest single
  RFC in plan-advice scope.
- **FDW API surface area**. `postgres_fdw`, `file_fdw`,
  `parquet_fdw`, `mysql_fdw`, `oracle_fdw`, `tds_fdw` —
  each has slightly different `fdw_private` conventions,
  cost expectations, and deparse needs. Initial scope is
  `postgres_fdw` only; later FDWs need per-FDW work.
- **Catalog probe overhead**. Reading `pg_class.relkind`
  for every relation in every query adds ~50ns per
  relation. For non-foreign-heavy workloads this is wasted
  work; we mitigate by checking `relkind` only when the
  query actually has `FOREIGN_JOIN` advice.
- **Deparse fidelity**. Ra's deparse won't match
  `postgres_fdw`'s output byte-for-byte. SQL semantics
  match; cosmetic differences (whitespace, parenthesization)
  may surprise users diffing remote query logs.

## Rationale and alternatives

### Why this design?

1. **Matches PG's existing FDW contract** — we don't invent
   new APIs; we satisfy what `postgres_fdw` and friends
   already expect to consume.
2. **Sidecar approach** — `ForeignTableMap` parallels
   `PhysicalChoices` from RFC 0087. Same pattern: keep
   `RelExpr` pure, decide foreign-ness at plan-builder
   time. Consistent architecture.
3. **Opt-in only** — pushdown only fires with explicit
   advice. Auto-pushdown would mean replicating PG's full
   path-cost machinery, which is multi-month work and
   conflicts with Ra's "fast simple plans" thesis.

### Alternative 1: replicate PG's `add_paths_to_joinrel`

Pros: full pushdown without explicit advice, matches PG
behavior exactly. Cons: 3× the engineering effort and
re-implements something PG already has. Rejected: the user
already has PG when they want PG's behavior.

### Alternative 2: emit a stub plan that defers to PG's FDW

Have the plan-builder produce a single `T_ForeignScan` with
a stub `fdw_private` and let PG's FDW handler regenerate
the deparse. Pros: avoids deparse work. Cons: requires
PG-side cooperation that doesn't exist; FDWs expect
`fdw_private` to be filled in by the planner that produced
the path.

### Alternative 3: feature flag gating

Ship the work behind `ra_planner.enable_foreign_pushdown`
default-off so foreign-light workloads don't pay the
catalog-probe cost. Decision: yes, gate it. Default off
during initial rollout, default on once the test matrix
covers the major FDWs.

## Prior art

- **PostgreSQL's own FDW pushdown**:
  `~/src/postgres/contrib/postgres_fdw/postgres_fdw.c` —
  `postgresGetForeignJoinPaths` is the reference. Ra's
  plan-builder replicates the relevant subset.
- **Citus**:
  shards split queries across worker nodes; uses a similar
  deparse-and-execute pattern but with its own non-FDW
  protocol. Informs our deparse-context design.
- **Trino**:
  pluggable connectors with declarative pushdown contracts.
  Different problem domain (no shared SQL dialect) but the
  cost-attribution model is informative.
- **Ballista** (RFC 0086): distributed plan emission
  framework Ra already has scaffolding for. Different
  semantics (Ballista pushes whole plan trees to workers;
  this RFC pushes joins to FDWs) but shares the deparse-
  to-remote-SQL pattern.

## Unresolved questions

- **Multi-server pushdown**. PG's planner doesn't attempt
  joining tables on different foreign servers. Should Ra?
  Tentatively no, defer to a follow-up RFC if the demand
  appears.
- **Aggregate pushdown**. `FOREIGN_JOIN` covers joins; PG's
  `postgres_fdw` also pushes aggregates via
  `GetForeignUpperPaths`. Plan-advice has no
  `FOREIGN_AGG` directive today; if we add one it's
  scoped to a separate RFC.
- **Subquery pushdown**. `WHERE foo IN (SELECT ...)` shapes
  are common in federated queries. Initial deparse won't
  handle them; defer.

## Future work

- `FOREIGN_AGG` directive and aggregate pushdown.
- Multi-server foreign joins via Ra's distributed-execution
  layer (Ballista, RFC 0086).
- FDW-specific cost calibration. The default 1.0 network
  cost factor is a placeholder; real tuning needs
  per-server roundtrip measurement.
- Auto-pushdown without explicit advice (alternative 1
  rejected here, but workload demands may revive it).

## References

- `~/src/postgres/contrib/postgres_fdw/` — the canonical
  FDW pushdown implementation.
- `~/src/postgres/src/include/foreign/foreign.h` — FDW
  routine struct.
- `~/src/postgres/src/include/foreign/fdwapi.h` —
  `GetForeignJoinPaths` API.
- RFC 0086 — distributed plan emission.
- RFC 0087 — physical-operator selection (sidecar pattern).
- `docs/integrations/plan-advice.md` — user-facing
  plan-advice docs.
- `crates/ra-engine/src/plan_advice_validate.rs::classify_foreign_join`
  — current validation-only behavior this RFC replaces.
