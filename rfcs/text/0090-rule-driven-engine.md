# RFC 0090: Ra as a Rule-Driven Engine

- Start Date: 2026-06-05
- Author: gregburd
- Status: Draft
- Tracking Issue: TBD
- Supersedes the sidecar disposition of RFC 0087; subsumes RFC 0089.

## Summary

Make the `.rra` rule corpus the **single source of truth** for every
planning and optimization decision Ra makes — logical rewrites,
physical-operator lowering, index selection, costing, and route/stop-early
control flow. The core of Ra becomes a small, stable engine that *runs
rules*, drives the dual cost model (the `IntegratedCostFn` + the BitNet
neural model), and consumes live dataflow about system and database state.
All decision logic moves out of hand-coded Rust and into structured,
declarative rule files that researchers can add, edit, and remove without
touching the engine.

The end state:

- `crates/ra-engine/src/rewrite.rs` and the per-category hand-coded rule
  functions (`predicate_pushdown_rules()`, `join_reordering_rules()`, …)
  shrink to **zero** executable rules.
- The `PhysicalChoices` sidecar (RFC 0087) is **retired**; physical method
  selection happens via cost-scored physical-lowering rules (RFC 0089),
  authored in `.rra`.
- The speculative router's hard-coded fast-route / stop-early logic is
  expressed as preconditioned **routing rules** (Gap B).
- A `.rra` file's e-graph pattern, preconditions, **and cost function** are
  all structured data the engine consumes directly.

## Motivation

Ra's premise is that query-optimization knowledge belongs in an editable
rule corpus, so the engine can be a research platform: companies and
schools try new transformation/costing methods by changing rule files, not
engine internals. Today that premise is only partially realized, and the
gap is large enough to undermine the premise.

### The current reality (grounded in the code)

There are **two** rule systems, and the declarative one is not
authoritative:

1. **Hand-coded egg rewrites** — `crates/ra-engine/src/rewrite.rs` defines
   ~213 rules as Rust `rewrite!` macros inside per-category functions.
   `lazy_rules.rs::load_category` dispatches every `RuleCategory` to one of
   these functions (`RuleCategory::FilterOptimization =>
   crate::rewrite::predicate_pushdown_rules()`, etc.). **These are the
   rules that actually drive optimization.**

2. **Generated rules from `.rra`** — `crates/ra-engine/build.rs` scans
   `rules/`, extracts the `rewrite!`/`rw!` code block from each file's
   `## Implementation` section, and emits `$OUT_DIR/generated_rules.rs`,
   exposed as `all_generated_rules()`. Of 1,387 `.rra` files, ~94 produce
   active rules; the rest are prose (no `rewrite!` block) or reference
   condition functions that don't exist yet.

So the `.rra` corpus is, in practice, **documentation plus a supplementary
rule set** — not the source of truth. Worse, the current build script is
not a declarative compiler: it **extracts literal Rust `rewrite!` macro
text** embedded in the `.rra` and pastes it into generated code. The rule's
"meaning" is still Rust, not structured data.

Two further decision layers live entirely in hand-coded Rust, invisible to
the rule corpus:

- **Physical-operator selection** — the `PhysicalChoices` sidecar
  (RFC 0087) plus `plan_builder` heuristics (the index-scan and
  index-nested-loop choices added for filtered joins). The cost model never
  sees the physical alternatives, because `RelExpr` is logical-only.
- **Routing / stop-early** — `is_trivial_query`, the speculative router,
  and `try_fast_route` decide whether to skip the e-graph entirely. This is
  why the predicate-pushdown rules "didn't fire" on simple joins: the fast
  route bypasses the only place rules execute.

### Why this matters

If physical lowering (RFC 0089) ships as written, it adds ~15 **more**
hand-coded `rewrite!` rules — deepening the exact gap this RFC closes. The
correct order is: make the corpus authoritative first, then author physical
and routing rules as `.rra` from day one.

## Guide-level explanation

A researcher adds a new optimization by writing a `.rra` file. Nothing in
`ra-engine`'s source changes. A logical rewrite looks like:

```
---
id: filter-through-join-left
name: "Push filter through inner join (left)"
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb]
kind: rewrite                # rewrite | physical | routing
benefit_range: [0.2, 5.0]
---

## Rewrite

lhs: (filter ?pred (join inner ?cond ?left ?right))
rhs: (join inner ?cond (filter ?pred ?left) ?right)
when: pred_references_only(?pred, ?left)   # named precondition, optional
```

A physical-lowering rule (RFC 0089, now authored in `.rra`) carries a cost
function alongside the pattern:

```
---
id: join-to-index-nestloop
name: "Index nested-loop join"
category: physical/join-method
kind: physical
---

## Rewrite

lhs: (join inner ?cond ?outer ?inner)
rhs: (index-nestloop ?cond ?outer ?inner)
when: inner_has_btree_on_join_key(?cond, ?inner) and outer_is_selective(?outer)

## Cost

# Structured cost; the engine evaluates this against live stats + BitNet.
startup: rows(?outer) * index_descent_cost(?inner)
total:   rows(?outer) * (index_descent_cost(?inner) + rows_matched(?inner))
```

A routing rule (Gap B) declares when to stop early:

```
---
id: route-single-scan-skip
name: "Skip e-graph for trivial single-table scans"
category: routing
kind: routing
---

## Route

when: single_base_relation() and not has_subquery() and not has_aggregate()
action: skip          # skip | left-deep | egraph(budget)
```

`EXPLAIN` and behavior are unchanged for users. What changes is **where the
decision lives**: in the file, not the engine.

## Reference-level explanation

### The declarative rule schema

Each `.rra` `kind` maps to an engine construct:

| `kind`     | Compiles to                                   | Phase |
|------------|-----------------------------------------------|-------|
| `rewrite`  | `Rewrite<RelLang, RelAnalysis>` (logical)     | 1     |
| `physical` | lowering `Rewrite` + cost contribution        | 3     |
| `routing`  | a preconditioned `RouteRule` the router reads | 4     |

The schema fields:

- **`lhs` / `rhs`**: S-expression patterns over `RelLang` (the existing
  `define_language!` vocabulary in `egraph/lang.rs`). `RelLang: FromOp`, so
  `Pattern::from_str` already parses these — the same strings the hand-coded
  `rewrite!` macros use today. This is the crux that makes migration
  mechanical: today's `rewrite!("name"; "LHS" => "RHS")` is *already* a
  name plus two pattern strings.
- **`when`**: zero or more named preconditions from a fixed **condition
  vocabulary** (`crate::conditions`, which already implements
  `egg::Condition<RelLang, RelAnalysis>` and documents itself as "referenced
  by `.rra` rule files via `if condition_name(...)` syntax"). Each condition
  is engine code; the *reference* is data. New conditions unlock more rules.
- **`## Cost`** (physical rules): a structured cost expression over a fixed
  **cost vocabulary** (`rows(?x)`, `index_descent_cost(?x)`,
  `sort_cost(?x)`, …) that the engine evaluates against live statistics and
  blends with the BitNet model. This is how the cost model becomes
  rule-authored (Gap D).
- **Metadata**: `id`, `name`, `category`, `databases`, `benefit_range`,
  `complexity`, `priority`, `preconditions` — already present in the
  `Rule`/`RuleMetadata` types in `ra-core` and partly parsed by `build.rs`.

### The compiler

`build.rs` evolves from "extract embedded Rust `rewrite!`" to "compile
structured fields":

1. Parse `lhs`/`rhs`/`when` from the `## Rewrite` section (not a Rust code
   block). Emit `Rewrite::new(id, Pattern::from_str(lhs)?, applier)` where
   the applier is a plain `Pattern` for unconditional rules, or a
   `ConditionalApplier` wrapping the named conditions for `when` clauses.
2. Validate at build time: every `lhs`/`rhs` parses against `RelLang`;
   every `when` names a known condition; unknown/ malformed rules are
   rejected with a file:line diagnostic (the build script already rejects
   malformed patterns and wraps each category independently so one bad rule
   can't drop the batch).
3. Keep the embedded-`rewrite!` path working during migration (a rule may
   provide either a structured `## Rewrite` block or a legacy code block),
   so categories migrate one at a time.

### Making generated rules authoritative

`load_category` flips from returning hand-coded functions to returning the
generated rules for that category:

```rust
RuleCategory::FilterOptimization => generated::category("logical/predicate-pushdown"),
```

behind a feature flag (`rules-authoritative`) so each category is validated
before the hand-coded function is deleted.

### Physical lowering (subsumes RFC 0089)

RFC 0089's `RelLang` physical variants (`HashJoin`, `MergeJoin`, `NestLoop`,
`IndexNestLoop`, `HashAggregate`, `SortAggregate`), one-way lowering rules,
and per-physical cost estimates are implemented — but the **rules and their
cost functions are authored in `.rra`** (`kind: physical`), not hand-coded.
The `plan_builder` index-scan / index-NLJ heuristics added recently are
retired into these cost-scored rules; `plan_builder` becomes a pure renderer
of the extracted physical plan.

### Routing as rules (Gap B)

The router's hard-coded predicates (`is_trivial_query`, `OptRoute`
selection) become `kind: routing` rules: a precondition over query shape +
an `action` (`skip` / `left-deep` / `egraph(budget)`). The engine's router
shrinks to "evaluate routing rules in priority order, take the first
match." The BitNet difficulty prediction remains available as a condition
(`predicted_difficulty() < k`) so learned routing is expressible as a rule.

### Retiring the sidecar

Once physical lowering produces fully-physical extracted plans, the
`PhysicalChoices` sidecar is removed. Supplied plan-advice
(`SET ra_planner.plan_advice = …`) becomes a bias on the physical-lowering
rules (per RFC 0089 phase 6), not a separate map.

## Migration plan (phases)

0. **This RFC.** Align on schema + the authoritative-corpus direction.
1. **Compiler beachhead.** Structured `## Rewrite` compilation; migrate the
   predicate-pushdown category off hand-coded behind `rules-authoritative`;
   prove behaviorally identical.
1b. **Migrate remaining pure-pattern categories** (projection pushdown, join
    reordering/commutativity, expression simplification, …).
2. **Precondition + applier vocabulary.** Cover cost-dependent / computed-RHS
   rules declaratively; `rewrite.rs` → near-zero.
3. **Physical lowering as `.rra`** (RFC 0089), retiring `plan_builder`
   physical heuristics.
4. **Routing as rules** (Gap B).
5. **Retire the sidecar and the last hand-coded rules.** `.rra` is fully
   authoritative.

## Validation strategy

Correctness is the prime invariant; every phase is gated by:

1. **Rule-set parity** — the generated category produces the same rule
   names and patterns as the hand-coded function it replaces (unit-level).
2. **Behavioral identity** — `optimize()` produces identical optimized
   `RelExpr` for hand-coded vs generated rules across a query corpus
   (TPC-H, JOB, the cross-operator regression set). Diff must be empty.
3. **Ra-vs-PG regression diff** — the planned exhaustive PG-regression diff
   (Ra-on vs Ra-off, identical results) runs after each category flip.
4. **Feature flag** — `rules-authoritative` lets a category be validated and
   reverted independently; hand-coded code is deleted only after its
   category is proven.

## Drawbacks and risks

- **Expressiveness ceiling.** Some hand-coded rules use dynamic appliers
  (computed RHS, multi-step). Until the Phase-2 applier vocabulary exists,
  those stay hand-coded. The schema must not pretend to express what it
  can't; such rules are explicitly flagged, not silently dropped.
- **Cost-function-as-data complexity.** A structured cost grammar that's
  both expressive and safe is real work (Phase 3). The fallback is a fixed
  vocabulary of cost primitives rather than arbitrary expressions.
- **E-graph blowup from physical variants** (per RFC 0089): mitigated by
  one-way lowering and feature gating.
- **Planning-time regression risk.** Generated rules must be cached as
  aggressively as the hand-coded ones (the `LazyRuleCompiler` cache already
  does this per category).

## Unresolved questions

- Cost grammar: fixed primitive vocabulary vs a small expression language?
  (Decide at Phase 3 design review.)
- Routing rules: can all current router behavior be captured by
  precondition + action, or are there decisions needing engine state the
  vocabulary can't yet name? (Decide at Phase 4.)
- Do we keep S-expression patterns as the surface syntax, or introduce a
  friendlier pattern notation that compiles to S-expressions? (Surface
  ergonomics; deferred.)

## References

- RFC 0087 — physical-operator selection (sidecar; this RFC retires it).
- RFC 0089 — e-graph cost-driven physical lowering (this RFC subsumes it,
  authoring its rules in `.rra`).
- RFC 0004 — formal preconditions (the `PreCondition` type the `when`
  vocabulary builds on).
- `crates/ra-engine/build.rs` — the existing `.rra` → rule generator.
- `crates/ra-engine/src/rewrite.rs` — the hand-coded rules to retire.
- `crates/ra-engine/src/conditions.rs` — the condition vocabulary.
- `crates/ra-engine/src/egraph/lang.rs` — the `RelLang` pattern vocabulary.
- `crates/ra-engine/src/plan_advice_physical.rs` — the sidecar to retire.
