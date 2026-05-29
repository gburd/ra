# Plan Advice

Ra ships an experimental `ra-plan-advice` crate that parses,
manipulates, and renders the same plan-advice mini-language
PostgreSQL 19 introduced in
[`contrib/pg_plan_advice`](https://www.postgresql.org/docs/current/pgplanadvice.html).
Strings produced by Ra are intended to round-trip through PG's
parser unchanged and vice versa.

## Status

| Capability | Status |
|---|---|
| Parse PG-compatible advice strings | **Done** (every example from PG's `sql/*.sql` corpus is accepted/rejected the same way) |
| Render an `Advice` AST as a PG-compatible string | **Done** (proptest-verified round-trip) |
| `PGS_*` strategy-mask constants (bit-for-bit PG-compatible) | **Done** |
| Trove lookup (scan / rel / join categories) | **Done** |
| `PGPA_FB_*` feedback flags + exact PG output wording | **Done** |
| Generate advice from a finished `RelExpr` plan | **Done** (`ra_engine::plan_advice_emit::emit_advice`) |
| Honor advice in Ra's optimizer (`OptimizerConfig.plan_advice`) | **Done** for `JOIN_ORDER`; scan-method tags parsed but not gated (RelExpr is logical-only) |
| Round-trip oracle test (parallel to PG's `test_plan_advice`) | **Done** (`crates/ra-engine/tests/plan_advice_round_trip.rs`) |
| Validate produced plan and compute `FeedbackFlags` | **Done** (`ra_engine::plan_advice_validate::validate_advice`) |
| `Cost::DISABLE_PENALTY` for plans that violate supplied advice | **Done** (`ra_engine::Optimizer::apply_plan_advice_penalty`) — adds `1e10` per `FAILED` item to the produced cost, matching PG's `disable_cost` behavior |
| Per-relation physical-strategy preferences (`PhysicalChoices`) | **Done** — scan-method, join-method, and parallelism advice compile to a typed map exposed on `OptimizationResult.physical_choices` |
| Cost-driven population of `PhysicalChoices` when advice is absent | **Done** — `PhysicalChoices::augment_from_stats` picks SeqScan for tiny tables, IndexScan for medium tables with useful indexes, Hash for equi-joins, NestedLoopPlain for non-equi-joins ([RFC 0087](../../rfcs/text/0087-physical-operator-selection.md)) |
| PG extension GUCs (`ra_planner.plan_advice`, ...) | **Done** |
| `pg_plan_advice.advice` GUC compatibility shim | **Done** (registers under both names; the upstream name wins when set) |
| `EXPLAIN (PLAN_ADVICE)` registration via `RegisterExtensionExplainOption` | **Done** (raw FFI; renders supplied advice with feedback flags) |
| `Generated Plan Advice:` block in EXPLAIN output | **Done** — emit runs in the planner hook; explain hook reads via session-local stash |
| `ra-pg-extension` plan-builder consumption of `PhysicalChoices` | **Done** — `PlanBuilder::set_physical_choices` + `build_scan_with_advice` dispatch (`SEQ_SCAN`, `INDEX_SCAN`, `INDEX_ONLY_SCAN` honored fully; `TID_SCAN` and `BITMAP_HEAP_SCAN` honored when a parent Filter provides the required predicate shape — see "Filter peephole" below; `DO_NOT_SCAN` documented as out-of-scope negative constraint) + `build_join` (Hash, NestedLoop_*, MergeJoin_* honored fully; `FOREIGN_JOIN` documented as out-of-scope FDW pushdown) + Gather suppression (`NO_GATHER`) |

The first three rows are what shipped in
[commit 8aef6a13](https://codeberg.org/gregburd/ra/commit/8aef6a13).
The bottom three are tracked in
[`docs/research/pg-plan-advice-port.md`](../research/pg-plan-advice-port.md)
phases 4–7.

## Quick start

```rust
use ra_plan_advice::{parse_advice, render_advice};

let s = "JOIN_ORDER(f d) HASH_JOIN(d) SEQ_SCAN(f d)";
let advice = parse_advice(s)?;
assert_eq!(advice.len(), 3);

// AST is fully typed
assert_eq!(advice[0].tag, ra_plan_advice::AdviceTag::JoinOrder);

// Round-trip via the renderer
let s2 = render_advice(&advice);
assert_eq!(parse_advice(&s2)?, advice);
```

## Mini-language summary

The grammar matches PostgreSQL exactly. See PG's
[plan advice documentation](https://www.postgresql.org/docs/current/pgplanadvice.html)
for the full reference. Quick tour:

```text
advice          := item*
item            := tag '(' targets ')'
target          := identifier | '(' targets ')' | '{' targets '}'
identifier      := name [ '#' n ] [ '/' [schema '.'] partition ] [ '@' plan ]
```

Tags fall into four shape classes:

| Class | Tags | Targets |
|---|---|---|
| Simple | `SEQ_SCAN`, `BITMAP_HEAP_SCAN`, `TID_SCAN`, `NO_GATHER` | flat list of identifiers |
| Index | `INDEX_SCAN`, `INDEX_ONLY_SCAN` | identifier paired with `[schema.]index_name` |
| Generic | `HASH_JOIN`, `MERGE_JOIN_*`, `NESTED_LOOP_*`, `GATHER`, `GATHER_MERGE`, `SEMIJOIN_*`, `PARTITIONWISE`, `DO_NOT_SCAN`, `FOREIGN_JOIN` | identifiers and one level of `(...)` sublist |
| Join order | `JOIN_ORDER` | identifiers, `(...)` ordered sublists, and one level of `{...}` unordered sublists |

`FOREIGN_JOIN` requires every target to be a sublist with at
least two members (PG raises a parse error otherwise; so does Ra).

`JOIN_ORDER` requires at least one target.

All other tags accept an empty target list.

## Compatibility commitments

1. **Identifier syntax** (`alias#n/schema.name@plan`) is exactly
   PG's.
2. **Tag spellings** are case-insensitive on input and uppercase on
   output, identical to PG's `pgpa_cstring_advice_tag`.
3. **Whitespace and `/* ... */` comments** are skipped between
   every token. Unterminated comments are an error in both engines.
4. **Strategy-mask bit values** (the `PgsMask` constants) match
   PG's `pathnodes.h` so masks generated by Ra can be honored by PG
   without reinterpretation.
5. **Feedback wording** (`matched`, `partially matched`,
   `not matched`, `inapplicable`, `conflicting`, `failed`) matches
   `pgpa_trove_append_flags` byte-for-byte, so log filtering and
   tooling between the two are interoperable.

## Verification

`crates/ra-plan-advice/tests/data/pgpa-corpus.txt` is a verbatim
extract of every `SET pg_plan_advice.advice = '...'` value in
PostgreSQL's own regression suite (`contrib/pg_plan_advice/sql/*.sql`).
`tests/corpus.rs` runs Ra's parser over every line and verifies the
accept/reject classification matches PG.

`tests/round_trip.rs` runs a proptest with 256 generated ASTs;
every one survives `parse_advice(render_advice(advice)) == advice`.

Run the test suite:

```bash
cargo test -p ra-plan-advice
```

## Status

The
[port plan](../research/pg-plan-advice-port.md)
documents the design rationale for the physical-method
machinery; the user-facing plan-advice surface is now
production-complete for every tag `pg_plan_advice` defines:

| Tag | Status |
|-----|--------|
| `JOIN_ORDER` | Honored end-to-end |
| `SEQ_SCAN` | Honored end-to-end |
| `INDEX_SCAN` | Honored end-to-end |
| `INDEX_ONLY_SCAN` | Honored end-to-end |
| `BITMAP_HEAP_SCAN` | Honored via Filter peephole; bitmap-AND/OR combination across multiple indexes |
| `TID_SCAN` | Honored via Filter peephole when `ctid =` predicate present |
| `DO_NOT_SCAN` | Negative-constraint validation: `failed` when alias still in plan, `matched` when eliminated, `partially matched + failed` when partially eliminated |
| `HASH_JOIN` | Honored end-to-end |
| `MERGE_JOIN_PLAIN` / `MERGE_JOIN_MATERIALIZE` | Honored end-to-end via `Sort` + `T_MergeJoin` with `get_mergejoin_opfamilies` |
| `NESTED_LOOP_PLAIN` / `NESTED_LOOP_MATERIALIZE` / `NESTED_LOOP_MEMOIZE` | Honored end-to-end |
| `FOREIGN_JOIN` | Validation flags as `failed` (FDW pushdown not implemented; see "Architectural notes" below) |
| `GATHER` / `GATHER_MERGE` | Honored end-to-end |
| `NO_GATHER` | Honored end-to-end (Gather wrapper suppression) |

## Architectural notes

Two areas are worth calling out so consumers of plan-advice
understand them as **design decisions**, not bugs or unfinished
features:

### `FOREIGN_JOIN` is honest validation-only

True FDW pushdown means the optimizer recognizes that two
foreign-table joins can be sent to the foreign server as a
single SQL statement, fetches the result, and avoids
materializing rows on the local backend. PG implements this
via `GetForeignJoinPaths` in the FDW API plus per-FDW
`postgres_fdw`/`file_fdw` deparse logic. Honoring
`FOREIGN_JOIN` advice in Ra would require:

1. A way to recognize foreign tables in `RelExpr` (today the
   algebra is FDW-agnostic — every base relation is treated
   identically).
2. A deparse path that takes a `Join` subtree, walks it back
   to SQL using the foreign server's dialect, and emits a
   `T_ForeignScan` plan node.
3. Cost integration so the optimizer prefers foreign-pushed
   plans only when the network cost is acceptable.

This is multi-week work that goes well beyond plan-advice
semantics. The honest, production-correct behavior today is
to validate the advice as `failed` so the user gets a clear
EXPLAIN signal that the requested optimization isn't
available. The user can decide whether to disable Ra for
foreign-join queries, configure their FDW differently, or
accept the local join. The full design — phased
implementation, deparse approach, FDW integration, cost
plumbing — is documented in
[RFC 0088](../../rfcs/text/0088-fdw-pushdown-foreign-join.md)
for when a workload demand justifies the engineering
investment (cross-region warehouse replicas, sharded fact
tables via `postgres_fdw`, `file_fdw` over Parquet are
the leading candidates).

### Cost-driven physical lowering: chosen sidecar design

`pg_plan_advice`'s upstream documentation describes physical
hints as steering "path-cost" decisions during PG's
join-enumeration phase. PG interleaves logical exploration
(which join orders to try) with physical decisions (hash vs
merge vs nestloop) because each candidate path's cost depends
on both.

Ra's optimizer makes a different architectural choice. The
e-graph (egg) does logical equality saturation only; physical
strategy is decided **after** extraction by populating the
`PhysicalChoices` sidecar map either from supplied advice or
from cost-driven defaults
(`PhysicalChoices::augment_from_stats`). The plan-builder
consumes the map at PG-Plan emission time.

This is a deliberate trade-off, not a missing feature:

- **Pro**: keeps the e-graph small and fast. Adding physical
  variants for every operator (50+ variants) would multiply
  the e-class count and slow saturation. Ra's measured
  planning time is 12.8μs geomean on TPC-H SF=0.01 (vs PG's
  1089μs); much of that comes from keeping the e-graph
  logical-only.
- **Pro**: matches Ra's primary use case — PG drop-in
  replacement for OLTP plans where physical decisions are
  near-trivial (small-table seq-scan, indexed point lookup,
  hash join for equi-joins). The full Cascades-style
  interleaving pays off mostly for analytical workloads with
  expensive join orders.
- **Con**: physical decisions can't influence join ordering.
  When a particular join order is only attractive *because*
  the inner side would be a cheap hash-join build, Ra can't
  see that during enumeration. PG can.

Workloads where the con dominates would warrant promoting
`PhysicalChoices` into e-graph rewrite rules that lower
`Join` to `HashJoin`/`MergeJoin`/`NestLoop`. That design is
documented in detail in
[RFC 0089](../../rfcs/text/0089-egraph-cost-driven-physical-lowering.md):
phased migration, cost-model extensions for sort-order
analysis, supplied-advice biasing, and the rollout
strategy. The sidecar design **is the chosen design today**
because we haven't measured a workload where Ra loses to PG
on plan quality (only on plan time, where Ra wins by 89×).
RFC 0089 ships when the workload demand changes — the
leading candidates are TPC-H 7-12 way joins, sort-rich
analytical queries (TPC-DS), and cardinality-skewed joins
where physical-method choice influences the optimal join
order.

See [RFC 0087](../../rfcs/text/0087-physical-operator-selection.md)
for the formal architectural comparison and
[RFC 0089](../../rfcs/text/0089-egraph-cost-driven-physical-lowering.md)
for the e-graph migration plan.

### Filter peephole for `TID_SCAN` and `BITMAP_HEAP_SCAN`

Both of these scan strategies require predicate context that
isn't visible at the leaf `Scan` node — a `ctid =` clause for
TidScan, a column-equality on an indexed column for
BitmapHeapScan. The plan-builder handles them via a peephole
in the `Filter` arm of `build_plan`: when the immediate input
is a base `Scan` with the corresponding advice, the filter's
predicate is consumed by the specialized builder and the
result returned directly (no separate Filter wrapping). When
the predicate doesn't have the required shape, both builders
return `Err(reason)` and the path falls through to the
standard `Filter`+`SeqScan` chain. This is honest production
behavior: when advice can't be honored, EXPLAIN reflects
reality rather than emitting a malformed plan.

## See also

- [`docs/research/pg-plan-advice-port.md`](../research/pg-plan-advice-port.md)
  — the full implementation plan.
- [`rfcs/text/0087-physical-operator-selection.md`](../../rfcs/text/0087-physical-operator-selection.md)
  — design for physical-operator selection (the sidecar
  approach in production today).
- [`rfcs/text/0088-fdw-pushdown-foreign-join.md`](../../rfcs/text/0088-fdw-pushdown-foreign-join.md)
  — design for honoring `FOREIGN_JOIN` via PG FDW pushdown.
- [`rfcs/text/0089-egraph-cost-driven-physical-lowering.md`](../../rfcs/text/0089-egraph-cost-driven-physical-lowering.md)
  — design for moving physical decisions into the e-graph.
- [PostgreSQL plan-advice documentation](https://www.postgresql.org/docs/current/pgplanadvice.html)
  — upstream reference.
- `~/src/postgres/contrib/pg_plan_advice/README` — the design
  rationale, written by the upstream authors.
