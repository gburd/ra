# Executor Divergences Review

Ra's `plan_builder` hand-constructs PostgreSQL `Plan` trees and feeds them
directly to the executor, **bypassing the planner's `set_plan_references`
(setrefs) pass**. This is the central place where Ra does something the
PostgreSQL planner would normally do for us, so each divergence is reviewed
here and verified by differential testing (Ra-on vs Ra-off, row-equivalent).

## Reviewed divergences

| Divergence | Why it is needed | Soundness |
|---|---|---|
| Upper projecting nodes (Agg/Join/Window output, join/agg quals) emit `Var` with `varno = OUTER_VAR`/`INNER_VAR` and `varattno` = column position in the child targetlist | setrefs normally rewrites scan-relative Vars into OUTER/INNER references; Ra emits them directly | Differentially verified across join / agg / window / agg-expression shapes (row-equivalent). Passthrough nodes (Sort/Limit/Unique/Append) share the child targetlist verbatim. |
| Scalar sub-queries built as inline `SubPlan` (EXPR_SUBLINK) with `parParam`/`args`, correlation outer-Vars replaced by `PARAM_EXEC` `Param`s | no SS_process_ctes / SS_finalize_plan run | Correlated SubPlans are evaluated per outer row by `ExecSubPlan` (they live in an expression), so per-row re-evaluation is correct **without** the `Plan.extParam`/`allParam` bitmaps setrefs would compute. Verified for correlated/uncorrelated scalar sub-queries in projection and WHERE, including under Sort. |
| Recursive CTE built as `CteScan` over `RecursiveUnion{Result, WorkTableScan}`, reusing PG's existing `RTE_CTE` and threading cte/worktable `PARAM_EXEC` params | no SS_process_ctes | Verified row-equivalent for counters, graph traversal, multi-column, and base-relation joins. |

## Rejected divergence

- **`resjunk` ORDER BY columns** (exposing an ORDER-BY key not in the SELECT
  list as a `resjunk` scan output): attempted and **reverted** — it
  segfaulted the executor (signal 11) despite PG18 `InitPlan` setting up the
  SELECT junk filter. ORDER BY over a non-output column stays a safe
  fallback to the native planner.

## Containment

Any unsound divergence is bounded by three layers:
1. The correctness gate (`first_unsupported_op`) — only verified shapes build.
2. Differential testing — Ra-on results compared to native PG.
3. The planner hook's `catch_unwind` + fall back to `call_prev_planner`.

The standing rule: **prefer a safe fallback over emitting a plan structure
the executor would not produce.** A divergence is admitted only after it is
differentially verified row-equivalent on the live PG18 harness.
