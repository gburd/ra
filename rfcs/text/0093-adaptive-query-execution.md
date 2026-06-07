# RFC 0093: Adaptive Query Execution (re-plan as conditions change)

- Start Date: 2026-06-07
- Author: Ra Team
- Status: Proposed (banked — not yet scheduled)
- Tracking Issue: n/a
- Related: RFC 0091 (Cost Models as Rules; live conditions B0–B2), RFC 0023
  (Adaptive Query Execution), RFC 0076 (Adaptive mid-query re-optimization)

## Summary

RFC 0091 made Ra's cost model **live-conditions-aware at plan time**: the system
fingerprint (`hit_rate` / `io_saturation` / `cpu_load`) sampled when a query is
planned steers plan choice (e.g. seq-scan vs index-scan). This RFC banks the
natural next step: react to conditions that **change while a plan is already
executing** — a long-running plan chosen for a warm cache may become wrong when
the cache turns cold mid-flight, or vice versa.

## Motivation

A plan is chosen once, from a snapshot of host state. For short queries that is
fine. For long-running plans, the host state that justified the plan can shift
during execution:

- a concurrent workload evicts the cache → the index-nested-loop the planner
  chose (cheap random I/O assumed) now thrashes;
- I/O saturation spikes → a plan that streams large sequential scans contends;
- CPU load drops → a parallel plan could now use more workers.

Plan-time live conditions (RFC 0091) cannot address this: the decision is frozen
at planning. Adaptive execution closes the loop by re-evaluating during the run.

## Sketch of approaches (to be detailed if scheduled)

1. **Re-plan at pipeline boundaries.** Between blocking operators (after a sort,
   hash build, or materialize) re-sample the fingerprint and, if it has moved
   past a threshold, re-optimize the *remaining* sub-plan. Bounded blast radius;
   no mid-operator surgery.
2. **Operator-level adaptivity.** Executor hooks that switch strategy in place
   (e.g. nested-loop → hash when the inner stops fitting cache), as PostgreSQL's
   own AQE-style nodes and adaptive joins do.
3. **Parameterized "robust" plans.** Choose plans whose cost is flat across the
   plausible condition range (avoid plans that are great warm but catastrophic
   cold), trading peak speed for resilience — cheaper than re-planning.

## Why this is non-trivial (and why it's banked, not built)

- **Executor coupling.** Ra is a `planner_hook` drop-in; it produces a PG
  `PlannedStmt` and hands off to PostgreSQL's executor. Re-planning mid-execution
  needs executor-level hooks (custom scan/executor nodes) that Ra does not have,
  and must never violate the prime invariant (correct results, always).
- **Cost of re-planning.** Re-optimization is not free; it must pay for itself
  versus finishing the current plan. Needs a gating model.
- **Measurement.** Like RFC 0091, any adaptive change must be validated by
  plan-/run-quality A/B, not asserted — and adaptivity is far harder to measure
  (non-deterministic conditions).

## Decision

Banked as a future RFC. The plan-time foundation (RFC 0091 B0–B2: the cost model
reads live conditions and steers scan/join method) is the prerequisite and is
done; this RFC builds on it when scheduled. Approach (1) (re-plan at pipeline
boundaries) is the most tractable first slice and the recommended entry point.
