# RFC 0086: Ballista Plan Emission

- Start Date: 2026-05-26
- Author: gregburd
- Status: Draft
- Tracking Issue: TBD

## Summary

Add a new emission target to Ra so that an optimised `RelExpr` can be
serialised as an [Apache DataFusion][df] `LogicalPlanNode` protobuf
and executed on an [Apache DataFusion Ballista][ballista] cluster.
This makes Ra usable as the query optimiser for distributed analytical
workloads without coupling Ra to PostgreSQL's executor.

[df]: https://datafusion.apache.org/
[ballista]: https://datafusion.apache.org/ballista/

## Motivation

Ra today emits one execution target: PostgreSQL's `PlannedStmt`, via
`crates/ra-pg-extension`. That covers a broad slice of the OLTP and
mixed workloads PostgreSQL is good at, but it intentionally hands
distributed execution off to Citus or foreign-data-wrappers — Ra
itself doesn't reason about exchange operators or stage boundaries.

Ballista solves the distributed-execution half of the problem. It
takes a DataFusion logical plan, breaks it into "query stages"
separated by shuffle/repartition operations, schedules those stages
across executors, and stitches the Arrow-native shuffle output back
together. Its planner is DataFusion's own — fine for SQL workloads
but a much smaller rule set than Ra's 307 active rewrite rules and no
neural cost model.

Combining Ra (optimiser) and Ballista (distributed executor) gives:

- **A distributed target for Ra** without writing a new executor.
  Ballista already handles scheduling, shuffle, fault tolerance, and
  Arrow IPC.
- **A better optimiser for Ballista** without intruding on
  DataFusion's roadmap. Ra's output is a DataFusion logical plan; the
  Ballista scheduler treats it identically to a plan DataFusion built
  itself.
- **A useful new benchmark.** TPC-H at SF=10–100 across a small
  Ballista cluster is a workload Ra has never been measured on.
  Generating a Ballista-shaped target plan exposes whether Ra's
  rewrite rules and cost model are useful at scale, or whether the
  benefit is OLTP-specific.

The audit (May 2026) found Ra winning all 21 TPC-H SF=0.01 queries
against PostgreSQL 18.4's planner by 89× geo-mean (planning time
only — see [`benchmarks/ra-vs-pg18-head-to-head.md`][bench]). That
result is on a single node. Whether Ra's advantage holds, shrinks, or
inverts in a distributed setting is open.

[bench]: ../../benchmarks/ra-vs-pg18-head-to-head.md

## Guide-level explanation

A user who wants to run a query through Ra and execute it on Ballista
follows three steps:

```rust
use ra_parser::sql_to_relexpr;
use ra_engine::Optimizer;
use ra_ballista::ToLogicalPlan;          // new crate

// 1. Parse and optimise as today.
let expr = sql_to_relexpr(
    "SELECT customer_id, SUM(amount) FROM orders \
     WHERE created_at > '2026-01-01' GROUP BY customer_id"
)?;
let optimised = Optimizer::new().optimize(&expr)?;

// 2. Convert RelExpr → DataFusion LogicalPlanNode.
let table_provider = catalog.resolve("orders")?;
let logical_plan = optimised.to_datafusion_logical_plan(&catalog)?;

// 3. Hand the plan to Ballista.
let ctx = ballista::prelude::BallistaContext::remote(
    "scheduler.example.com", 50050, &Default::default()
).await?;
let df = ctx.create_dataframe_from_logical_plan(logical_plan)?;
let batches = df.collect().await?;
```

Step (2) is the new code path. Steps (1) and (3) are existing. The
new crate `ra-ballista` provides the conversion and (optionally) a
session-context wrapper that hides step (3).

There are two consumption modes:

| Mode | Description | When to use |
|------|-------------|-------------|
| Embedded | Construct DataFusion's `LogicalPlan` Rust type in-process; pass to `BallistaContext::create_dataframe_from_logical_plan` | Same Rust process drives optimisation and submits to Ballista |
| Wire | Encode `LogicalPlanNode` as protobuf bytes; submit via Ballista's gRPC `ExecuteQuery(LogicalPlanNode)` | Process boundary between Ra and the Ballista scheduler |

Wire mode is the durable interface. Embedded mode is a convenience.

## Reference-level explanation

### Mapping `RelExpr` to DataFusion `LogicalPlan`

DataFusion's logical-plan tree (`datafusion-expr::LogicalPlan`) has a
near-isomorphic shape to Ra's `RelExpr`, with a small number of
gaps. The mapping table:

| `RelExpr` variant | DataFusion `LogicalPlan` variant | proto tag | Notes |
|---|---|---|---|
| `Scan { table, alias }` | `TableScan` | `ListingTableScanNode` (1) | Resolved via `TableProvider`; alias represented as `LogicalPlanBuilder::alias` |
| `Filter { predicate, input }` | `Filter` | `SelectionNode` (4) | Direct |
| `Project { columns, input }` | `Projection` | `ProjectionNode` (3) | `ProjectionColumn::alias` → `Expr::Alias` |
| `Join { join_type, condition, .. }` | `Join` | `JoinNode` (7) | Map `JoinType` enum: Inner/Left/Right/Full/Semi/Anti are 1:1; Ra's `Cross` becomes `Join` with empty equi-join keys and trivial filter, or `CrossJoinNode` |
| `Aggregate { group_by, aggregates, input }` | `Aggregate` | `AggregateNode` (6) | Map `AggregateExpr::function` (Min/Max/Sum/Count/Avg/StdDev/Variance/ArrayAgg/StringAgg) to DataFusion's `AggregateUDF` registry |
| `Sort { keys, input }` | `Sort` | `SortNode` (8) | `SortKey::direction` and `nulls` map to `SortExpr::asc` and `SortExpr::nulls_first` |
| `Limit { count, offset, input }` | `Limit` | `LimitNode` (5) | Direct |
| `Union { all, left, right }` | `Union` (or `Distinct(Union)`) | `UnionNode` | DataFusion's Union is bag-union; `all=false` wraps the result in `Distinct` |
| `Intersect { all, left, right }` | `Intersect` | (compound) | DataFusion supports both via `LogicalPlanBuilder::intersect` |
| `Except { all, left, right }` | `Except` | (compound) | Same |
| `CTE { name, definition, body }` | `SubqueryAlias` referencing a `Cte` extension | n/a | DataFusion has no first-class CTE; flattening is the standard trick. Ra's `RuleAdvisor` already inlines small CTEs upstream |
| `RecursiveCTE { ... }` | `RecursiveQuery` | `RecursiveQueryNode` | DataFusion 35+ supports it directly |
| `Window { functions, input }` | `Window` | `WindowNode` | Map `WindowExpr` to `WindowUDF` |
| `Distinct { input }` | `Distinct` | `DistinctNode` | Direct |
| `Values { rows }` | `Values` | `ValuesNode` | Direct |
| `Unnest { expr, .. }` | `Unnest` | `UnnestNode` | DataFusion's Unnest is column-oriented; map array-typed projection + Unnest |
| `MultiUnnest { ... }` | (multiple `Unnest`) | n/a | Lower to chained `Unnest` per array |
| `TableFunction { name, args, .. }` | (catalog `TableFunction`) | `CustomTableScanNode` | Requires DataFusion to have the function registered |
| `RowPattern { ... }` | (extension) | n/a | **Out of scope for v1.** No DataFusion equivalent |
| `IncrementalSort { ... }` | (lower to `Sort`) | `SortNode` | DataFusion treats this as an executor concern, not a logical-plan concept; emit a regular `Sort` and let Ballista's scheduler decide |

`Expr` (Ra's scalar expression type) maps to DataFusion's `Expr` enum
the same way:

| Ra `Expr` | DataFusion `Expr` |
|---|---|
| `Column(ColumnRef)` | `Expr::Column` |
| `Const(scalar)` | `Expr::Literal` |
| `BinOp { op, left, right }` | `Expr::BinaryExpr` |
| `UnaryOp { op, operand }` | `Expr::Not`, `Expr::Negative`, `Expr::IsNull`, etc. |
| `Function { name, args }` | `Expr::ScalarFunction` (DataFusion `ScalarUDF`) |
| `AggregateExpr { ... }` | `Expr::AggregateFunction` |
| `Case { operand, when, then, else }` | `Expr::Case` |
| `Cast { expr, ty }` | `Expr::Cast` or `Expr::TryCast` |
| `SubQuery(Box<RelExpr>)` | `Expr::ScalarSubquery(Arc<LogicalPlan>)` |
| `Exists(Box<RelExpr>)` | `Expr::Exists` |
| `InList { expr, list, .. }` | `Expr::InList` |
| `Between { expr, low, high, .. }` | `Expr::Between` |

Function names are the principal interop hazard. Ra accepts every
function its parser recognises, including PostgreSQL-isms like
`date_trunc`, `regexp_match`, JSONB operators (`@>`, `->>`),
`array_agg`. DataFusion's catalog supports a subset, plus its own
extensions (`make_array`, `array_element`). The conversion layer
needs a function-name dictionary with three policies:

1. Exact match in DataFusion stdlib → use it.
2. Known equivalent name → rewrite (`date_trunc` → DataFusion's
   `date_trunc`, `array_agg` → DataFusion's `array_agg`).
3. No equivalent → emit `Expr::ScalarUDF` referring to a Ra-side
   registered UDF, *or* return `EmissionError::UnsupportedFunction(name)`
   so the caller can decide whether to fail the query or fall back to
   the embedded path.

### Crate layout

A new workspace crate, `ra-ballista` (experimental layer):

```text
crates/ra-ballista/
├── Cargo.toml
├── src/
│   ├── lib.rs                  # public surface: ToLogicalPlan trait
│   ├── plan_emitter.rs         # RelExpr → datafusion-expr::LogicalPlan
│   ├── expr_emitter.rs         # ra_core::Expr → datafusion-expr::Expr
│   ├── function_dict.rs        # name-to-UDF mapping with policies
│   ├── proto.rs                # encode LogicalPlan → protobuf bytes
│   └── catalog.rs              # CatalogProvider trait Ra calls into
└── tests/
    ├── tpch_emission.rs        # 21 TPC-H queries → LogicalPlan, deserialise, EXPLAIN compare
    └── round_trip.rs           # RelExpr → LogicalPlan → protobuf → LogicalPlan, structural equality
```

Dependencies:

- `datafusion = "49"` (or whatever current stable matches the chosen
  Ballista release line)
- `datafusion-proto = "49"`
- `prost = "0.13"`
- `ra-core`, `ra-engine`

`ra-ballista` is **experimental layer** (`--features experimental`),
not core, because (a) its dependency footprint is large, (b) it
duplicates schema-resolution logic that core deliberately abstracts
behind `ra-metadata`, and (c) DataFusion's protobuf is not
version-stable across DataFusion releases (the proto README warns
that "a plan serialized with one version of DataFusion may not be
able to deserialized with a different version").

### Schema and statistics

DataFusion plans require an Arrow schema for every plan node. Ra's
`RelExpr` doesn't carry per-node schemas; column types are resolved
against `ra-metadata` on demand. The emitter therefore takes a
`CatalogProvider` argument (a thin trait — `lookup_table(name) ->
TableSchema` and `lookup_function(name) -> FunctionSignature`) and
threads schema construction top-down so each `LogicalPlan` node knows
its Arrow schema.

Statistics are a DataFusion extension; the emitter optionally
populates `Statistics` on each `TableScan` if Ra has them, which
DataFusion's optimiser will read. This is purely additive — DataFusion
handles missing stats gracefully.

### Distribution decisions: Ra's optimiser, Ballista's scheduler

Ra's e-graph saturation does not introduce `Repartition` or `Exchange`
nodes today. Ballista's scheduler decides where stage boundaries fall:
it walks the logical plan, identifies "pipeline breakers" (joins,
aggregates, sorts), and inserts shuffles automatically. The minimal
v1 emitter therefore emits a plan with **no repartition nodes** and
lets Ballista schedule.

This is the correct default but not the best long-term answer. Ra's
[distributed rule set][distributed-rules] (broadcast-hash-join,
shuffle-aware-join, two-phase-decomposable-agg, etc.) and existing
`Optimizer::with_topology` API understand cost-of-shuffle and could
emit explicit `Repartition` / `RepartitionExec(Hash)` nodes that
Ballista's scheduler would honour. **Out of scope for v1**, tracked
as a follow-up RFC. v1 ships the minimal-correct emitter; v2 adds
distribution awareness.

[distributed-rules]: ../docs/rules/distributed/

### Testing strategy

Three layers, mirroring what `ra-pg-extension` does:

1. **Round-trip structural equality.** For each TPC-H and JOB query in
   the existing corpus, parse → optimise → emit → protobuf bytes →
   deserialise → compare to the freshly-emitted plan. Any difference
   here is a bug in the emitter.
2. **DataFusion local execution.** Same plans run through
   `SessionContext::execute_logical_plan` against an in-process
   DataFusion (no Ballista). Compare result rows to a reference
   (Postgres or DuckDB) via the existing `ra-difftest` harness. This
   verifies semantic equivalence without dragging in a Ballista
   cluster.
3. **Ballista cluster execution (CI-optional).** Same plans run
   against a docker-compose Ballista cluster. Gated behind a
   `ballista-integration` feature so CI runs are opt-in.

### Versioning and compatibility

DataFusion's protobuf is *not* compatibility-stable across DataFusion
versions. Pin `datafusion` and `datafusion-proto` to the same minor
version that the target Ballista release was built against, surface
that pin in `ra-ballista`'s `Cargo.toml`, and document the
DataFusion-version requirement prominently. Mismatch is the most
likely deployment failure mode.

A future Ballista release that switches to Substrait (which is on the
roadmap; see Ballista issues tracking Substrait support) would
significantly reduce this concern. If/when that happens, add a
Substrait emission path alongside the DataFusion-proto path.

## Drawbacks

- **Dependency weight.** `datafusion` v49 pulls in ~80 transitive
  dependencies (`arrow-*`, `parquet`, `object_store`, etc.). Confining
  to the experimental layer keeps the core build untouched but the
  crate itself is heavy.
- **Plan-shape mismatch.** Ra's saturation produces shapes (e.g.
  bushy join trees with extracted projections) that DataFusion's
  default optimiser would never produce. Ballista's scheduler may
  not stage these as efficiently as plans that came out of
  DataFusion's own optimiser. v1 ships with no distribution awareness
  in Ra's emitter; v2 fixes this.
- **Function coverage is a moving target.** DataFusion's UDF catalog
  grows; Ra's parser accepts everything. Maintaining the function
  dictionary is ongoing work. The emission error path (return
  `UnsupportedFunction`) keeps it tractable but pushes the burden to
  callers.
- **DataFusion-proto version pinning.** A Ballista cluster running
  DataFusion 49 cannot deserialise a plan emitted against DataFusion
  48. This is a fact of DataFusion's design, not Ra's, but the docs
  must call it out.
- **Maintenance against a moving upstream.** DataFusion releases
  every ~4 weeks. The emitter has to track. Plan: pin to the
  DataFusion version of the latest stable Ballista, bump together.
- **Two emission targets to keep coherent.** Bug fixes in
  `ra-pg-extension::plan_builder` and `ra-ballista::plan_emitter` will
  share root causes (e.g. a `RelExpr` shape that's awkward to lower).
  Factor shared lowering passes — particularly the
  ordering-pass and the projection-resolver — into `ra-engine` rather
  than duplicating per-target.

## Unresolved questions

- **Should the Ra → DataFusion mapping go through Substrait first?**
  Substrait is the cross-engine intermediate format; DataFusion has a
  Substrait consumer; Ballista has Substrait support on its roadmap.
  Emitting Substrait would future-proof against the
  DataFusion-proto-version churn. The cost is going through a less
  expressive IR (Substrait doesn't yet cover everything DataFusion
  does, particularly around recursive CTEs and table-valued
  functions). Decision deferred to v2.
- **Does Ra's cost model help DataFusion's plans?** DataFusion will
  re-optimise the logical plan on receipt by default (its own optimiser
  passes run before stage planning). Some passes may undo Ra's
  rewrites. Plan: investigate `SessionContext::with_optimizer_rules(vec![])`
  to disable DataFusion's logical optimiser when consuming Ra plans
  — but only if benchmarks show DataFusion's passes are net-negative
  on Ra-shaped plans.
- **Where does parse happen?** v1 makes Ra the only parser
  (Lime grammar → `RelExpr` → DataFusion `LogicalPlan`). Long term it
  may be useful to also offer DataFusion → `RelExpr` for plans that
  arrive as SQL strings the Ballista client built without going
  through Ra. That's a separate RFC.

## Future possibilities

- **Distribution-aware emission (v2).** Generate `Repartition` /
  hash-partition / broadcast hints in the logical plan, driven by
  Ra's existing distributed cost model and topology awareness.
  This is where the Ra+Ballista combination becomes more interesting
  than Ballista on its own.
- **Substrait emission.** Once DataFusion's Substrait coverage is
  complete enough, swap the wire format. Ra owns the `RelExpr` →
  Substrait mapping; downstream consumers (DataFusion, Ballista,
  potentially Spark Connect) become fungible.
- **Round-trip the other way.** A `LogicalPlan` → `RelExpr` adapter
  would let Ra optimise plans built by DataFusion's SQL frontend
  (e.g. via the Python or DataFrame API) without re-parsing the SQL.
- **Bench in earnest.** TPC-H SF=10 / SF=100 across a 4-executor
  Ballista cluster, measuring (a) end-to-end query time, (b)
  optimisation time, (c) shuffle volume. Compare three optimiser
  configurations: DataFusion default, Ra without distribution
  awareness (v1), Ra with distribution awareness (v2). Publishable
  result if Ra wins meaningfully.

## References

- Ballista architecture overview:
  <https://datafusion.apache.org/ballista/contributors-guide/architecture.html>
- DataFusion proto definition:
  <https://github.com/apache/datafusion/blob/main/datafusion/proto/proto/datafusion.proto>
- DataFusion proto version-stability warning:
  <http://docs.rs/datafusion-proto/latest>
- [`crates/ra-pg-extension/src/plan_builder.rs`][pg-builder] — the
  existing PostgreSQL emitter, structural template for `ra-ballista`.
- [`crates/ra-engine/src/egraph/optimizer.rs`][optimizer] — Ra's
  optimiser entry point.
- [`crates/ra-core/src/algebra.rs`][relexpr] — the `RelExpr` enum
  this RFC maps from.
- [`docs/research/geqo-vs-ra.md`](../../docs/research/geqo-vs-ra.md)
  — sibling document comparing Ra to PostgreSQL GEQO.

[pg-builder]: ../../crates/ra-pg-extension/src/plan_builder.rs
[optimizer]: ../../crates/ra-engine/src/egraph/optimizer.rs
[relexpr]: ../../crates/ra-core/src/algebra.rs
