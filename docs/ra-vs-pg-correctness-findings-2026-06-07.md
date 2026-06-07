# Ra vs PostgreSQL 19-beta1 — exhaustive A/B findings (2026-06-07)

Follow-up to `ra-vs-pg-correctness-findings-2026-06-06.md`. Re-ran the Pg-vs-Ra
comparison on the PG19 cluster (`/usr/local/pgsql`, port 5433, db `tpch`,
`shared_preload_libraries=pg_ra_planner`) with the **only sound methodology**.

## Methodology (why the old "27/27 correct" was partly false)

- Toggle Ra at **connect time**: `PGOPTIONS='-c ra_planner.enabled=on|off'`, run
  the **bare query as the only statement**. `SET ra_planner.enabled=...; <query>`
  in one `psql -c` corrupts Ra's re-parse (the hook sees a query_string starting
  with `SET`).
- **Hash the result on the shell side**: `psql -tAq -c "<bare query>" | grep -v
  '^Time:' | sort | shasum`. Do **not** wrap the query in `count(*)` or
  `md5(string_agg(...))` — that makes Ra fall back, so the harness compares
  PG-to-PG and masks Ra bugs. (`ra-bench compare --verify` wraps in
  `md5(string_agg)` and so its "27/27" is false-correct for join/subquery shapes.)
- Strip psql's `Time: N ms` line (`\timing`) before hashing — otherwise every
  comparison is a false DIFF on the timing jitter.

## Correctness: 5 wrong-result families confirmed (prime-invariant violations)

All returned **different results than PG with no error and no fallback**:

| Shape | Symptom |
|-------|---------|
| LEFT JOIN + WHERE on outer table | outer predicate remapped to inner rel |
| CROSS JOIN + outer WHERE | qual dropped |
| scalar subquery in WHERE | mis-evaluated |
| IN-subquery `AND` base predicate | quals dropped |
| **bare IN-subquery** (regression) | returned **0 rows vs 3921** — semi-join build broken |

The bare-IN case was reported *correct* on 2026-06-06, so a semi-join
build regression landed since then.

## Fix shipped: correctness gate (correctness > coverage)

`PlanBuilder::wrong_result_risk` (`plan_builder.rs`), consulted in
`build_planned_stmt`, defers to the native planner when the optimized tree
contains:
- any **non-inner join** — LEFT/RIGHT/FULL/CROSS outer + SEMI/ANTI from
  decorrelated IN/EXISTS/NOT IN/NOT EXISTS, or
- a **scalar subquery in a filter predicate**.

A naive `Filter{Join}` shape-gate did **not** work: subquery decorrelation
(IN/EXISTS → semi/anti join, before the e-graph) and predicate pushdown
restructure the tree, so the gate must key on **join type anywhere in the
optimized tree**, not on a surface Filter-over-Join shape.

### Validation — 30 shapes, all row-identical Ra-on vs Ra-off

- Fixed via fallback: left-join+where, cross-join+where, scalar-subq,
  in-subq+pred, bare-in, exists, not-in, not-exists.
- Still planned by Ra (no over-gating): simple filter, inner join, self-join,
  3/6-way inner joins, group-by/having, grouping sets, rollup, window, distinct,
  distinct-on, order+limit, offset, union/union-all/intersect/except, case, cast,
  coalesce, between, in-list, lateral.

Commit: `fix(pg): correctness gate — defer non-inner joins + scalar subqueries
to PG` (main `a82e638c`).

## Performance: 1 Ra-slower case found

Median total time (plan+exec, 5 runs, Ra-on vs Ra-off):

| Shape | PG (off) | Ra (on) | Note |
|-------|----------|---------|------|
| simple filter, inner/3/6-join, agg, window, distinct, order+limit | ~3–13 ms | ~equal | within noise |
| **UNION (2-branch scan)** | 4.2 ms | **215 ms** | **50× slower** |

`auto_explain` shows the produced plans are **identical** (Unique→Sort→Append→2
SeqScans, ~1.9 ms actual exec for both). The 215 ms is **Ra planning time** —
the e-graph saturates slowly on the set-op shape (~213 ms of planning). The
speculative router does not fast-path UNION/INTERSECT/EXCEPT.

## Open follow-ups (tracked; not yet fixed)

1. **Re-enable outer-join coverage** by fixing the predicate-pushdown bug
   properly (see Update below) — LEFT/RIGHT/FULL/CROSS are still gated.
2. **UNION planning blowup**: route set-ops through a fast path or cap the
   e-graph budget for set-op-dominated trees.
3. **Scalar subquery in WHERE**: still gated (SubPlan result param wiring);
   re-enable with a proper fix.

---

## Update — same day, second pass (root-caused one family, narrowed the gate)

Deeper diagnosis with `EXPLAIN (VERBOSE)` + `ra-cli optimize` turned the
broad "all non-inner joins are wrong" finding into two distinct causes:

### Real bug fixed: missing collation on coerced comparisons

The "bare IN-subquery returns 0 rows" was **not** a semi-join bug — it was a
**collation** bug. `WHERE textcol = 'literal'` on a `character`/`bpchar` column
becomes `(col)::text = 'literal'::text` (implicit bpchar→text coercion). When no
exact-type operator exists, `expr_translator` falls back to PG's `make_op`, which
inserts the coercion but does **not** assign collations — PG's parser does that
in a separate `assign_expr_collations` pass that Ra skipped. The OpExpr had
`inputcollid = InvalidOid` → executor error *"could not determine which collation
to use for string comparison"* (counted as 0 rows under `2>/dev/null`). The
decorrelated IN-subquery's inner string filter hit the same path.

Fix (`expr_translator.rs`): run `assign_expr_collations(NULL, op)` on the
`make_op` result. This fixed **all string-comparison scan filters** (`=`,`<`,`>`,
`<>` on text/bpchar) **and** the entire **IN / EXISTS / NOT IN / NOT EXISTS**
family (semi/anti joins) — now row-identical to PG and **planned by Ra, not
deferred**. Commit `fix(pg): assign collations on coerced comparisons` (main
`24131eda`).

### Genuine remaining bug: outer-join WHERE pushdown to the wrong child

`SELECT o.o_orderkey FROM orders o LEFT JOIN customer c ON o.o_custkey=c.c_custkey
WHERE o.o_orderkey<50` → 20000 rows vs PG's 49. `ra-cli optimize` shows a
predicate-pushdown rewrite places the **outer**-relation predicate
(`o.o_orderkey<50`) on the **inner** (customer) child:
`LEFT JOIN(Scan orders, Filter(o.o_orderkey<50, Scan customer))`. The executed
plan confirms `Filter: (o.o_orderkey<50)` on the customer seq scan, so it never
filters and the join keeps all rows. The sound inner-join rules
(`filter-through-join-left/right`) guard with `references_only`; the
`*-filter-through-left-join-*` rewrites do not, but none pushes to the *right*
child — the exact offending rewrite (likely a `left-outer-to-inner-*`
interaction) needs e-graph firing traces to pin down. Gated for now.

Remaining gated (correct via fallback, coverage follow-up): outer joins, scalar
subquery in WHERE. Remaining perf: UNION planning blowup.

---

## Update — third pass: root-caused the outer-join bug to a systemic condition bug

E-graph rule tracing (`optimize_with_tracking_verbose`, `applied` rules) on the
LEFT-join case named the offenders: **`left-outer-to-inner-{lt,gt,…}`** (convert
LEFT→INNER) and **`datafusion-filter-pushdown-through-join-{left,right}`** (push
the filter into a child). Both are guarded by `references_only` /
`predicate_references_only` preconditions that *should* forbid placing an
outer-relation predicate on the inner side — but the guard is a **no-op for
scalar predicates**:

`ReferencesOnly::check` (`conditions.rs`) tests
`pred_data.tables.is_subset(&side_data.tables)`. The analysis's `tables` set
(`analysis.rs`) is populated only for **relational** nodes (Scan/Filter/Join);
a **scalar** predicate e-class such as `(lt (qcol o o_orderkey) (const 50))`
has an **empty** `tables` set. `∅.is_subset(anything) == true`, so the guard
always passes → every `references_only`-gated filter-pushdown / outer-join
conversion is effectively **unguarded**. Bad (semantically-invalid) equivalents
are inserted into the e-graph; they stay hidden whenever cost extracts a correct
equivalent, and surface as wrong results when the cost model (e.g. with the
extension's live-fingerprint + page-size tuning) extracts the bad one. That is
why `Optimizer::new().optimize()` returns the correct plan but the extension
(and ra-cli with cost tuning) returns the buggy `LEFT JOIN(orders, Filter(o.col,
customer))`.

This is **systemic**: the same weakness underlies the inner-join pushdown rules
too (their bad forms just aren't usually extracted).

### What landed (ra-engine, main `27c6e47f`)

`ReferencesOnly` is now sound. A new `RelData.qualifiers` field tracks relation
table-names + aliases (from `Scan`/`ScanAlias`) and the qualifier on each `QCol`
leaf (for scalar predicates), propagated up like `columns`. Because `QCol` stores
the query alias and `ScanAlias` carries both table and alias, they share one
namespace — no alias↔table resolution needed. `references_only` now tests
`pred.qualifiers ⊆ side.qualifiers` (a real check), and `tables` (+ its consumers
`is_uncorrelated`/`single_reference`/cardinality) is untouched. The
`left/right-outer-to-inner-*` rules also got explicit
`references_only("?col", <nullable-side>)` guards. 2025 ra-engine lib tests pass;
clippy clean. Verified: LEFT JOIN + outer-side WHERE now keeps the filter above
the join under the extension's cost config (the original wrong-result case).

### Still gated: a *distinct* second bug (filter pushdown to the nullable side)

Un-gating outer joins after the fix surfaced a different unsoundness: generic
`(filter ?pred (join ?type ?cond ?left ?right))` pushdown rules
(`datafusion-/materialize-filter-pushdown-through-join-{left,right}`,
`calcite-filter-into-join`, `logical/predicate-pushdown/filter-join-push`) push a
predicate to a child for **any** `?type`. With `references_only` now working they
correctly push *side-matching* predicates — but pushing a predicate to the
**nullable** side of an outer join is unsound regardless (e.g.
`A LEFT JOIN B WHERE p(B)` ≠ `A LEFT JOIN (B WHERE p(B))`). Confirmed:
`… LEFT JOIN customer c … WHERE c.c_acctbal>9000` returned 20000 vs PG's 0, with
`Filter (… < c.c_acctbal)` pushed onto the customer scan under a `Hash Left Join`.
Outer joins remain **gated** (correct via fallback) pending this fix.

### Proper fix (next, self-contained)

Restrict the generic `(join ?type …)` *side*-pushdown rules so they only push to
a side the join type permits: inner/cross → either side; LEFT outer → left
(preserved) only; RIGHT outer → right only; FULL → neither. Simplest sound
encoding: change those rules' `?type` to `inner` (the dedicated
`duckdb-/xml-filter-through-left-join` rules already cover the valid
left-outer→left push), or add an `is_inner_join("?type")` precondition +
register it in `build.rs` KNOWN_CONDITIONS. Then un-gate outer joins and re-run
the A/B (the 3 inner-side-predicate cases: `left+inner-where`,
`right+outer-where`, `left+str-where`). Needs unit tests asserting the bad
pushed form never enters the e-graph for outer joins.

---

### Gate narrowed + validation expanded (second pass)

`wrong_result_risk` now gates only **outer joins (LEFT/RIGHT/FULL/CROSS)** and
**scalar subqueries in filter predicates**; SEMI/ANTI are no longer gated.
**63 distinct query shapes** verified row-identical Ra-on vs Ra-off, including:
recursive/multi/nested CTEs, =ANY/=ALL, correlated scalar subquery in SELECT,
FILTER-clause + ordered-set (`percentile_cont`) aggregates, CUBE/ROLLUP/GROUPING
SETS, window partition/frame/lag-lead, INTERSECT/EXCEPT ALL, NULLS FIRST/LAST,
TPC-H Q1/Q3-shaped multi-join+agg+order+limit, and the string-filter + IN/EXISTS
families now planned by Ra.

Remaining gated (correct via fallback, coverage follow-up): outer joins, scalar
subquery in WHERE. Remaining perf: UNION planning blowup.
