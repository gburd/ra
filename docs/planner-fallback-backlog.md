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

## Currently supported (no fallback)

- Single-relation `Scan` (SeqScan) with any nesting of `Filter` (→ qual) and
  `Project` (→ targetlist), including qualified and unqualified columns.

## Plan-builder gaps (each = one task)

Priority P0 (common, highest value), P1 (common), P2 (specialized).

| Op token | SQL it blocks | Status / why it falls back | Pri |
|---|---|---|---|
| `Join` | any multi-table join | `build_join` returns empty result sets | P0 |
| `Aggregate` | `count/sum/avg/...`, `GROUP BY`, `HAVING` | `build_aggregate` ignores the aggregate exprs → unresolved `aggregate_dummy` | P0 |
| `Sort` | `ORDER BY` (and `IncrementalSort`) | `build_sort` corrupts executor memory ("write past chunk end") | P0 |
| `Limit` | `LIMIT` / `OFFSET` | corrupts executor memory in combination with sort | P0 |
| `Distinct` | `SELECT DISTINCT` | crashes the backend | P1 |
| `Union` / `Intersect` / `Except` | set operations (+ `ALL`) | not verified | P1 |
| `Window` | window functions (`OVER (...)`) | not verified | P1 |
| `Values` | `VALUES (...)`, `INSERT ... VALUES` source | not verified | P1 |
| `CTE` / `RecursiveCTE` | `WITH` / `WITH RECURSIVE` | not verified | P1 |
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
| Correlated / general subquery expressions | "Subquery expressions not yet supported in the e-graph" | P1 |
| (general) any `RelExpr` whose `to_rec`/`from_rec` round-trip is lossy | extend e-graph encoding | — |

## Definition of done

Zero fallbacks means: across a representative workload (and the JOB / TPC-H
suites), `ra_planner.log_decisions = on` reports no `fell back` lines, and the
replan-equivalence test passes for every shape exercised.
