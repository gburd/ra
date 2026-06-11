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

- Single-relation `Scan` with any nesting of `Filter` (→ qual) and
  `Project` (→ targetlist), including qualified and unqualified columns, NULL
  tests (`IS [NOT] NULL`), and text/collation-sensitive comparisons. Physical
  access path is chosen by plan_builder peepholes: a real `IndexScan`
  (`col <op> const`, ranges, multi-column prefixes), a covering `IndexOnlyScan`
  (a btree index covers all projected + predicate columns), or a
  `BitmapHeapScan` (top-level OR of indexed-column conditions) — otherwise a
  `SeqScan`. All verified row-identical to PG.
- `Sort` (`ORDER BY`, single or multi-key, ASC/DESC, NULLS FIRST/LAST, aliases)
  and `Limit`/`OFFSET`, when every sort key is a plain column that appears in
  the output. Verified row-equivalent on a live PG18 cluster.
- **All join types** — INNER / LEFT / RIGHT / FULL / CROSS outer joins and
  SEMI / ANTI joins (from decorrelated `IN`/`EXISTS`/`NOT IN`/`NOT EXISTS`),
  including multi-way joins, joins with `WHERE` on either side, and
  `IN`/`EXISTS` families. Verified row-identical to PG19 (see
  `docs/ra-vs-pg-correctness-findings-2026-06-07.md`). The outer-join
  correctness arc fixed three optimizer bugs: `references_only` was a no-op for
  scalar predicates (now uses an analysis `qualifiers` set); generic
  `(join ?type …)` rewrites pushed predicates to the nullable side / commuted
  outer joins (now guarded by `is_inner_join`); and an unguarded generated copy
  of `duckdb-filter-through-left-join-left` pushed a right-relation predicate
  onto the left scan (now guarded by `references_only`).

## Correctness gate — the one remaining wrong-result fallback

`PlanBuilder::wrong_result_risk` defers a query to PG when it contains a
**scalar subquery in a filter predicate** (e.g. `WHERE x < (SELECT avg(y) FROM
t)`). Decorrelation lowers this to a cross/semi join whose inner side is an
`Aggregate` and whose filter references the aggregate **result** as a function
expression, not a `Var` — so it needs PostgreSQL `SubPlan`/`InitPlan` +
`PARAM_EXEC` wiring (an executor-coupled, RFC-scale mechanism), not a rewrite
rule. Falling back is correct; closing it is tracked as a dedicated task.

## Known performance gap (correct, but slower planning)

- **`UNION` / set-op planning ~100× slower than PG (root-caused 2026-06-10):**
  a 2-table `UNION ALL` optimizes in ~204ms vs PG ~2ms. Measured: `optimize=204ms`,
  constant regardless of 2- vs 3-way, i.e. it runs to the `default_timeout_ms_for_tables`
  budget (200ms for 2–4 tables) and terminates with `term=timeout, iters=1, nodes=36`.

  **On-backend profiling (macOS `sample` of a live backend, 2026-06-10) — corrects the
  earlier "e-graph node explosion" guess, which was wrong (the e-graph stays at 36 nodes):**
  - A standalone egg `Runner` over the same rules + the same 36-node set-op e-graph
    saturates in ~0.08ms. So egg matching is *not* inherently slow on this shape; the
    slowdown is specific to the in-backend run.
  - Sampling the **`EXPLAIN`** (plan-only) path is unambiguous: the hot leaves are all
    PostgreSQL functions — `resolve_special_varno` (≈1650 hits), `get_tle_by_resno` (≈940),
    `check_stack_depth` (≈465) — i.e. PG's deparse / plan-ref Var resolution recursing
    deeply. This points at the **plan_builder emitting a degenerate special-varno
    target-list structure for Append/SetOp** (a deep/chained `OUTER_VAR`/`INNER_VAR`
    resolution), which makes PG recurse pathologically when resolving Vars.
  - Sampling the executing path is contaminated by the query's own seq-scans + numeric
    comparisons (and the PG19 ASSERT build's `AllocSetCheck`), so it is not a clean planning
    profile; prefer `EXPLAIN`-loop sampling.

  **Landed (output-preserving, did NOT move the 204ms):** `all_rules_unsorted()` and
  `all_rules_annotated()` now cache their built rule set in a process-local `OnceLock`
  (egg `Rewrite` is `Clone`+`Send`+`Sync`). `load_rules()` called both per optimize,
  re-parsing ~293 patterns each time; sampling showed those construction frames on the hot
  path. Caching removes the per-query rebuild (~a few ms) but the dominant 204ms is plan
  shape / Var-resolution bound, not rule construction — verified by re-measuring (still
  204ms) and by 0 DIFF on the 60-shape requalification suite.

  **Attempted+REVERTED:** per-iteration `egraph.clone()` removal (not the bottleneck —
  egg's clone is cheap). A bypass fast-path is UNSAFE (segfaults — the e-graph performs a
  set-op normalization plan_builder depends on).

  **Next:** inspect `plan_builder`'s Append/SetOp target-list construction — build the
  output Vars so PG's `resolve_special_varno` resolves them in O(1) (reference the child
  tlist resno directly) rather than chaining through nested special varnos. Verify with an
  `EXPLAIN`-loop `sample` (the recursion frames should vanish) + requalify 0 DIFF + the
  set-op `optimize=` time drops to single-digit ms. NOTE (legacy): `UNION` with filters on
  both branches — ~10–15× slower *planning* than PG
  (the produced plan and results are identical). The cost is the per-iteration
  e-graph saturation machinery (each interleaved iteration clones the e-graph
  and spins up a fresh egg `Runner` over the full rule set + scheduler), not a
  single rule. A fix that avoids re-creating the `Runner`/cloning per iteration
  would benefit all queries and is tracked separately.

## Plan-builder gaps (each = one task)

Priority P0 (common, highest value), P1 (common), P2 (specialized).

| Op token | SQL it blocks | Status / why it falls back | Pri |
|---|---|---|---|
| ~~`Join`~~ | ~~multi-table join~~ | **DONE** — all join types (INNER/LEFT/RIGHT/FULL/CROSS + SEMI/ANTI), multi-way, WHERE on either side, IN/EXISTS. Verified row-identical to PG19 | — |
| ~~`Aggregate`~~ | ~~`count/sum/avg/min/max`, `GROUP BY`~~ | **DONE** for count/sum/avg/min/max (± GROUP BY, ± ORDER BY). HAVING, expressions over aggregates, DISTINCT aggregates, and stddev/variance/string_agg/array_agg still defer | P2 |
| ~~`Sort`~~ | ~~`ORDER BY`~~ | **DONE** (plain-column keys); expression keys and `ORDER BY` of a non-output column still defer (need resjunk targetlist / ordering-operator resolution) | — |
| ~~`Limit`~~ | ~~`LIMIT` / `OFFSET`~~ | **DONE** | — |
| ~~`Distinct`~~ | ~~`SELECT DISTINCT`~~ | **DONE** — `build_unique` sorts its input on all output columns (Sort+Unique) | — |
| ~~`Union` / `Intersect` / `Except`~~ | set operations (+ `ALL`) | **DONE** — UNION/UNION ALL (Append+dedup), INTERSECT/EXCEPT (+ALL) via PG18 hashed SetOp | — |
| ~~`Window`~~ | ~~window functions~~ | **DONE** for row_number/rank/dense_rank and sum/count/avg/min/max OVER (PARTITION BY/ORDER BY, default frame, single spec). Explicit frames, multiple window specs, and lag/lead/ntile/nth_value/first_value/last_value defer | P2 |
| ~~`Values`~~ | `VALUES (...)` | **DONE** — ValuesScan over PG's RTE_VALUES (single and multi-row) | — |
| ~~`CTE`~~ / `RecursiveCTE` | `WITH` | **CTE DONE** — non-recursive CTEs inlined with range-table flattening (cte_flatten_rtes + fresh rtable copy in build_planned_stmt). RecursiveCTE and multi-relation/non-simple CTE bodies defer | P2 |
| ~~`IndexScan`~~ | index scans (`col = const`, ranges, multi-col prefixes) | **DONE** — `try_build_index_scan` Filter(Scan) peephole builds a real `T_IndexScan` with canonical INDEX_VAR `indexqual`; verified row-identical to PG | — |
| ~~`IndexOnlyScan`~~ | covering index-only scans | **DONE** — `Project(Filter(Scan))` peephole builds a real `T_IndexOnlyScan` when a btree index covers all projected + predicate columns (`find_covering_index`); INDEX_VAR indexqual + indextlist + empty recheckqual. Verified RA-BUILT + row-identical to PG (main 1886a312) | — |
| ~~`BitmapScan`~~ | bitmap heap/index/or scans | **DONE** for a top-level OR of indexed-column conditions — `Filter(Scan)` peephole builds `BitmapHeapScan → BitmapOr → BitmapIndexScan` (INDEX_VAR indexqual). Verified RA-BUILT + row-identical to PG (main e0ff52c8). **BitmapAnd (multi-index AND) DONE** (main 5983d21d): a top-level AND with >=2 conjuncts on DISTINCT indexes builds `BitmapHeapScan -> BitmapAnd -> BitmapIndexScan`; single-index ANDs stay a plain index scan | — |
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
| Index-stats double-free | **PLATFORM-SPECIFIC (macOS-arm64 / PG19devel), fully contained — NOT reproducible on Linux.** `pfree called with invalid pointer 0x…c08100 (header 0x7f7f7f7f7f7f7f7f)` — a PG palloc-level double-free (0x7f = assert-build CLOBBER_FREED_MEMORY). Caught by the planner-hook `catch_unwind` → native fallback → **0 wrong results, 0 crashes** (on macOS a full requalify run logs ~58 caught panics with 0 DIFF/0 ERR/0 crash). | **Cross-platform repro (2026-06-10, on `meh`):** built the extension + a cassert+debug PG18.3 on Linux x86_64 and stress-tested the EXACT macOS triggers (`SELECT FROM pg_stat_database`, `pg_stat_activity`, indexed scans, text `IN`-lists, monitor refresh) in **both debug and release** builds → **0 panics** (300+ queries each). So the double-free does **not** occur on Linux x86_64 + PG18.3. It is therefore specific to macOS-arm64 and/or the patched PG19devel used in local dev. The `do_ereport → BogusFree` backtrace is a *late symptom*: the heap is already corrupted during core planning, and `errfinish`'s cleanup is merely the first `pfree` to trip on the clobbered chunk. **DISPROVEN (2026-06-10): not the cee-scape sigsetjmp asm** — forcing pgrx's C-shim `call_closure_with_sigsetjmp` path on aarch64-darwin (instead of the external `cee-scape` `asm_based` crate) did NOT change the panic count (still 58/suite), so the architecture-specific sigsetjmp asm is not the cause. The early free happens earlier, in core planning. Ra's own logic is platform-independent Rust that runs clean on Linux. Could NOT test Linux+PG19 to fully separate arch-vs-version: the pgrx fork is tuned to macOS's PG19 and won't build against `meh`'s differently-patched PG19 (buffer-manager research that made `slock_t` volatile, removed `SpinLockFree`, and turned `SpinLockAcquire` into a function). RULED OUT as causes: `register_feedback`, monitor `maybe_refresh`, `make_scalar_array_op`, all explicit Ra `pfree`/`list_free`. Single-user mode is NOT a valid repro (corrupts on every query — a separate pgrx-single-user artifact). **Production impact: none on Linux PG18.** **Next (if needed):** force pgrx's non-asm `call_with_sigsetjmp` path on macOS and re-test (would confirm the cee_scape-asm hypothesis), or obtain a stock Linux PG19 the fork builds against. **Mitigation landed (8c588e0e):** the planner hook routes queries to the native planner while `monitor::is_refreshing()`, removing the monitor's self-inflicted subset. | P1 |

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

### Index scans (single-column btree equality) — IMPLEMENTED 2026-06-01

**Ra now emits a real `IndexScan` for `col = const` on a btree-indexed
column.** `try_build_index_scan` (a `Filter`-over-`Scan` peephole in
`plan_builder.rs`) detects a single-column btree equality conjunct, pushes
it into `indexqual` (canonical `INDEX_VAR` form, key on the left, operator
verified as the `BTEqualStrategyNumber` member of the index's
`rd_opfamily[0]`, commuted via `get_commutator` when written `const = col`),
emits `indexqualorig` (heap-Var form), and leaves any residual conjuncts as
the heap recheck `qual`. It is strictly conservative: anything unproven
(no index, non-btree, non-equality, non-`Const` other side, key not a Var of
the scanned rel, untranslatable residual) bails to the standard SeqScan
path, so a wrong/crashing index condition is never produced.

Validated on PG18.3: 13/13 row-equivalence vs Ra-off; `auto_explain`
confirms `Index Scan … Index Cond: (col = const)` with residual conjuncts as
a recheck `Filter` (equality, commuted, text, leading column of a
multi-column index); no-index tables correctly produce `Seq Scan`; 7000+
mixed-shape stress queries with 0 crashes; equality lookups run at
index speed (300 lookups in <0.1 s vs ~15 s for the prior SeqScan).

Two latent bugs were found and fixed while validating: a NULL-deref in
`index_resolver` (it read `(*index_list).length` without guarding the
empty/NIL list returned for a table with no indexes — exposed because the
peephole now calls `resolve_index` on every `Filter`-over-`Scan`), and the
monitor's `poll_hardware_metrics` querying `pg_stat_bgwriter.buffers_backend`
(removed in PG17/18 → a per-refresh panic that overflowed the error stack
under load) — now reads `pg_stat_io`.

**Extended 2026-06-01 (range + bounds + aliases + cost).** The peephole now
collects *every* leading-column comparison conjunct (btree strategies 1–5)
into the index condition, so `id >= a AND id <= b` (and `BETWEEN`, which
desugars to it) becomes a *bounded* index scan rather than a half-open one;
non-pushable conjuncts (e.g. `<>`) stay as recheck `qual`. The column
qualifier is no longer matched by name (the translated-Var `varno` check is
authoritative), so aliased queries (`WHERE b.id = …`) now use the index too.
A unique-index equality reports `plan_rows ≈ 1`. The RHS guard is now
`!contain_var_clause`, allowing any value that references no relation column.
Validated: range/BETWEEN/aliased/commuted-bounds row-equivalence; auto_explain
shows two-bound `Index Cond`s; 12000 mixed stress queries with 0 crashes.

**Remaining scope (follow-ups; all currently bail to a correct SeqScan):**
`Param`/`$1` right-hand sides are a *parser-level* gap — the Lime grammar has
no `$N` token (and ra-core no `Param` variant), so prepared statements fail
Lime parse and already fall back to PG (which index-scans them correctly);
adding native support is a grammar + ra-core + translator change, not a
plan_builder one. `IndexOnlyScan` (covering) and `BitmapScan` (OR-of-indexed)
emission are now DONE (2026-06-10, plan_builder access-path peepholes — see the
plan-builder gaps table above). Still pending: the optimizer choosing index
access by cost (the peepholes choose structurally, not by cost), `BitmapAnd`
for multi-index AND predicates, and a non-unique-equality `plan_rows` estimate
better than the generic `0.1` selectivity.

**Multi-column prefixes (added 2026-06-01).** `build_index_clause` matches a
conjunct against *any* key column of the chosen index and emits the
`INDEX_VAR` at that column's attno, checking the operator against that
column's own operator family (`rd_opfamily[attno-1]`, capped at
`indnkeyatts`), so `a = x AND b = y` on a compound index pushes both keys and
mixed-type indexes (`(int, text)`) are handled correctly. Conjuncts on
non-index columns stay as recheck `qual`. Validated row-equivalence +
auto_explain (two-key `Index Cond`s, equality + non-leading range) + 10000
compound stress queries with 0 crashes.

### Prior root cause (now resolved)
The plan builder *has* `build_index_scan` / `build_index_only_scan` /
`build_bitmap_heap_scan` / `build_merge_join`, but every index-access builder
set only `scanrelid` + `indexid` and **never set `indexqual`**, so the
executor scanned the whole index (correct, but no faster than a SeqScan);
the predicate survived only as a recheck `qual`. `try_build_index_scan`
(see above) now constructs the canonical `indexqual` for the equality case.
`RelExpr::IndexScan` still carries only `{ table, column }` (no condition
field) — the peephole reads the condition from the parent `Filter` instead
of changing the algebra, which is why it is scoped to `Filter`-over-`Scan`.

### EXPLAIN transparency
`EXPLAIN` is a utility statement, so the planner hook skips it and EXPLAIN
shows PostgreSQL's native plan, not the plan Ra actually builds for
execution. For an extension that advertises new planner behaviour this is a
transparency gap worth closing (e.g. an `EXPLAIN (RA_PROVENANCE)` option, as
sketched in planner_hook.rs).

## Coverage-gap inventory (differential search 2026-06-01)

Differential harness (Ra-on vs Ra-off, sorted-md5 set equality + decision
log) over diverse SQL on PG18. **Correctness: clean** — no wrong results and
no Ra-side errors across ~30 shapes; every gap below falls back to PG
correctly. These are shapes PG plans but Ra does **not** (it defers), in
rough priority order:

| Shape | Ra decision (reason) | Layer |
|-------|----------------------|-------|
| 3+ table join (`a JOIN b JOIN c`) | plan-build: `Join not yet supported` (join side is itself a join) | plan_builder build_join only handles a scan on each side |
| Window funcs (`lag/row_number OVER`) | plan-build: `window function` | plan_builder has no Window builder |
| `string_agg`/`array_agg` in target | plan-build: `aggregate output expression` | plan_builder build_agg_out_expr |
| Correlated `EXISTS` (→ SemiJoin) | plan-build: `join condition` | semijoin condition translation |
| Scalar subquery `b = (SELECT max..)` (→ join) | plan-build: `join side not a scan` | join side is an Aggregate |
| `FULL JOIN` | plan-build: `join type` | plan_builder join_type mapping (no FULL) |
| `= ANY(ARRAY[...])` | parse: `unexpected ARRAY` | Lime grammar (ANY+ARRAY) |
| Scalar subquery + alias in select list `(SELECT ..) AS x` | parse: `unexpected IDENT` | Lime grammar (aliased scalar subquery) |

Ra **does** plan (verified row-equivalent): single/aliased scans, equality &
range index scans, 2-table equi-joins (incl. extra non-equi ON conjunct),
`IN (subquery)` semijoin, `GROUP BY ... HAVING`, set ops (`UNION`/`EXCEPT`/
`INTERSECT`), `DISTINCT`, simple aggregates, `CASE`, `COALESCE`, `LIKE`,
`ORDER BY ... NULLS LAST`, `LIMIT/OFFSET`, single-level CTE.

Highest-value to close: **3+ table joins** (ubiquitous) and **window
functions**. NOTE: execution-time ("slower") comparison needs a
direct-execution harness — `EXPLAIN ANALYZE` cannot measure Ra's plan because
the `EXPLAIN ...` text fails Lime parse and falls back to PG.

## Coverage-gap closure (2026-06-03) — 8 of 8 closed

Worked the differential coverage-gap inventory one at a time (design / build /
verify on PG18, each committed separately):

- **#1 3+ table joins** — recursive `build_join_tree`/`build_join_node`,
  `(rtindex,attno)` Var remap, HashJoin for hashable equi-joins (NestLoop was
  O(n*m)), `Filter(Join)` post-join qual fix. Aggregate-over-join kept as a
  fallback. (main 376d14aa)
- **#2 window functions** — lag/lead/first_value/last_value/ntile/percent_rank.
  (b5d13750)
- **#3 string_agg / array_agg** — multi-arg + polymorphic `anyarray` return in
  `build_aggref`. (7adfdef0)
- **#6 FULL / RIGHT joins** — unified join builder, hashable equi required.
  (1d687b1a)
- **#4 correlated EXISTS with sub-query alias** — `flatten_rtes` now captures
  the SubLink relation's alias. (02763f99)
- **#8 implicit column alias** (`expr alias`, no `AS`) — grammar
  `target_item ::= expr IDENT`. (9d04d9e8)
- **#7 `OP ANY/ALL (array)`** — grammar + `build_sao_array` →
  `ScalarArrayOpExpr`. (ac3d75b7)

**Remaining: none.** All eight known coverage-gap shapes are now closed.

### #5 scalar subquery compared to an aggregate — CLOSED (0399eac9)

`WHERE x < (SELECT avg(y) FROM t)` and friends. Two changes routed these
through the plan builder's existing scalar-subquery path instead of a
non-renderable CrossJoin-with-Aggregate:

- **decorrelation** (`decorrelate_scalar_comparison`): no longer rewrites an
  *uncorrelated* scalar comparison subquery into `Filter(x op col,
  CrossJoin(input, Q))`. Declining leaves `Filter(x op (SELECT ...))` intact so
  `prepare_subplans` builds an `EXPR_SUBLINK` SubPlan — uncorrelated lowers to
  an `InitPlan` (evaluated once, identical plan to PG), correlated uses
  `PARAM_EXEC` via `paramify_plan`. Correlated *aggregate* decorrelation to a
  LeftJoin (TPC-H Q20) is unchanged.
- **expr_translator** (`op_expr_from_nodes`): when no operator exists for the
  exact operand types (e.g. `int4 < numeric` from comparing an int column to
  `avg()`), defer to PostgreSQL's `make_op`, which selects the best candidate
  and inserts the same implicit coercions the parser would. Exact-match cases
  are unchanged. General fix, not subquery-specific.

Self-table scalar subqueries (`b = (SELECT max(b) FROM <same table>)`) also
plan natively — the SubPlan carries its own range table, so the old
`flatten_rtes` self-join limitation does not apply.

## Deferred: Lime parser-generator update to v0.10.0

Lime v0.10.0 (current submodule pin: v0.8.7) restructured the generator into
~40 source files and a separate lex-compiler static library, so the host-tool
build recipe grew (build.rs must link the full `src/lex/*.c` set +
`emit_c_skin_bison.c` + `jit_inline.c` with `-DLIME_HAS_LEX_COMPILER
-DLIME_HAS_RUST_OUTPUT`). After that, the **actual blocker** is a spurious
`warning: --enable=safe has no effect without --target=rust` that v0.10.0
emits on every C-target build (the `safe` feature defaults ON yet is marked
rust-only). That extra stderr line defeats `build.rs`'s conflict-tolerance
check (which expects every stderr line to be a resolved-conflict line) and
aborts the build. Full reproducible root-cause analysis (with `lime.c` line
numbers) and a suggested upstream fix are written up for the Lime team in
[`docs/lime-v0.10-upgrade-blocker.md`](lime-v0.10-upgrade-blocker.md).
Holding at v0.8.7 until the upstream warning is fixed; special-casing the
warning string in `build.rs` was rejected as brittle (it would mask future
legitimate warnings).

## Fallback-closing pass (2026-06-10)

Closed the tractable translation-level fallbacks (each verified row-identical
to PG, requalify 0 DIFF / 0 ERR / 0 crash):

- **proj-coalesce** — `COALESCE` now coerces arguments to a common type
  (`select_common_type` + `coerce_to_common_type` + `assign_expr_collations`),
  so `coalesce(varchar_col, 'literal')` builds instead of failing on an exact
  type-match check.
- **scan-in** — `IN (...)` / `= ANY(ARRAY[...])` on `text`/`bpchar`/`varchar`
  columns now translate via `make_scalar_array_op` when no exact element
  operator exists (the prior `OpernameGetOprid` exact lookup found no
  `bpchar = text`).
- **window first_value/last_value** — looked up by their polymorphic
  `anyelement` signature with the result type resolved from the actual
  argument.

Suite: 60 shapes, **43 RA-BUILT / 17 FALLBACK / 0 DIFF / 0 ERR**.

### Remaining fallbacks — deferred with rationale (each a dedicated effort)

These fall back *correctly* (0 wrong results); closing them is real
feature/structural work, not a translation tweak:

| Shape(s) | Why it falls back | Scope to close |
|---|---|---|
| `agg-distinct` (`count(DISTINCT x)`) | needs `Aggref.aggdistinct` + a `SortGroupClause` (eqop/sortop) and `ressortgroupref` wiring | plan_builder, executor-coupled |
| `agg-filter` (`count(*) FILTER (WHERE …)`) | `AggregateExpr` has no `filter` field — dropped at parse | ra-core + parser + builder |
| `agg-rollup` / `agg-cube` | GROUPING SETS (`GroupingSetsClause`, `Agg` grouping sets) | RFC-scale |
| `ordered-set` (`percentile_cont … WITHIN GROUP`) | ordered-set aggregate | RFC-scale |
| `distinct-on` (`DISTINCT ON`) | Lime grammar has no `DISTINCT ON` production | grammar + builder |
| `order-limit` (`ORDER BY` non-output column) | needs a resjunk targetlist entry + top-level junk filtering | plan_builder, correctness-sensitive |
| `window-lag` (`lag`/`lead` with offset) | the grammar's `ra_window_expr` takes a single arg, dropping the offset; `WindowExpr` carries one `arg` | grammar + ra-core + builder |
| `self-join`, `union-all` (same table twice), `subq-from` (derived table) | the range-table map is keyed by bare table name + does not recurse into set-op/subquery RTEs, so the same table twice / nested base tables resolve to "table not found" | plan_builder range-table identity (key by RTE/alias; recurse into subquery RTEs) |
| `rec-cte` | recursive worktable name resolution for this shape | plan_builder |
| `scalar-subq-where`, `corr-subq` | scalar subquery in a WHERE predicate needs SubPlan/InitPlan result-param wiring; currently gated for correctness | RFC-scale |
| `cte`, `cte-multi`, `lateral` | e-graph extraction emits a node (`NestLoopOp`/`HashJoinOp`) or a projection the extractor/builder can't lower | ra-engine extraction |
