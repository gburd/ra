# Ra vs PostgreSQL 19-beta1 — comprehensive requalification (2026-06-07, eve)

Re-examined Ra against PG19 with a 54-query suite spanning scans, projections,
all join types, aggregates (incl. FILTER / ROLLUP / CUBE / ordered-set),
DISTINCT / DISTINCT ON, window functions, set ops, subqueries (IN / EXISTS /
NOT IN / NOT EXISTS / scalar / correlated), CTEs (incl. RECURSIVE), LATERAL,
derived tables, and TPC-H Q1/Q3. Harness + suite committed at
`scripts/ra_requalify.sh` + `scripts/ra_suite.txt`.

## Methodology

Per query, on the live PG19 cluster (port 5433, db `tpch`):
- **Correctness**: sorted shell-side hash of `ra_planner.enabled=off` vs `=on`,
  using the bare query as the only statement, `</dev/null` on every psql (the
  while-loop heredoc would otherwise be consumed by psql and mangle queries).
- **Fallback**: with `ra_planner.log_decisions=on`, scan the per-query log
  window for "fell back", **excluding** the extension's background monitor
  queries (`pg_stat_database` etc.) — those pollute the window and inflated an
  earlier count from 20 to a false 28.

## Result

| | Count |
|---|---|
| RA-BUILT (Ra planned it, row-identical to PG) | **34 / 54** |
| FALLBACK (deferred to PG, row-identical) | 20 / 54 |
| DIFF (wrong result) | **0** |
| ERR (error) | **0** |

**Correctness is solid: zero wrong results, zero errors.** The gap to the goal
("never fall back, outperform PG everywhere") is the 20 fallbacks below — each a
coverage task, not a correctness bug.

## RA-BUILT (34)

scan point/range/str-eq/between/or/is-null/like, projection expr/case/cast,
inner join (+str filter), LEFT/RIGHT/FULL/CROSS join, 3-way join, scalar
aggregate, GROUP BY, HAVING, DISTINCT, OFFSET, window rank/partition,
INTERSECT, EXCEPT, IN-subquery, EXISTS, NOT EXISTS, NOT IN, scalar subquery in
SELECT, TPC-H Q1, TPC-H Q3.

## FALLBACK (20) — by cause

| Cause (plan_builder/optimizer) | Queries | What's needed |
|---|---|---|
| **`IndexScan` not buildable** | union, union-all, cte, cte-multi, scan-in, proj-coalesce | The e-graph *extracts* a physical `IndexScan`/`IndexOnlyScan`/`BitmapHeapScan` (cost-preferred) that plan_builder cannot emit, so Ra defers. **Highest-value lever** — real index-scan support in plan_builder closes these *and* wins on execution. (Lowering them to SeqScan would close the fallback but regress execution vs PG's index plan, so it is not a win.) |
| **scalar subquery in WHERE** | scalar-subq-where, corr-subq | `SubPlan`/`InitPlan` + `PARAM_EXEC` wiring (executor-coupled, RFC-scale). |
| **`ORDER BY` col not in output** | order-limit | resjunk targetlist entries for sort-only columns. |
| **aggregate output expression** | agg-distinct, agg-filter, agg-rollup, agg-cube, ordered-set | DISTINCT/FILTER aggregates, GROUPING SETS/ROLLUP/CUBE, ordered-set aggs. |
| **join WHERE predicate (self-join)** | self-join | self-join range predicate remap (`a.k < b.k`). |
| **e-graph extraction (NestLoopOp)** | lateral | LATERAL → parameterized nestloop build. |
| **DISTINCT ON** | distinct-on | `DISTINCT ON` lowering. |
| **window frame / lag** | window-lag | explicit window frames + lag/lead/ntile. |
| **RECURSIVE CTE** | rec-cte | `RecursiveUnion` + worktable. |
| **derived table** | subq-from | aggregate-derived-table passthrough. |

## The UNION "blowup", re-characterized

The earlier "UNION ~50× planning blowup" is **two compounding issues**:
1. The e-graph saturates slowly on filtered set-ops (~43 ms; per-interleaved-
   iteration egg `Runner` setup + ~290-rule scheduler over a tree with multiple
   filtered branches and their index/bitmap alternatives).
2. It then **falls back anyway** — the cost model extracts an `IndexScan` the
   plan_builder can't emit — so the 43 ms is wasted before PG re-plans.

A set-op fast-path that bypasses the e-graph (optimize each branch, rebuild) was
**attempted and reverted**: it produced an un-normalized set-op tree that the
plan_builder mis-builds, causing an intermittent backend **segfault** on
UNION ALL / INTERSECT / EXCEPT. The e-graph performs a set-op normalization the
plan builder depends on; a safe fast-path must reproduce it (or plan_builder
must be hardened). Tracked.

## Roadmap to "never fall back + outperform PG"

In value order:
1. **Real `IndexScan`/`BitmapHeapScan` build in plan_builder** — closes the
   largest fallback class and wins execution (index vs seq). Prereq: the prior
   bitmap-advice crash must stay fixed; verify with `replan-equivalence-test.sh`.
2. **Set-op normalization in plan_builder** so a cheap branch-wise fast-path is
   safe (also kills the 43 ms saturation cost).
3. **Scalar/correlated subquery via SubPlan** (RFC-scale, executor-coupled).
4. Advanced aggregates, DISTINCT ON, window frames, RECURSIVE CTE, LATERAL,
   resjunk sort keys, aggregate-derived-table passthrough.

Each must pass the replan-equivalence property test before its fallback gate is
removed (correctness > coverage remains the invariant).
