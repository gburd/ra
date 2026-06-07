# Live-conditions plan-impact measurement (2026-06-07)

Quantifies whether threading the live system fingerprint
(`shared_buffers_hit_rate` / `io_saturation` / `cpu_load_fraction`) into Ra's
e-graph cost model (`IntegratedCostFn`, via `LiveConditions` →
`CalibratedCostModel::with_live_conditions`) actually changes the plans Ra
**picks** — the honesty-mandate question left open when the feature was wired
(its effect was asserted by construction, never measured).

## Method

A new diagnostic GUC set in `pg_ra_planner` overrides the fingerprint fed to the
optimizer so conditions can be forced to known values:

- `ra_planner.debug_hit_rate`
- `ra_planner.debug_io_saturation`
- `ra_planner.debug_cpu_load`

`-1.0` (default) uses the real monitored value; `0.0..1.0` forces that
component. `scripts/live_conditions_sweep.py` runs `EXPLAIN (ANALYZE, FORMAT
JSON)` for each query with `ra_planner.enabled=on` under three forced
fingerprints and compares the plan node-type structure, Ra's estimated cost, and
execution time. Run on PG19devel, synthetic TPC-H (~SF 0.02).

| condition  | hit_rate | io_saturation | cpu_load |
|------------|----------|---------------|----------|
| neutral    | 0.0      | 0.0           | 0.0      |
| cached     | 0.99     | 0.0           | 0.0      |
| contended  | 0.0      | 0.9           | 0.9      |

## Result

**Plan choice changed on 0 of 6 queries.** The extracted plan shape is identical
across neutral / cached / contended for every query tested (2-, 3-, 4-table
joins; scan+filter; group-by aggregates; part/supplier join). Execution-time
differences between conditions are run-to-run noise on the *same* plan.

## Why (the honest finding)

Live conditions scale broad cost **categories** uniformly: all I/O page rates by
`(1 - hit_rate)(1 + io_saturation)`, all CPU/tuple rates by `(1 + cpu_load)`.
The e-graph's plan-choice freedom is dominated by **join method** selection
(hash / merge / nest-loop), and all of those are tuple-cost-based — so a CPU
multiplier scales them *together* and never reorders them. Scan cost (page/I/O
based) does move with `hit_rate`, but a scan is rarely a competing *alternative*
in the e-graph (you scan the table regardless; the open choice is the join
method), so changing its magnitude does not flip a decision.

So the live-conditions wiring is, at present, **inert for e-graph plan choice**.
It still affects total cost *magnitude*, which feeds the neural blend
(`compute_blend_alpha`) and the speculative router, and `plan_builder` separately
modulates the EXPLAIN costs it annotates — but it does not, today, steer the
traditional plan-decision cost toward a different plan.

## To make it deliver a desired outcome

For live conditions to change plan *choice*, the e-graph needs alternatives
whose **relative** cost depends on the I/O-vs-CPU balance. The highest-leverage
candidate is promoting the **scan-method** decision (sequential vs index vs
bitmap) into the e-graph cost — it is currently a `plan_builder` peephole, so the
optimizer never weighs a cache-cheap index scan against a seq scan. With that in
the e-graph, a high `hit_rate` (cheap random I/O) could correctly flip seq-scan →
index-scan. That is a scoped follow-up (RFC-worthy) rather than a tweak.
