# RFC 0090: Provenance via `EXPLAIN (RA_PROVENANCE)`

- Start Date: 2026-05-29
- Author: gregburd
- Status: Proposed
- Tracking Issue: TBD

## Summary

Expose Ra's `PlanProvenance` (cost-model snapshot id, hardware
hash, active rule-set hash, optimization route, termination
reason) through a PostgreSQL `EXPLAIN (RA_PROVENANCE)` option,
mirroring the CLI's `ra-cli explain --provenance` output. This
makes a Ra plan reproducible from its EXPLAIN alone.

## Motivation

`PlanProvenance` is already captured on every
`OptimizationResult` and rendered by the CLI. Inside PostgreSQL,
though, there's no way to see it: an operator debugging why two
identical queries produced different plans can't tell whether the
cost model snapshot, hardware profile, or rule set changed
between them. Surfacing provenance in EXPLAIN closes that gap and
makes production plan differences diagnosable without re-running
through the CLI.

This RFC exists because the work was previously an inline
`TODO(provenance)` in `planner_hook.rs`. Inline TODOs rot; a
tracked RFC states the design and the explicit reason it's
deferred (no planning-behavior change; PG-version-sensitive FFI).

## Guide-level explanation

```sql
EXPLAIN (RA_PROVENANCE) SELECT * FROM orders o JOIN customers c
  ON o.cust_id = c.id;
--  Hash Join
--    ...
--  Ra Provenance:
--    cost_model: bitnet@a1b2c3d4
--    hardware:   m3max/12c/8MB-L3/128bit-simd (hash 7f3e...)
--    rule_set:   307-rules (hash 9c2a...)
--    route:      EGRAPH_MED (8 iters, 15ms budget)
--    terminated: converged (cost delta < 0.1%)
```

The block appears only when `RA_PROVENANCE` is requested. It
changes no plan; it's pure diagnostic output.

## Reference-level explanation

The implementation mirrors the existing
`crates/ra-pg-extension/src/plan_advice_explain.rs` machinery,
which already solves every hard part:

1. **Capture.** The planner hook already produces an
   `OptimizationResult` with `provenance: Option<PlanProvenance>`.
   Stash the rendered provenance string keyed on the produced
   `PlannedStmt` pointer (the same pointer-keyed
   `OnceLock<Mutex<HashMap<usize, String>>>` pattern
   `plan_advice_explain` uses, because pgrx-pg-sys 0.17's
   `PlannedStmt` binding lacks `extension_state`).

2. **Register the option.** `RegisterExtensionExplainOption(
   "ra_provenance", handler)` plus a per-`ExplainState` boolean
   slot via `Set/GetExplainExtensionState`, identical to the
   `plan_advice` option handler.

3. **Render.** Extend the existing `explain_per_plan_hook`
   (don't add a second hook) to also pop and emit the provenance
   block when the `ra_provenance` flag is set. Reuse
   `ExplainPropertyText`.

4. **Share the renderer.** Factor the CLI's provenance-rendering
   (`crates/ra-cli/src/commands/explain.rs`) into a function on
   `PlanProvenance` (e.g. `render_text(&self) -> String`) so the
   CLI and the PG extension produce byte-identical output.

### Why reuse `plan_advice_explain` rather than a new module

The option-registration, per-`ExplainState` slot, hook chaining,
and pointer-keyed stash are already written, tested against the
pgrx 0.17 pg18 bindings, and handle the binding quirks. A second
copy would duplicate ~150 lines of FFI. The per-plan hook should
gain a second optional block, not a sibling hook (PG calls
`explain_per_plan_hook` once; chaining two of our own would be
wasteful and ordering-fragile).

## Drawbacks

- Untestable in CI without a live PG (`cargo pgrx test`); the
  shared renderer can be unit-tested but the EXPLAIN wiring
  can't.
- PG-version-sensitive: `RegisterExtensionExplainOption`'s
  signature has shifted across PG releases (pgrx 0.17 binds the
  2-arg pg18 form). A pgrx bump may require signature updates.
- Adds a second per-`ExplainState` extension slot; bounded
  per-backend memory, freed with the `ExplainState`.

## Rationale and alternatives

- **JSON EXPLAIN field instead of a text block.** PG's
  structured EXPLAIN would carry provenance as nested JSON. Worth
  doing alongside the text block; the `ExplainPropertyText` path
  handles text, and `ExplainProperty*` variants handle JSON.
  Deferred to keep the first cut small.
- **A separate `ra_provenance()` SQL function** returning the
  last plan's provenance. Simpler FFI but worse ergonomics (not
  attached to the EXPLAIN the operator is already reading).
- **Do nothing.** Provenance stays CLI-only. Acceptable today;
  the gap only bites in-database debugging.

## Prior art

- `pg_plan_advice`'s `EXPLAIN (PLAN_ADVICE)` option — the exact
  pattern this RFC reuses, already ported in
  `plan_advice_explain.rs`.
- `auto_explain`'s settings-dump behavior — precedent for
  emitting planner metadata into EXPLAIN.

## Unresolved questions

- Should provenance also appear in `auto_explain` log output?
- Whether to gate behind a GUC (`ra_planner.always_explain_
  provenance`) like supplied advice has.
