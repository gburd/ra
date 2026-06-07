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

1. **Re-enable coverage** by fixing the gated plan_builder bugs properly:
   - non-inner-join predicate placement (`build_join_node` outer-WHERE remap);
   - semi-join build returning 0 rows (the bare-IN regression — bisect from
     2026-06-06).
2. **UNION planning blowup**: route set-ops through a fast path or cap the
   e-graph budget for set-op-dominated trees.
3. Continue the shape sweep (recursive CTE, =ANY/=ALL, NULLS FIRST/LAST,
   correlated scalar subqueries in SELECT, FILTER-clause aggregates, ordered-set
   aggregates) — not yet exhausted.
