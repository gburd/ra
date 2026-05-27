# PostgreSQL GEQO vs. Ra

PostgreSQL's [Genetic Query Optimizer][geqo-doc] (GEQO) is the planner's
escape hatch for queries that join too many relations for the
near-exhaustive System R / dynamic-programming search to remain
tractable. Ra solves the same "the search space is too big" problem
with a fundamentally different mechanism — equality saturation over an
e-graph, guided by a learned cost model and a speculative router. This
document compares the two, points out where they overlap, and records
the lessons GEQO offers Ra and vice versa.

The GEQO source for this comparison is `~/src/postgres/src/backend/optimizer/geqo/`
(REL_18_STABLE branch, May 2026), in particular `geqo_main.c` and
`geqo_eval.c`. Configuration defaults come from `postgresql.conf.sample`
and `src/include/optimizer/geqo.h`.

[geqo-doc]: https://www.postgresql.org/docs/current/geqo.html

> **Related Ra-side proposal**: [RFC 0035 — Genetic Query Optimizer
> for Large Join Graphs](../../rfcs/0035-genetic-query-optimizer.md)
> proposes adding a GA fallback *to Ra* for the same large-join case
> GEQO solves in PostgreSQL. This document is the upstream-research
> companion: it surveys what GEQO actually does, so a future decision
> on RFC 0035 can be made against accurate prior art rather than the
> sketch in the RFC itself.

## Side-by-side

| Concern | PostgreSQL GEQO | Ra |
|---|---|---|
| Triggering condition | `enable_geqo=on AND levels_needed >= geqo_threshold` (default 12 relations) | Always on for every query that survives the speculative router's `Skip` decision |
| Search algorithm | Steady-state genetic algorithm: pool of integer permutations representing join orders | Equality saturation: e-graph that holds *all* equivalent plans simultaneously, plus rewrite rules that grow it |
| Search representation | Each candidate is a Gene[] — an integer permutation of relation IDs encoding a join order, treated like a Travelling Salesman tour | Each candidate is an e-class in an e-graph; rules add e-nodes; extraction picks the cheapest representative |
| Fitness function | Standard planner's join-tree cost (`gimme_tree` builds a `RelOptInfo` from the tour, returns `joinrel->cheapest_total_path->total_cost`) | `IntegratedCostFn` (hardware + statistics + staleness) plus optional BitNet 1.58-bit neural cost model |
| Coverage | Join order only — scan/join methods, indexes, sort orders, predicate placement, projections still come from the standard planner | Whole-plan: 307 active rules cover predicate pushdown, projection pruning, join reordering, set-op rewrites, aggregate pushdown, CTE inlining, expression simplification, semi-join reduction, etc. |
| Determinism | Deterministic given `geqo_seed` (PRNG seeded per query); changes to `geqo_seed` change the plan | Deterministic given the same fingerprint, rules, statistics, hardware profile, and cost model |
| Termination | Fixed budget: `pool_size` × `number_generations`, both functions of `geqo_effort` (1–10) and `nr_rel`. Default at threshold (12 rels): pool ≈ min(2¹³=8192, 250) = 250 individuals × 250 generations ≈ 62,500 fitness evaluations | Adaptive budget chosen by the speculative router from a 16-D feature vector: 3/8/15/20 iterations and 5/15/50 ms wall-time, with a continuation gate that stops early if cost improvement < 0.1 % |
| Self-tuning | None — `geqo_effort`, `geqo_pool_size`, `geqo_generations`, `geqo_selection_bias`, `geqo_seed` are static GUCs | Online: every executed plan emits an `OptimizationTrace` (features, per-iter costs, termination cause, optimal stopping point). The trainer batches 64 traces, updates the BitNet model, and snapshots every 256 steps |
| Memoisation | None across runs. Within one run, the GEQO docs explicitly call out that "different candidates use similar sub-sequences of joins, a great deal of work will be repeated" as a known future-work item | Plan cache keyed by genetic fingerprint; reported 97.5 % hit rate on a 5-template × 40-variation OLTP integration test (real-workload measurement is open) |
| Hard-failure mode | If the random tour can't be force-joined into a single `RelOptInfo` (LATERAL or join-order restrictions), `gimme_tree` returns NULL and `geqo` calls `elog(ERROR, "geqo failed to make a valid plan")` | Egg's `Runner` returns a `StopReason`; if the e-graph saturates without finding a complete cost-able plan, extraction falls back to the input expression. There's no equivalent fail-the-query path |
| Bushy plans | Yes, but as a side effect — the original GEQO produced left-deep tours; the current `gimme_tree` postpones illegal/undesirable joins via the "clumps" mechanism, which incidentally yields bushy shapes | Yes, by construction — `join-associativity`, `left-deep-to-bushy`, and `dphyp-join-reorder` rules participate in the search. Whether bushy is selected is a cost decision |
| Treatment of non-join optimisations | All other transformations happen *before* GEQO is called (`subquery_planner` → `query_planner` → `make_one_rel`); GEQO is purely a join-ordering escape hatch | All transformations are unified into the same equality-saturation pass; cost extraction picks the best combination across categories simultaneously |
| Failure containment | None: `elog(ERROR)` aborts the query | Per-rule-category `catch_unwind` wrappers in `all_generated_rules()` mean one malformed generated rule cannot drop the rest of the batch (post-2026-05-26, see Item 6 in the audit changelog) |

## How GEQO actually works

GEQO replaces the System R DP search [`make_rel_from_joinlist`] but
keeps PostgreSQL's `make_join_rel` / `set_cheapest` / `add_path`
machinery for evaluating any one tour. Per `geqo_main.c`:

```text
loop pool_size times:
    chromosome <- random permutation of [1..nr_rel]
    chromosome.fitness <- gimme_tree(chromosome).cheapest_total.total_cost

loop number_generations times:
    momma, daddy <- linear-bias selection from sorted pool
    kid <- recombine(momma, daddy)        ;; ERX by default; PMX/CX/PX/OX1/OX2 compile-time options
    kid.fitness <- gimme_tree(kid).cheapest_total.total_cost
    spread_chromo(kid, pool)              ;; insert sorted, evict worst

return gimme_tree(pool[0]).joinrel        ;; fittest tour wins
```

Two details matter for the comparison:

1. **`gimme_tree` is not a left-deep builder.** The current
   implementation walks the tour adding each new relation to the first
   "clump" it can legally and "desirably" join with (`desirable_join`
   prefers joins on common columns). Failing that, it starts a new
   clump. After the tour is exhausted, remaining clumps are
   force-joined in any legal order. This produces bushy plans where
   they are the only legal shape (LATERAL, etc.) and incidentally
   improves quality for queries where they're cost-effective.

2. **The fitness function is the rest of PostgreSQL's optimiser.**
   For each tour evaluation, `make_join_rel` enumerates every relevant
   nested-loop / hash / merge variant, considers every interesting
   sort order, and runs the same `set_cheapest` machinery the regular
   planner uses. GEQO is a join-ordering shell around an unchanged
   inner planner.

## How Ra solves the same problem

Ra never falls back to a separate algorithm. Every query goes through
the same pipeline:

```text
SQL → RelExpr → speculative router (~80 ns BitNet predict_all)
                    │
                    ├── SKIP        : trivial query, return as-is
                    ├── LEFT_DEEP   : equi-join chain → cardinality-ordered tree
                    └── EGRAPH_*    : equality saturation, budget chosen from features
                                       │
                                       └── extract → ordering pass → PlannedStmt
```

The speculative router is the closest moral equivalent to GEQO's
`enable_geqo && levels_needed >= geqo_threshold` check, but with two
qualitative differences:

- It runs on a 16-D feature vector that includes table count *plus*
  predicate complexity, expected selectivity, join-graph shape, and
  current statistics-cache staleness. GEQO uses only `nr_rel`.
- It picks one of five paths, not two. Easy queries skip the search
  entirely; medium queries get a tight e-graph budget; large queries
  get a generous one. There is no abrupt cliff at threshold − 1
  vs. threshold.

For the queries that reach equality saturation, the e-graph
fundamentally differs from a GA: it holds *every* derivation
simultaneously, so two rule applications that produce the same
expression coalesce into the same e-class. There's no "population"
that loses diversity over generations and there's no per-candidate
re-cost-from-scratch — the cost function decorates each e-class with
a `Cost` value that's recomputed only when its children change.

## Empirical comparison (TPC-H SF=0.01, 21 queries, M3 Max release build)

From [`benchmarks/ra-vs-pg18-head-to-head.md`](../../benchmarks/ra-vs-pg18-head-to-head.md):

| Metric | Ra v0.4.0 | PostgreSQL 18.4 (no GEQO; default planner)|
|---|---|---|
| Geo-mean planning time | 12.8 µs | 1089 µs |
| Speedup | 89× | — |
| Range | 3.4–37.6 µs | 434–3425 µs |

That comparison is against the System R DP planner, not GEQO — TPC-H
SF=0.01 only goes up to 9 base relations per query, below
`geqo_threshold = 12`. To compare against GEQO directly we'd need
either JOB-style queries (17 relations) or to lower
`geqo_threshold`. Both are tracked as follow-up benchmarks.

## What Ra can learn from GEQO

GEQO has been in production since the late 1990s. It has survived
because Utesch's original design got several things right that Ra
should explicitly preserve.

### 1. Bound the search by *evaluations*, not iterations

GEQO's budget is `pool_size × number_generations`, both quadratic
functions of relation count clamped by `geqo_effort`. The hard cap on
fitness evaluations means GEQO's worst-case planning time is
predictable from `nr_rel` alone. Ra's per-query budget is set in
*iterations* and *milliseconds*; iterations are loose because rule
fan-out can balloon e-class count. The iter/timeout pair already
covers this in practice but it's an indirect measure of "how much work
have we done?".

**Action**: continue capping by node-count and rule-applications, not
just iterations. The existing `default_iter_limit_for_tables` and
`default_timeout_ms_for_tables` heuristics in
`crates/ra-engine/src/egraph/optimizer.rs` are the right shape; the
PG18 vs Ra benchmark notes (insight 2026-05-11) already flagged 6+
table joins as the regression boundary where saturation runs long.
Make the e-graph node-count cap a first-class config knob alongside
the iter/timeout pair, mirroring `geqo_pool_size`/`geqo_generations`
as a hard upper bound on work.

### 2. Make the random seed observable and reproducible

`geqo_seed` is exposed as a GUC precisely so that operators can
reproduce a surprising plan. Ra's plan is deterministic given the
same fingerprint, rules, statistics, hardware profile, and cost-model
weights — but those last two are not visible at the SQL/EXPLAIN level
right now. A user who reports "this plan changed overnight" has no
direct way to ask "what changed?". GEQO has the easier job (it's just
the seed) but the principle generalises.

**Action**: include the cost-model snapshot ID and statistics
timestamp in EXPLAIN output (or expose them as a session GUC like
`ra.cost_model_id`). RFC 0059 statistics-based plan-cache invalidation
is adjacent and could share infrastructure.

### 3. The "clumps" trick: don't force a bad shape just because the algorithm produced one

`gimme_tree` doesn't faithfully execute the GA's tour. If the next
gene in the tour can't be joined desirably (no shared columns, would
force a Cartesian product), it postpones the relation and tries again
later. The GA explores tours; the evaluator picks the best legal
shape close to the proposed tour. This decoupling is a big part of
why GEQO produces useful plans even though the search representation
is naïve.

Ra has the analogous opportunity: the speculative router proposes a
*budget*, not a *plan shape*. The e-graph extraction currently picks
the lowest-cost extraction *given the rules that fired*. If a rule
category is non-productive on this query (e.g. multi-model rules on a
pure-relational query), there's no penalty for not running it — the
cost is the time we spent considering it. The `RuleAdvisor`'s 3-stage
filtering (context → query-shape → learned ranking) already does this
selection but the second stage's heuristics could borrow from
`desirable_join`: "before applying join-reordering rules, check
whether the join graph admits a non-Cartesian ordering at all".

**Action**: add a query-shape pre-check that demotes join-reorder
rule categories on graphs where every legal ordering is forced (e.g.
LATERAL chains, single connected component with a unique
spanning shape). Saves rule-application cost on queries that look
like they need reordering but don't.

### 4. Failure should not be unrecoverable

GEQO's `elog(ERROR, "geqo failed to make a valid plan")` is the only
backstop and it kills the query. Ra inherits the lesson by
*contrast*: the per-rule-category `catch_unwind` added by audit Item 6
means one malformed rule no longer drops the entire generated batch.
This is the right direction; document it as a design principle so
future contributors keep the property.

**Action**: add a "rule-isolation" section to
`docs/internals/optimizer-architecture.md` documenting the policy
that one bad rule must not block the rest, and reference the build-time
metavariable-in-operator-position check
(`crates/ra-engine/build.rs::check_sexp_invalid`) as the first line
of defence.

## What GEQO can't easily learn from Ra

These are the differences that *aren't* portable back to GEQO without
a much bigger rewrite, and so are interesting because they characterise
where Ra's design genuinely diverges.

- **Equality saturation requires you to commit to a single
  representation of all candidate plans.** PostgreSQL's `RelOptInfo` /
  `Path` /`Plan` triple is shaped around the System-R DP search; e-graph
  embedding would mean rewriting the whole optimiser. GEQO can't adopt
  e-graphs incrementally.
- **Online learning requires execution feedback.** PostgreSQL's hook
  surface (`planner_hook`, `ExecutorEnd_hook`) makes this technically
  possible, but the GUC framework isn't built for "the optimiser
  changes its weights between queries". Ra's training coordinator
  assumes a stable identity (the cost model object) that updates
  in-place. PG would need to surface a "model snapshot" abstraction
  it doesn't have today.
- **Whole-plan rewrite categories**. GEQO's universe is "what order do
  I join these tables in?". Ra's universe is "what's an equivalent
  plan?", which subsumes ordering, predicate placement, projection
  pruning, set-op rewrites, etc. Adopting whole-plan rewrites in
  PostgreSQL would mean reorganising `subquery_planner` →
  `grouping_planner` → `query_planner` into a single rewrite-driven
  pass — a much larger change than swapping out the join-ordering
  algorithm.

## Summary

GEQO and Ra solve the same problem at different scales of ambition.
GEQO is a clean, narrow, three-decade-stable join-ordering escape
hatch for the System R DP planner; Ra replaces the planner entirely
and unifies join-ordering with every other transformation under a
single equality-saturation search.

The lessons Ra should adopt from GEQO are about
*disciplined-budgeting*, *reproducibility*, *robust-shape selection*,
and *failure containment* — not about the algorithm. The algorithm
itself is the smaller question; GEQO and Ra reach answers for the same
queries via fundamentally different mechanisms, and Ra's mechanism is
a strict super-set in expressive power. The discipline is what's
worth borrowing.

## References

- `~/src/postgres/src/backend/optimizer/geqo/geqo_main.c` — GA loop
- `~/src/postgres/src/backend/optimizer/geqo/geqo_eval.c` — `gimme_tree`, clumps, fitness
- `~/src/postgres/src/include/optimizer/geqo.h` — defaults and effort knob
- `~/src/postgres/doc/src/sgml/geqo.sgml` — Utesch's original write-up
- [The egg paper (Willsey et al., 2021)](https://arxiv.org/abs/2004.03082) — equality saturation
- [`crates/ra-engine/src/egraph/optimizer.rs`](../../crates/ra-engine/src/egraph/optimizer.rs)
  — Ra's saturation loop
- [`crates/ra-engine/src/speculative_router.rs`](../../crates/ra-engine/src/speculative_router.rs)
  — adaptive budget selection
- [`benchmarks/ra-vs-pg18-head-to-head.md`](../../benchmarks/ra-vs-pg18-head-to-head.md)
  — TPC-H SF=0.01 head-to-head numbers
