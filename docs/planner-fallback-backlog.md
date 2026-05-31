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
| Correlated / general subquery expressions | "Subquery expressions not yet supported in the e-graph" — also needs the same range-table reconstruction as CTE | P2 |
| (general) any `RelExpr` whose `to_rec`/`from_rec` round-trip is lossy | extend e-graph encoding | — |

## Known bugs causing fallback (not operator gaps)

| Bug | Trigger | Status | Pri |
|---|---|---|---|
| Index-stats double-free | planning a query over an indexed table (intermittent) — `pfree called with invalid pointer (header 0x7f…)` | A `pfree` of an already-freed pointer in `stats_bridge` index-stats gathering. PG's `pfree` guard catches it (no corruption) and the planner-hook `catch_unwind` falls back, so results stay correct; but it discards Ra's plan. Wrapped `gather_index_stats` in `catch_unwind` reduced but did not eliminate it — not yet root-caused (became too rare to capture a backtrace). Use `backtrace_functions='BogusFree'` on an assert build to catch the caller. | P1 |

## Definition of done

Zero fallbacks means: across a representative workload (and the JOB / TPC-H
suites), `ra_planner.log_decisions = on` reports no `fell back` lines, and the
replan-equivalence test passes for every shape exercised.
