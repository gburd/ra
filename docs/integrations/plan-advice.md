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

## What's next

The
[port plan](../research/pg-plan-advice-port.md)
documents the remaining work:

- **Bitmap-index combination.** The cost-driven layer in
  [RFC 0087](../../rfcs/text/0087-physical-operator-selection.md)
  handles compound-index column-prefix matching with
  primary-key/unique tie-breaking, and the plan-builder honors
  `BITMAP_HEAP_SCAN` advice when a parent `Filter` covers an
  indexed column. What's not yet covered: bitmap-AND / bitmap-OR
  combination across multiple indexes (PG's `BitmapAnd` /
  `BitmapOr` plan nodes synthesized from independent index
  conditions). Adding this requires walking AND/OR conjunctions
  to enumerate covering-index sets and emit
  `BitmapIndexScan` -> `BitmapAnd` -> `BitmapHeapScan`.
- **Selectivity-aware index comparison.** When multiple
  indexes have the same prefix length, the current code
  breaks ties by primary > unique > regular. PG additionally
  considers histogram-driven selectivity per predicate.
  Wiring this needs the cost-model layer to read column NDV /
  MCV statistics, which `Statistics::columns` already exposes
  but `pick_scan_strategy` doesn't yet consult.
- **`DO_NOT_SCAN` semantics.** This is a *negative* constraint
  ("do not produce a scan of `t`"). Honoring it requires the
  e-graph to express join-eliminated plans for the table and
  pick them in extraction. Today the advice is parsed and
  recorded in `PhysicalChoices`, the cost penalty fires, but
  the plan-builder logs and falls back to SeqScan. Documented
  in RFC 0087.
- **`FOREIGN_JOIN` semantics.** Requires FDW pushdown
  machinery (deparse, `GetForeignJoinPaths`, etc.) — beyond
  this RFC's scope. Documented in RFC 0087.
- **Cost-driven physical lowering inside the e-graph.** Today
  the e-graph extracts a logical `RelExpr` (`Scan`, `Join`,
  `Aggregate`) and physical strategy lives in the
  `PhysicalChoices` sidecar map consumed at PG-Plan emission
  time. A future RFC may add e-graph rewrite rules that lower
  `Join` to `HashJoin` / `MergeJoin` / `NestLoop` directly,
  letting the cost extractor reason about logical plan shape
  and physical method together. Multi-week refactor; see
  RFC 0087 for the design comparison.

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
  — design for physical-operator selection.
- [PostgreSQL plan-advice documentation](https://www.postgresql.org/docs/current/pgplanadvice.html)
  — upstream reference.
- `~/src/postgres/contrib/pg_plan_advice/README` — the design
  rationale, written by the upstream authors.
