# RFC 0091 — Cost models as rules: wiring `.rra` cost functions into `IntegratedCostFn`

- Status: Proposed
- Created: 2026-06-06
- Related: RFC 0090 (Ra as a rule-driven engine)

## Summary

Move the per-operator / per-access-method cost functions that are currently
hand-coded in `crates/ra-engine/src/cost.rs` into the **physical-operator
`.rra` rule files**, compile them at build time into a cost-function registry,
and have `IntegratedCostFn` dispatch to them. The rule-provided operator cost
becomes the **traditional** term that is then combined — exactly as today —
with the staleness factor, the `ra_hardware` `CalibratedCostModel`, and the
`BitNet` neural multiplier to produce the final blended cost.

## The key question: should *every* rule carry a cost function?

**No — and forcing it on every rule would be a category error.** This is the
"explain why that's a bad idea" answer.

Cost is a property of **physical operators in the chosen plan**, not of
**logical rewrites**. In a Cascades/e-graph optimizer, a rewrite rule
(predicate pushdown, join reordering, projection pruning, constant folding,
subquery decorrelation, …) transforms one logical expression into an
*equivalent* one. It has **no intrinsic cost**: the cost is evaluated on the
operators of the extracted plan during extraction, and the e-graph picks the
cheapest equivalent form. Asking "what is the cost of `predicate-pushdown`?"
is meaningless; what has a cost is the `Filter` and the `HashJoin` it produces.

Today **1,062 `.rra` files already contain a `## Cost Model` Rust block**, but:
- they are **illustrative** with **non-uniform signatures**
  (`estimated_benefit(rows, groups, overhead) -> f64`,
  `calibration_guidance(hw) -> f64`, …) — documentation, not a compilable
  interface; and
- on logical-rewrite rules they model the rewrite's **benefit/heuristic**, which
  is a *router/rule-ordering* signal, **not** an operator cost and must not be
  summed into per-node plan cost.

So the design is: **only the physical-operator rules get the uniform,
wired-in cost interface.** The logical-rewrite `## Cost Model` sections stay as
documentation (and may later feed the rule *advisor*/router, a separate
concern). This matches where cost actually lives.

### Which rules get a wired-in cost function

The `RelLang` physical operators and the `cost.rs` methods that currently model
them — these are the in-scope set (~14 functions):

| RelLang operator | current `cost.rs` method |
|---|---|
| `Scan` (seq) | `scan_cost` |
| `index-scan` | `index_scan_cost` |
| `index-only`/covering | `index_only_scan_cost`, `covering_index_scan_cost` |
| `bitmap-index-scan` / heap / combine | `bitmap_index_scan_cost`, `bitmap_heap_scan_cost`, `bitmap_combine_cost` |
| `fts-index-scan` | (new) |
| `Filter` | `filter_cost` |
| `hash-join` / `merge-join` / `nest-loop` / `index-nest-loop` | `join_cost`, `join_cost_with_runtime_filter` |
| `Sort` / incremental sort | `sort_cost`, `incremental_sort_cost` |
| `Aggregate` (hash/stream) | `aggregate_cost` |

Each gets a `## Cost Model` section in its `rules/physical/...` file whose body
*is* the (moved) reference implementation. Operators without a dedicated rule
file get one created under `rules/physical/<area>/`.

## The cost-function interface

A single, stable signature all physical-operator cost models implement:

```rust
/// Inputs available when costing one physical operator instance.
pub struct OperatorCostCtx<'a> {
    /// Costed child sub-plan costs (egg gives child costs, not cardinalities).
    pub child_costs: &'a [f64],
    /// Base-relation statistics for the operator's input(s).
    pub stats: &'a ra_core::statistics::Statistics,
    /// Estimated input/output cardinalities and selectivity.
    pub rows_in: f64,
    pub rows_out: f64,
    pub selectivity: f64,
    /// Hardware rates (already calibrated): seq/rand page, per-tuple CPU, SIMD/par factors.
    pub hw: &'a ra_hardware::calibration::CalibratedCostModel,
    /// Statistics staleness for the operator's relation(s).
    pub staleness: ra_stats::accuracy::Staleness,
    /// Live system fingerprint (cache hit rate, io saturation, cpu load).
    pub live: crate::cost::LiveConditions,
}

/// A rule's operator cost model: returns the *traditional* cost of one
/// operator instance (the term later staleness-penalised + neural-blended).
pub type OperatorCostFn = fn(&OperatorCostCtx) -> f64;
```

This is deliberately the union of what the `cost.rs` methods already consume
(stats, hardware calibration, selectivity) plus the live fingerprint — so the
moved bodies are near-verbatim. It also closes a gap noted in the 2026-06-01
cost audit: `IntegratedCostFn`'s calibration is static `from_profile` and does
**not** read the live fingerprint that `plan_builder` already uses; routing the
fingerprint through `OperatorCostCtx.live` lets the rule bodies tune to live
conditions, the same transform `plan_builder` applies.

## Pipeline

1. **Parse** (`ra-parser` rule_file_parser): add a `cost_model: Option<String>`
   field to `RuleFile` (the `## Cost Model` ```` ```rust ```` block). Today the
   parser drops it entirely.
2. **Compile** (`ra-engine/build.rs`): for physical-operator rules, emit the
   cost body as `fn <rule_id>_cost(ctx: &OperatorCostCtx) -> f64 { <body> }`
   into `$OUT_DIR/generated_costs.rs`, plus a registry
   `OPERATOR_COST_FNS: &[(&str /*RelLang op*/, OperatorCostFn)]` keyed by the
   operator the rule lowers to (declared via a new frontmatter field
   `costs_operator: hash-join`). Bodies that don't compile against
   `OperatorCostCtx` are rejected at build time (same discipline as the rewrite
   compiler), so a bad cost model can't silently ship.
3. **Dispatch** (`IntegratedCostFn::cost`): for each `RelLang` enode, look up
   the registry fn for that operator; if present, call it; else fall back to the
   built-in default (the current `cost.rs` body, retained as the fallback until
   every operator is migrated).
4. **Blend** (unchanged): the rule cost is the `traditional` value in
   `HybridCostFn`: `blend = α·(traditional × neural_multiplier) + (1−α)·traditional`,
   α from `SystemFingerprint::compute_blend_alpha`, capped at 0.9. The staleness
   penalty stays where it is (or moves into the ctx-aware body). **BitNet and the
   rule cost are not alternatives — the rule cost is the analytic base BitNet
   scales.**

## Why this is the right blend (final-cost recipe)

```
op_cost      = rule_cost_fn(ctx)              // from the .rra physical rule
             = f( stats, selectivity, hw-calibrated rates, live fingerprint )
traditional  = op_cost × staleness_factor(staleness)   // robustness to mis-estimation
neural       = traditional × bitnet_multiplier(node_features, fingerprint)
final        = α·neural + (1−α)·traditional,  α = confidence-weighted, ≤ 0.9
```

So all four ingredients participate: `ra_stats` + selectivity (in `op_cost`),
staleness factor, `ra_hardware` `CalibratedCostModel` (the rates inside
`op_cost`), and `BitNet` (the multiplier). The only change vs. today is *where
`op_cost`'s formula lives* — the `.rra` rule instead of `cost.rs`.

## Phased plan

- **P1 — Foundation (beachhead):** parse `## Cost Model` into `RuleFile`; add
  `OperatorCostCtx`/`OperatorCostFn` + the registry codegen; migrate **one**
  operator (`hash-join`) end-to-end and prove `IntegratedCostFn` dispatches to
  the rule body with identical plan choices (golden-cost test vs. the retained
  built-in).
- **P2 — Migrate the operators:** move each `cost.rs` method body into a
  `## Cost Model` rule; keep the built-in as fallback; per-operator golden-cost
  equivalence test. **Done — every egg `CostFunction` operator arm is now
  rule-sourced:**
  - *Join family* (`costs_operator:` on the `join-lowering-core` rules):
    `hash-join` (P1), `merge-join`, `nest-loop`, `index-nest-loop`.
  - *Other operators* (cost-only rules under `rules/cost-models/operators/` —
    frontmatter `costs_operator:` + `## Cost Model`, no `## Rewrite`):
    `scan` (shared by `scan`/`scan-alias`), `filter`, `project`, `join`
    (logical), `aggregate`, `sort`, `incremental-sort`.
  - `OperatorCostCtx` was extended with `row_count`, `simd_width_bits`, and
    `cpu_cores`; `IntegratedCostFn` dispatches via `op_cost_ctx` /
    `flat_op_cost` / `rule_operator_cost`, with the prior built-in as fallback.
    Each operator has a golden-cost test asserting rule == retired built-in, so
    no plan choices change. (The `Limit` startup-cost arm is plan-shape logic,
    not a per-operator base cost, and the analytic `IntegratedCostModel`
    methods — `index_scan_cost`/`bitmap_*`/`parquet`/`vector` — are a separate
    surface not used by the e-graph extractor; both stay built-in for now.)
- **P3 — Retire the built-ins (DONE):** every duplicated built-in formula was
  removed from the egg `CostFunction` arms. Each arm now dispatches through a
  single `operator_cost(op, ctx) = rule_operator_cost(op, ctx).unwrap_or(
  UNREGISTERED_OPERATOR_COST)`; the `.rra` `## Cost Model` rules are the **sole**
  source of operator cost (the formulas no longer live in two places and cannot
  drift). `UNREGISTERED_OPERATOR_COST` (1e6) is a fail-safe sentinel for a
  missing rule, which `all_migrated_operators_have_registered_cost_rules` proves
  unreachable. The per-operator golden tests are retained and now pin the `.rra`
  math directly.
- **P4 — Validate plan QUALITY (DONE):** ran the A/B on a freshly-built PG19
  cluster (pgrx `--features pg19`; required a pgrx-fork fix for a
  `TransactionId{Precedes,Follows}` binding collision) with the extension
  installed and FK-consistent synthetic TPC-H data. `ra-bench compare`
  (`ra_planner.enabled` on/off, `EXPLAIN ANALYZE`) over the corpus: 27 Ra-planned
  queries, **27/27 result-correct** (content-verified), **median execution
  speedup 1.08x** (parity — 14 Ra-faster / 13 PG-faster; worst cases sub-ms
  queries dominated by fixed parse overhead). This qualifies the migration:
  rule-driven costs produce correct, competitive plans. (The A/B harness's
  `verify_results` was also fixed to content-compare sorted result sets and
  distinguish Ra-errors/DML from genuine mismatches, replacing a misleading
  row-count proxy.) The honesty mandate is satisfied — parity is measured, not
  assumed.

## Live-conditions plan steering (Options A & B)

Once costs are rules, the live system fingerprint (`hit_rate` / `io_saturation`
/ `cpu_load`) is threaded into the rule `ctx` (via `with_live_conditions`
re-tuning the calibration rates). Measurement showed it changed plan **choice**
on 0/6 queries, because operators were costed in a single category (all
`tuple_cost`) so the rates scaled them uniformly. Two changes make live
conditions actually steer plans:

- **Option A — differentiate the I/O-vs-CPU split of existing operators (DONE).**
  Each join method's `.rra` cost is now a sharp blend of `tuple_cost` (CPU) and
  `seq`/`rand_page_cost` (I/O) reflecting its physical character (hash ~90% CPU,
  nest-loop ~90% random I/O, merge sequential-I/O-leaning, index-nl random-I/O).
  Weights sum to the prior coefficient so neutral behaviour matches the
  validated baseline. Result (measured): live conditions now reorder methods —
  a warm cache flips hash-join → nested-loop in the mid-size-join regime
  (`live_conditions_flip_join_method`). But the flip window is **narrow**:
  join-method choice is dominated by input *size* (the work amount), so large
  joins stay hash regardless and the tested real TPC-H queries did not flip.
  A is the right physical foundation but is **not sufficient** on its own.

- **Option B — promote the scan-method decision into the e-graph (NEXT).** The
  robust lever. Sequential-scan vs index-scan (vs bitmap) is a *sharp*
  I/O-vs-CPU tradeoff on the **same input**, present in nearly every query — not
  a narrow crossover. Today that choice is a `plan_builder` peephole, so the
  optimizer never weighs a cache-cheap index scan against a seq scan, and live
  conditions can't touch it. Plan:
  1. Add `index-scan` (and later `bitmap-scan`) e-graph operators in
     `egraph/lang.rs` (mirroring the physical-join variants), lowering back to a
     `Scan` in `from_rec` with the method captured into `PhysicalChoices`.
  2. Cost-only `.rra` rules: `seq-scan` = pages·`seq_page_cost` (sequential I/O);
     `index-scan` = `selectivity`·rows·`rand_page_cost` + per-row `tuple_cost`
     (random I/O, sensitive to `hit_rate`). Extend `OperatorCostCtx` with
     `selectivity` (already reserved in the RFC) and per-scan index availability.
  3. A lowering rule `scan → index-scan` guarded by a `has_index_for` condition
     (needs index metadata in `RelData`/`table_info`).
  4. `plan_builder` consumes the chosen scan method from `PhysicalChoices`
     (the seq/index peephole becomes a fallback, then retires).
  5. Validate with the debug-GUC sweep (force `hit_rate` high → expect
     index-scan; low/`io_sat` high → expect seq-scan) + the PG19 A/B
     (correctness + exec-time improvement on selective-predicate queries).

## Risks / open questions

- **Compile surface:** generated cost bodies must compile against a *stable*
  `OperatorCostCtx`; the build.rs codegen must `include!` them inside a module
  that imports the ctx type. Bad bodies fail the build (intended).
- **Per-node cost is hot:** the registry is a `match`/array dispatch (fn
  pointers), ~same cost as today's `match enode`. No dynamic dispatch on the hot
  path.
- **Logical-rule benefit models:** left as documentation in P1–P4; a follow-up
  could feed them into the rule advisor's ordering, but that is *not* plan cost
  and is out of scope here.
- **Cardinality, not just child cost:** egg's cost fn only sees child *costs*.
  `OperatorCostCtx.rows_*` must be reconstructed from `RelAnalysis`
  (`estimated_rows`, already tracked) — confirm it is populated for every
  operator before relying on it in a rule body.
