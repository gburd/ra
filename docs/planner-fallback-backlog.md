# Planner Fallback Backlog — toward zero native-planner fallbacks

**Goal.** When `ra_planner.enabled = on`, *every* query should be planned by
Ra. Any fallback to PostgreSQL's native planner is a coverage gap: a feature
Ra must implement. This document is the task list of those gaps. We do **not**
remove the fallback safety net — falling back is always correct — but each
fallback path here is a bug we intend to close.

## How fallbacks surface

The planner hook logs every fallback when `ra_planner.log_decisions = on`,
naming the cause:

- `ra_planner: parse fell back to PG: <error>` — Lime grammar gap.
- `ra_planner: optimize fell back to PG: <error>` — e-graph/optimizer gap.
- `ra_planner: plan-build fell back to PG: <Operator> not yet supported …` —
  plan_builder gap. The `<Operator>` token comes from
  `PlanBuilder::first_unsupported_op` and maps directly to a task below.
- `ra_planner: inner panic, falling back …` — a Ra-side panic (should be rare;
  treat as a bug to root-cause, not a normal gap).

**Process to close a gap:** implement the operator in `plan_builder`, admit it
to `first_unsupported_op` (return `None`), then prove it with
`scripts/replan-equivalence-test.sh` (replan under varied advice + statistics;
result rows must equal the native planner). Only operators that pass the
property test are allowed to stay enabled.

**Expression-level fail-safe.** A plan is also rejected (→ native planner) when
any filter predicate or projection column cannot be faithfully translated to a
PG expression. This prevents silently dropping an untranslatable qual (which
returned unfiltered rows). Note: a *runtime* error from a malformed-but-built
plan (e.g. a missing collation) is NOT caught by the planner-hook fallback, so
expression translation must be correct, not merely non-null — see the text
collation fixes (`varcollid` / `inputcollid` / `constcollid`).

## Currently supported (no fallback)

- Single-relation `Scan` (SeqScan) with any nesting of `Filter` (→ qual) and
  `Project` (→ targetlist), including qualified and unqualified columns, NULL
  tests (`IS [NOT] NULL`), and text/collation-sensitive comparisons.
- `Sort` (`ORDER BY`, single or multi-key, ASC/DESC, NULLS FIRST/LAST, aliases)
  and `Limit`/`OFFSET`, when every sort key is a plain column that appears in
  the output. Verified row-equivalent on a live PG18 cluster.

## Plan-builder gaps (each = one task)

Priority P0 (common, highest value), P1 (common), P2 (specialized).

| Op token | SQL it blocks | Status / why it falls back | Pri |
|---|---|---|---|
| ~~`Join`~~ | ~~multi-table join~~ | **DONE** for Inner/Left/Cross over two base relations (build_projected_join, NestLoop). Right/Full/Semi/Anti and 3+ table joins still defer. Fixing this also fixed two latent **optimizer** correctness bugs (left-deep dropped a WHERE predicate / rebuilt the join as a cartesian product; left-deep converted LEFT/RIGHT/FULL joins to INNER) | P2 |
| ~~`Aggregate`~~ | ~~`count/sum/avg/min/max`, `GROUP BY`~~ | **DONE** for count/sum/avg/min/max (± GROUP BY, ± ORDER BY). HAVING, expressions over aggregates, DISTINCT aggregates, and stddev/variance/string_agg/array_agg still defer | P2 |
| ~~`Sort`~~ | ~~`ORDER BY`~~ | **DONE** (plain-column keys); expression keys and `ORDER BY` of a non-output column still defer (need resjunk targetlist / ordering-operator resolution) | — |
| ~~`Limit`~~ | ~~`LIMIT` / `OFFSET`~~ | **DONE** | — |
| ~~`Distinct`~~ | ~~`SELECT DISTINCT`~~ | **DONE** — `build_unique` sorts its input on all output columns (Sort+Unique) | — |
| ~~`Union` / `Intersect` / `Except`~~ | set operations (+ `ALL`) | **DONE** — UNION/UNION ALL (Append+dedup), INTERSECT/EXCEPT (+ALL) via PG18 hashed SetOp | — |
| ~~`Window`~~ | ~~window functions~~ | **DONE** for row_number/rank/dense_rank and sum/count/avg/min/max OVER (PARTITION BY/ORDER BY, default frame, single spec). Explicit frames, multiple window specs, and lag/lead/ntile/nth_value/first_value/last_value defer | P2 |
| ~~`Values`~~ | `VALUES (...)` | **DONE** — ValuesScan over PG's RTE_VALUES (single and multi-row) | — |
| ~~`CTE`~~ / `RecursiveCTE` | `WITH` | **CTE DONE** — non-recursive CTEs inlined with range-table flattening (cte_flatten_rtes + fresh rtable copy in build_planned_stmt). RecursiveCTE and multi-relation/non-simple CTE bodies defer | P2 |
| `IndexScan` | index / index-only scans (optimizer- or advice-chosen) | `build_index_scan*` unverified; scan-strategy advice not physically honored | P2 |
| `BitmapScan` | bitmap heap/index/and/or scans | `build_bitmap_*` crashed the backend (the removed Filter peephole) | P2 |
| `Parallel` | parallel scan/hash-join/agg, `Gather` | not verified | P2 |
| `Unnest` | `UNNEST(...)`, `MultiUnnest` | not verified | P2 |
| `TableFunction` | table functions in `FROM` | not verified | P2 |
| `MvScan` | materialized-view scans | not verified | P2 |
| `VectorSearch` | `TopK` / `VectorFilter` (ORDER BY distance LIMIT k) | not verified | P2 |
| `RowPattern` | `MATCH_RECOGNIZE` execution | not verified | P2 |
| `GraphTable` | `GRAPH_TABLE` (SQL/PGQ) | modeled; deferred to PG19 native machinery | P2 |
| `Insert` / `Update` / `Delete` / `Merge` | DML | `build_modify_table_from_dml` unverified; MERGE not lowered | P2 |

## Parser gaps (Lime grammar — `parse fell back`)

| Feature | Status | Pri |
|---|---|---|
| `PIVOT` / `UNPIVOT` | not parsed | P2 |
| `XMLTABLE` | not parsed | P2 |
| `MATCH_RECOGNIZE` | not parsed | P2 |
| (general) any syntax not in `ra_sql.lime` | add grammar + `RelExpr` mapping | — |

## Optimizer gaps (e-graph — `optimize fell back`)

| Feature | Status | Pri |
|---|---|---|
| IN / EXISTS / NOT IN / NOT EXISTS / derived tables | **DONE** — decorrelated to semi/anti joins (built as NestLoop) + SubLink range-table flattening; passthrough derived tables flattened. **Scalar subqueries** `(SELECT ...)` in expressions still fall back (need SubPlan/InitPlan) | P2 |
| (general) any `RelExpr` whose `to_rec`/`from_rec` round-trip is lossy | extend e-graph encoding | — |

## Known bugs causing fallback (not operator gaps)

| Bug | Trigger | Status | Pri |
|---|---|---|---|
| Index-stats double-free | planning over an indexed table (intermittent) — `pfree invalid pointer 0x7f…` — **contained** (catch_unwind → no index stats; correctness unaffected) and NOT reproducible in 750 stats-heavy queries; needs ASan + a repro to root-cause | A `pfree` of an already-freed pointer in `stats_bridge` index-stats gathering. PG's `pfree` guard catches it (no corruption) and the planner-hook `catch_unwind` falls back, so results stay correct; but it discards Ra's plan. Wrapped `gather_index_stats` in `catch_unwind` reduced but did not eliminate it — not yet root-caused (became too rare to capture a backtrace). Use `backtrace_functions='BogusFree'` on an assert build to catch the caller. | P1 |

## Definition of done

Zero fallbacks means: across a representative workload (and the JOB / TPC-H
suites), `ra_planner.log_decisions = on` reports no `fell back` lines, and the
replan-equivalence test passes for every shape exercised.

## Remaining deep gaps (fall back safely)

After the operator work, Ra natively plans the full common SQL surface
(scan/filter/project, sort, limit, distinct, aggregates, joins, window
functions, all set operations, VALUES, CTEs, derived tables,
IN/EXISTS/NOT IN/NOT EXISTS subqueries, **scalar subqueries**, and
**simple WITH RECURSIVE**). The previously-tracked deep gaps are now
resolved:

- **Scalar subqueries** `(SELECT ...)` in projection/WHERE expressions —
  **DONE.** Built as `EXPR_SUBLINK` `SubPlan` nodes; correlated
  outer-Var references in the inner plan are replaced with `PARAM_EXEC`
  `Param`s (parParam/args), and `PlannedStmt.subplans`/`paramExecTypes`
  are populated. Correlated and uncorrelated forms verified
  row-equivalent to native PG.
- **`WITH RECURSIVE`** — **DONE, including base-relation joins.**
  Built as `CteScan` over a `RecursiveUnion{anchor Result, recursive
  WorkTableScan}`, reusing PG's existing `RTE_CTE` and threading the
  cte/worktable `PARAM_EXEC` params. The join builder resolves a join
  side that references the in-scope CTE to its WorkTableScan/CteScan, and
  `flatten_rtes` pulls the joined base relations out of the recursive
  term's set-operation arms. Verified row-equivalent to native PG:
  counters, multi-column depth tracking, graph traversal (CTE joined with
  an edges table on either side), bodies joining the CTE with a base
  relation, and aggregate / GROUP BY / ORDER BY / WHERE over the CTE.
  `UNION` (distinct) recursive CTEs still defer cleanly (only `UNION ALL`
  is built natively).
- **Index-stats double-free** — **not a demonstrable bug.** Two code
  inspections found no double-free: `list_free(RelationGetIndexList(rel))`
  matches PG core (the returned list is caller-owned) and
  `resolve_am_type`'s `pfree(get_am_name(...))` is null-guarded. Not
  reproducible under aggressive concurrent index-DDL churn + hundreds of
  concurrent stats-gathering queries (0 safety events). Fully contained
  by `catch_unwind` (degrades to no index stats; correctness unaffected).
  No speculative fix applied per the no-phantom-features standard.

### Remaining narrow fallbacks
### Correctness fixes & coverage (2026-05-31, differential audit)
Bugs found by Ra-vs-PG differential audit and fixed:
- **Recurring backend abort**: `monitor::maybe_refresh` ran SPI inside the
  planner hook with no re-entrancy guard, recursing into nested SPI until
  abort (~once per 1s refresh under load). Added a re-entrancy guard.
- **DISTINCT aggregate wrong result**: the parser dropped the DISTINCT
  flag, so `count(DISTINCT x)` planned as `count(x)`. DISTINCT args are now
  wrapped in a `__distinct` marker → safe fallback (native DISTINCT-agg TBD).
- **CAST coercion**: `build_cast` always used CoerceViaIO; `(bool)::int`
  errored. Now resolves via `find_coercion_pathway` (FuncExpr / RelabelType
  / CoerceViaIO).
- **Function-call collation**: `upper()`/`lower()` errored ("could not
  determine collation"); `build_func_expr` now sets input/result collation.
- **Correlated IN/ANY/ALL wrong result**: decorrelation left the
  correlation predicate inside the inner side (unreachable by Ra's
  nested-loop); now pulled into the join condition.

Also added natively: `count(col)` (polymorphic count("any")), `OFFSET`
without `LIMIT`, `NULLIF`, `GREATEST`/`LEAST`.

### Expression / aggregate coverage added (2026-05-31)
Now built natively (verified row-equivalent on PG18), previously fell back:
- `LIKE` / `NOT LIKE` / `ILIKE` (the `~~` / `~~*` operators).
- `COALESCE(...)` (CoalesceExpr; same-type arguments, else defers).
- `IN (value-list)` / `NOT IN (value-list)` as a `ScalarArrayOpExpr`
  (also fixed a latent parser bug that dropped the tested operand).
- Aggregates nested in expressions, scalar and grouped
  (`max(id)-min(id)`, `sum(id)/count(*)`, `sum(id)+grp`).
- `HAVING` (single, aggregate-expression, group-column, and AND/OR
  conditions) as the Agg node's qual.

### Remaining narrow fallbacks
- `UNION` (distinct) recursive CTEs (only `UNION ALL` is built natively).
- Recursive terms/bodies with a 3+ way join (the projected-join builder
  handles two relations; deeper joins defer cleanly).
- 3+ way and self joins generally (projected-join builds two relations).
- No-FROM `SELECT <expr>` (standalone) defers to native PG.
- `IN (subquery)` non-decorrelatable shapes, GROUPING SETS/ROLLUP/CUBE,
  DISTINCT ON, aggregate FILTER, ORDER BY arbitrary expression, and
  non-default window frames still defer.

### Rules-system review (recent SQL constructs)
Scalar sub-queries and recursive CTEs need **no new `.rra` rewrite
rules** — they are lowering concerns (SubPlan / RecursiveUnion
construction in `plan_builder`, correctly in Rust). The e-graph treats
`RecursiveCTE` as opaque (like `GraphTable`); scalar sub-queries lower to
`SubPlan`. One piece of rule-like logic was mistakenly in Rust lowering:
the "projection of aggregate functions over a non-Aggregate input → no-
GROUP-BY Aggregate" normalization in `plan_builder` (a workaround for the
parser not recursing into sub-queries). It has been relocated to the
parser's `apply_all` (`normalize_subqueries`), so every consumer —
including the e-graph optimizer — sees normalized sub-queries, and the
lowering workaround is removed.


## Differential audit findings (2026-06-01)

### Wrong results
An exhaustive Ra-vs-PG sweep (~60 diverse shapes this round, on top of the
five wrong-results fixed previously: DISTINCT aggregates, CAST coercion,
function collation, correlated IN/ANY/ALL, and the monitor re-entrancy
abort) found **no new wrong results**. NULL handling, set ops, casts,
collation, aggregates/HAVING, joins, subqueries, and recursive CTEs are all
row-equivalent to native PostgreSQL.

### Suboptimal plans (top priority performance gap)
**Ra emits a `SeqScan` for selective equality predicates on indexed
columns** instead of an index scan. Measured: 200× `SELECT id,v FROM big
WHERE id=1234567` on a 2M-row indexed table took 0.08 s with Ra vs 0.01 s
native (8× slower). The plan builder *has* `build_index_scan` /
`build_index_only_scan` / `build_bitmap_heap_scan` / `build_merge_join`,
but `IndexScan`/`IndexOnlyScan`/`BitmapScan` `RelExpr`s are gated to
fallback, and the optimizer does not emit them for indexed predicates.
Forcing `INDEX_SCAN` advice produced correct results but no speedup.

**Precise root cause (2026-06-01).** *Every* index-access builder
(`build_index_scan`, `build_index_only_scan`, `build_bitmap_index_scan`)
sets only `scanrelid` + `indexid` and **never sets `indexqual`**. With no
index condition the executor scans the whole index, so the result is
correct but no faster than a SeqScan. The predicate currently survives only
as a recheck `qual` folded onto the scan by the `Filter` arm of
`build_plan`. Worse, `RelExpr::IndexScan` carries only `{ table, column }`
— it has **no field to represent the index condition** (unlike
`BitmapIndexScan`/`IndexOnlyScan`, which do carry a `predicate`).

**Why this is RFC-scale, not a wiring fix.** Closing it requires all of:
1. A way to carry the index condition to the builder — either add a
   `condition` field to `RelExpr::IndexScan` (ripples through `to_rec`/
   `from_rec`, optimizer, every match site) or plumb the parent `Filter`
   predicate through a peephole.
2. Canonical-form `indexqual` construction: an `OpExpr` whose indexed side
   is `Var(varno=INDEX_VAR, varattno=indexcol+1)` and whose operator comes
   from the index's **opfamily** (e.g. btree `=` strategy), plus
   `indexqualorig` (heap-Var form) and `indexqualcols`. This is
   executor-coupled: `ExecIndexBuildScanKeys` is strict about the form, and
   a malformed qual crashes the backend or returns wrong rows.
3. Optimizer support to actually emit index access paths for selective
   indexed predicates (cost-based), then gate admission + exhaustive A/B +
   crash testing.

This is the same executor-coupled class that the removed scan-strategy
advice peephole fell into — the `Filter` arm comment records it "produced
backend-crashing plans." Per the correctness mandate (prefer safe fallback
over a wrong/crashing plan), this should be done as a focused,
verification-first effort (single-column btree equality first, behind the
opt-in advice path, validated by `scripts/replan-equivalence-test.sh` for
correctness *and* timing) — not rushed. Until then Ra is
correctness-complete but produces simpler (SeqScan/NestLoop) physical plans
than PG for indexed / large-table queries.

### EXPLAIN transparency
`EXPLAIN` is a utility statement, so the planner hook skips it and EXPLAIN
shows PostgreSQL's native plan, not the plan Ra actually builds for
execution. For an extension that advertises new planner behaviour this is a
transparency gap worth closing (e.g. an `EXPLAIN (RA_PROVENANCE)` option, as
sketched in planner_hook.rs).
