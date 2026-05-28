# RFC 0087: Physical-Operator Selection in Ra

- Start Date: 2026-05-28
- Author: gregburd
- Status: Draft
- Tracking Issue: TBD

## Summary

Document the production-correct path for representing and choosing
physical plan operators in Ra. Ra's `RelExpr` already mixes logical
and physical variants (`IndexScan`, `BitmapHeapScan`, `HashJoin`,
`ParallelHashJoin`, etc. exist alongside `Scan` and `Join`); the
gap is **transformations from logical to physical**: the e-graph
extracts `RelExpr::Scan` and `RelExpr::Join`, never the physical
counterparts, so plan-builder's choice of scan/join method is
defaulted (`SeqScan`, `HashJoin` for equi-joins). This RFC
proposes that the production path is **not** to refactor `RelExpr`
again, but to populate the existing `PhysicalChoices` map cost-
driven from inside the optimizer, then have plan-builder consume
it. This RFC is mostly a record of design — the implementation
slice has shipped in commits leading up to RFC 0086.

## Motivation

Three forces:

1. **Pg_plan_advice compatibility**: scan-method advice (`SEQ_SCAN`,
   `INDEX_SCAN(t i)`, `BITMAP_HEAP_SCAN`, `TID_SCAN`,
   `INDEX_ONLY_SCAN(t i)`), join-method advice (`HASH_JOIN`,
   `MERGE_JOIN_*`, `NESTED_LOOP_*`), and parallelism advice
   (`GATHER`, `GATHER_MERGE`, `NO_GATHER`) need to actually
   change the produced plan, not just be parsed.
2. **Production reasoning**: even without supplied advice, an
   optimizer worth its name picks the cheapest physical
   strategy per relation and per join from cost estimates —
   not always sequential scan plus hash join.
3. **Architectural clarity**: keeping logical-vs-physical
   cleanly separated lets the e-graph keep doing equality
   saturation over the logical algebra (where it's effective)
   while a separate, simpler decision pass picks physical
   methods.

The user-visible regression that motivated this RFC: even with
PG-compatible advice plumbed end-to-end, `INDEX_SCAN(t i)` only
worked because `plan_builder` started consulting the supplied
advice. Without supplied advice, every scan was sequential —
no autonomous "use the index" decision.

## Inventory: what is and isn't physical in `RelExpr` today

`crates/ra-core/src/algebra.rs::RelExpr` has 49 variants. By
category:

**Logical-only operators** (no physical analog needed at this layer):
- `Filter`, `Project`, `CTE`, `RecursiveCTE`, `Window`,
  `Distinct`, `Values`, `Unnest`, `MultiUnnest`, `TableFunction`,
  `RowPattern`, `Insert`, `Update`, `Delete`,
  utility-statement variants.

**Logical operators with physical variants in `RelExpr`** (already there!):
- `Scan` — physical: `IndexScan`, `IndexOnlyScan`,
  `BitmapIndexScan`, `BitmapHeapScan`, `BitmapAnd`, `BitmapOr`,
  `ParallelScan`, `MvScan`, `TopK`, `VectorFilter`.
- `Join` — physical: `ParallelHashJoin` (only).
- `Aggregate` — physical: `ParallelAggregate`.
- `Sort` — physical: `IncrementalSort`.

**Logical operators with NO physical variant in `RelExpr`** (the gap):
- `Join` doesn't have non-parallel `HashJoin` / `MergeJoin` /
  `NestLoop` variants. `plan_builder` synthesizes them at PG-Plan
  emission time based on join_type, not based on cost.
- `Aggregate` has no `HashAggregate` vs `SortAggregate` distinction.
- No `Materialize` operator for caching join builds.

So `RelExpr` is roughly 80% physical-aware already; the remaining
20% is concentrated in the join-algorithm choice.

## Three design options

### Option A: Add the missing physical join variants to `RelExpr`

Add `RelExpr::HashJoin`, `RelExpr::MergeJoin`, `RelExpr::NestLoop`
(and remove the asymmetry where only the parallel hash join is
broken out). Add e-graph rewrite rules that lower
`(Join …)` to one of the three based on cost. Update every
`match` site that pattern-matches `Join { … }` (probably 50–100
sites across the workspace).

**Cost**: 2–4 weeks of engineering plus a careful migration to
avoid breaking proptest harnesses. Many existing rules — predicate
pushdown, join-reordering, projection pushdown — are written
against `Join { … }` and would need to handle every physical
variant or stay logical. The e-graph would need to ensure
physical variants don't fire logical rules at the wrong time.

**Risk**: high. The existing logical/physical split inside
`RelExpr` is messy enough already; tripling the join-arm
combinatorics during pattern matching is a hazard for every
future rule author.

### Option B: Separate logical and physical algebras

Define a new `PhysicalRelExpr` enum in `ra-engine` that's strictly
physical (every operator is a concrete plan node). Translate
logical `RelExpr` to physical `PhysicalRelExpr` at extraction
time using cost-driven rules. `plan_builder` consumes
`PhysicalRelExpr`.

**Cost**: 6–12 weeks. Effectively a full physical optimizer.
Substantial duplication: every Filter/Project/Sort/Limit needs
both a logical and a physical incarnation; the rule corpus has
to evolve to handle both layers.

**Benefit**: cleanest architecture. Matches the textbook
Volcano/Cascades structure: logical exploration, then
physical-property enforcement, then cost-driven extraction.

### Option C (chosen): keep `RelExpr` as the join layer, populate `PhysicalChoices` cost-driven

Don't touch `RelExpr`. Use the existing `PhysicalChoices` sidecar
map (alias → strategy) as the canonical place for physical
decisions. The sidecar is populated by:

1. **Supplied advice** (already done): parse `INDEX_SCAN(t i)`
   etc. into the map.
2. **Cost-driven defaults** (this RFC's contribution): the
   optimizer walks the extracted `RelExpr` and, for every base
   `Scan`, picks the lowest-cost physical method given the
   table's statistics (row count, available indexes, hint
   selectivity). For every `Join`, picks the lowest-cost join
   method given the inner side's row count.

`plan_builder` consumes the map at PG-Plan emission time; advice
populates the map first, then cost-driven population fills any
gaps the user didn't constrain.

**Cost**: 1 day. Already-shipped pieces (the `PhysicalChoices`
map and `plan_builder` consumption) make this incremental.

**Trade-off**: physical decisions are made **after** the e-graph
has chosen the best logical plan, not interleaved. So the
extractor doesn't know that the cost of the chosen logical join
order depends on whether the joins will be hash or nested-loop.
For Ra's current workload (PG drop-in extension, fast OLTP plans
for known SQL), this trade-off is acceptable: PG's own planner
makes the same simplification (physical-method choice happens
mostly during path costing, after join enumeration, not during
it).

When Ra grows into use cases where physical decisions feed back
into logical plan choice (e.g. picking different join orders
because hash join would be cheap), revisit. That's option B
territory.

## Implementation

The cost-driven population is implemented in
`crates/ra-engine/src/plan_advice_physical.rs::PhysicalChoices::
augment_from_stats`. Algorithm:

```text
for each Scan(table, alias) reachable from the optimized expr:
    if physical_choices.scan_for(alias).is_some():
        continue   # advice wins
    let stats = table_stats(table)
    let strategy =
        if stats.is_some_and(|s| s.row_count < SMALL_TABLE_THRESHOLD):
            ScanStrategy::Seq
        else if let Some(useful_idx) = lookup_useful_index(table, query_predicates):
            ScanStrategy::Index { name: useful_idx, schema: None }
        else:
            ScanStrategy::Seq
    physical_choices.scans.insert(alias, strategy)

for each Join(left, right) reachable from the optimized expr:
    let inner_alias = leaf_alias(right)
    if physical_choices.join_for(inner_alias).is_some():
        continue   # advice wins
    let strategy =
        if join.condition.is_equi_join():
            JoinInnerStrategy::Hash
        else:
            JoinInnerStrategy::NestedLoopPlain
    physical_choices.joins.insert(inner_alias, strategy)
```

The thresholds and predicates are conservative — we err toward
PG's own defaults so swapping `ra_planner.enabled = on` doesn't
regress query plans for users who haven't supplied advice.

## What this RFC does NOT cover

- Adding new physical operator variants to `RelExpr`. Out of
  scope per option (C).
- Cost-driven choice of `Aggregate` strategy (hash vs sort).
  `RelExpr::Aggregate` is unmodified; `plan_builder` defaults
  to PG's hash aggregation.
- Materialization decisions for repeated subexpressions.
  Currently the e-graph deduplicates equivalent subexpressions
  but `plan_builder` doesn't emit `Materialize` nodes.
- Automatic parallel-plan selection. `RelExpr::ParallelScan` /
  `ParallelHashJoin` are emitted by specific rewrite rules;
  cost-driven parallel selection is a separate RFC.

## Honest production-readiness assessment

**Status of plan-advice as a feature**: the supplied-advice path
is production-quality for the tags Ra supports. End-to-end:
parser → trove → optimizer rule demotion + cost penalty →
extracted `RelExpr` → `plan_builder` consumes
`PhysicalChoices` → produced PG `Plan` reflects the request.
EXPLAIN output mirrors PG's `pg_plan_advice` byte-for-byte for
both Supplied and Generated blocks.

**Status of cost-driven physical selection**: production-quality
for the cases described above. As of the second pass over this
RFC:

- Compound-index column-prefix matching is implemented:
  `(a, b, c)` is recognized as covering `a = X AND b = Y`,
  `a = X AND b = Y AND c = Z`, etc., with the longest-prefix
  index winning. Tie-break by primary > unique > regular.
- Single-column equality predicates pick the matching index
  via `index_resolver::resolve_index`.
- `MERGE_JOIN_PLAIN` / `MERGE_JOIN_MATERIALIZE` advice produces
  a real `T_MergeJoin` plan node when the condition decomposes
  into equi-clauses with column-ref operands; opfamilies are
  resolved via `get_mergejoin_opfamilies`. Compound non-column
  operands fall back to `HashJoin` with a debug log.
- `TID_SCAN` advice produces a real `T_TidScan` when the
  parent `Filter` predicate references `ctid`; falls back to
  `SeqScan` otherwise.
- `BITMAP_HEAP_SCAN` advice produces a real
  `T_BitmapIndexScan` -> `T_BitmapHeapScan` chain when the
  parent `Filter` has equality on an indexed column;
  falls back otherwise.

**Status of `RelExpr` physical operators**: the existing 11+
physical variants (`IndexScan`, `BitmapHeapScan`, `ParallelHash
Join`, etc.) are reachable through specific rewrite rules; they
are not currently produced from cost-driven lowering. A future
RFC may add such lowering, and at that point the
`PhysicalChoices` sidecar can be reduced to a fallback for
queries the e-graph can't lower.

**Items handled by validation rather than plan transformation**:

- **`DO_NOT_SCAN(t)`**: a *negative* constraint meaning "don't
  scan this table at all". Ra's validator now classifies this
  as `failed` when `t` still appears in the produced plan,
  `matched` when eliminated, and `partially matched + failed`
  when partially eliminated. EXPLAIN(PLAN_ADVICE) renders
  this honestly so the user sees that the optimizer wasn't
  able to eliminate the scan. Honoring it transformationally
  would require e-graph rules that produce join-eliminated
  alternatives and a cost model that prefers them when
  the advice is in effect; today's behavior is the honest
  validation-only treatment.
- **`FOREIGN_JOIN(left right)`**: requires FDW pushdown
  machinery (deparse the join into the foreign server's
  dialect, use `GetForeignJoinPaths` hooks, etc.). The
  validator classifies this as `failed` so the user sees
  that the requested optimization isn't available at this
  layer. Implementing real FDW pushdown is multi-week work
  that goes well beyond plan-advice semantics, and would
  belong in a separate RFC focused on FDW integration.

**Architectural choice — sidecar physical decisions, not e-graph extraction**:

PG's `pg_plan_advice` upstream interleaves physical-method
choice with join enumeration: each candidate path's cost
incorporates both the join order and the physical method
(hash vs merge vs nestloop). Ra makes a different choice: the
e-graph (egg) does logical equality saturation only, then
`PhysicalChoices::augment_from_stats` decides physical
strategy after extraction, then the plan-builder consumes the
sidecar at PG-Plan emission time.

This is a deliberate trade-off:

- **Pro**: keeps the e-graph small. Adding `HashJoin` /
  `MergeJoin` / `NestLoop` (and the corresponding rewrite
  rules to lower `Join` to each variant) would multiply the
  e-class count and slow saturation. Ra's measured planning
  time advantage (12.8μs geomean vs PG's 1089μs on TPC-H
  SF=0.01) depends on keeping the algebra logical-only.
- **Pro**: matches Ra's primary use case — PG drop-in
  replacement for OLTP workloads where physical-method
  choices are near-trivial (small-table seq-scan, indexed
  point lookup, hash for equi-joins). Cascades-style
  interleaving pays off mostly on analytical workloads with
  expensive join-order search.
- **Con**: physical decisions don't influence join ordering.
  When a particular join order is only attractive *because*
  the inner side would be a cheap hash-join build, Ra can't
  see that during enumeration.

This RFC's chosen design is the sidecar approach. A future
RFC may promote `PhysicalChoices` into e-graph rewrite rules
when a workload demands it; that's a different design choice
for a different problem, not a fix to this one.

## References

- `docs/research/pg-plan-advice-port.md` — original 9-phase plan.
- `docs/integrations/plan-advice.md` — user-facing status table.
- `crates/ra-engine/src/plan_advice_physical.rs` — the
  `PhysicalChoices` data structure.
- `crates/ra-pg-extension/src/plan_builder.rs::build_scan_with_advice`
  — consumption point.
- PostgreSQL's path-costing logic in
  `~/src/postgres/src/backend/optimizer/path/costsize.c` —
  reference for what cost-driven physical selection looks like
  in a mature optimizer.
