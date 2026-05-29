# RFC 0089: E-Graph Cost-Driven Physical Lowering

- Start Date: 2026-05-29
- Author: gregburd
- Status: Draft
- Tracking Issue: TBD

## Summary

Promote physical-method selection from the post-extraction
`PhysicalChoices` sidecar (RFC 0087) into the e-graph itself
via cost-driven rewrite rules that lower logical operators to
their physical counterparts (`Join` -> `HashJoin` /
`MergeJoin` / `NestLoop`; `Aggregate` -> `HashAggregate` /
`SortAggregate`). This lets the cost extractor reason about
logical plan shape and physical method together — the
Cascades / Volcano pattern — instead of deciding them in
sequence. This RFC scopes the work as a multi-phase migration
that preserves Ra's current sidecar performance while opening
the door to physical-aware join enumeration when the workload
demands it.

## Motivation

### Workload demand

This RFC is justified by **analytical workloads where physical-
method choice changes the optimal join order**. The sidecar
approach (RFC 0087) makes physical decisions *after*
extraction, so the cost extractor never sees that a different
join order would have produced cheaper physical methods. For
the workloads Ra targets today (PG drop-in OLTP, fast simple
plans), this trade-off is correct — physical methods are
near-trivial and join orders are short. But three workload
classes break that assumption:

1. **TPC-H / TPC-DS analytical queries** with 7-12 way joins
   on tables of dramatically different sizes. The optimal
   join order depends on which intermediate results fit in
   memory for hash-join builds. PG's planner gets this right
   precisely because it interleaves logical and physical
   choices in the path-cost model. Ra's sidecar approach
   produces correct plans but sometimes picks a join order
   that's optimal *for hash join everywhere* when in reality
   one merge-join branch would have unblocked a much better
   logical order.
2. **Sort-rich workloads** (TPC-DS Q4, JOB heavy queries)
   where some join orderings would benefit from a merge-join
   that exploits already-sorted index scans. The sidecar
   approach misses these because the e-graph never compares
   "this order with merge join" against "that order with
   hash join".
3. **Cardinality-skewed joins** where one branch has 10
   rows and the other has 10M. Nested-loop with the small
   side outer is cheap, but only if you knew during join
   enumeration that NestLoop was the physical method. The
   sidecar finds out too late.

### Quantifying the gap

Ra's current head-to-head benchmark vs PG 18.4 measures
**planning time only** (89× geomean speedup). The plan-
quality benchmark in
`benchmarks/planner_comparison/` measures end-to-end
execution time and shows Ra's plans are within 5% of PG's
on TPC-H SF=0.01 — but the gap widens to 20-40% on TPC-DS
SF=0.1 specifically on queries where physical-method choice
would have justified a different join order. Those queries
are the workload demand for this RFC.

### Why now isn't necessarily later

The sidecar design has shipped (RFC 0087, commits
`77e77eb9`-`204eae2c`). It works at production quality for
Ra's primary workload. This RFC isn't urgent fix-up; it's
forward-looking architecture work for when the workload
moves toward analytical use cases. We file it now so the
design space is documented and the migration path is clear,
not because we're committing to ship it next quarter.

## Guide-level explanation

After this RFC is implemented, the user-visible plan output
distinguishes physical join methods produced by the e-graph:

```sql
-- Today (sidecar approach, RFC 0087):
EXPLAIN SELECT ... FROM lineitem l JOIN orders o ON l.orderkey = o.orderkey;
-- Hash Join          <- chosen by plan_builder consulting
--   ...                 PhysicalChoices (cost-driven default)
--   ...

-- After this RFC (e-graph extraction):
EXPLAIN SELECT ... FROM lineitem l JOIN orders o ON l.orderkey = o.orderkey;
-- Hash Join          <- chosen by e-graph extraction comparing
--   ...                 (HashJoin with this order) vs
--   ...                 (MergeJoin requiring sort) vs
--                       (NestLoop with small inner)
```

The e-graph now contains physical variants for every join,
and the cost extractor picks the global minimum across both
logical and physical choices. The sidecar stays around as the
fallback for queries the e-graph can't fully lower.

For supplied plan-advice, behavior is unchanged: the user
says `HASH_JOIN(b)` and the planner produces `HashJoin`. The
internal mechanism becomes "advice biases the e-graph
extractor" rather than "advice writes into PhysicalChoices",
but the EXPLAIN output is the same.

## Reference-level explanation

### Phase 1: physical operator variants in the e-graph language

Ra's e-graph (egg) operates on `RelLang`, an enum of
operators. Today `RelLang` mirrors `RelExpr`'s logical
operators. We add physical variants:

```rust
// crates/ra-engine/src/egraph/mod.rs
enum RelLang {
    // Existing logical:
    Scan(Symbol),
    Filter([Id; 2]),
    Project([Id; 2]),
    Join([Id; 3]),       // [cond, left, right]
    Aggregate([Id; 3]),  // [groups, aggs, input]
    // ...

    // NEW physical variants:
    HashJoin([Id; 3]),
    MergeJoin([Id; 3]),
    NestLoop([Id; 3]),
    HashAggregate([Id; 3]),
    SortAggregate([Id; 3]),
    IndexScan(Symbol),
    SeqScan(Symbol),
}
```

`RelLang` is not user-facing; it's an internal e-graph
representation. The mapping `RelExpr -> RelLang` (in
`egraph/to_rec.rs`) and `RelLang -> RelExpr` (in
`egraph/from_rec.rs`) handle the new variants symmetrically.

### Phase 2: physical lowering rewrite rules

```rust
// crates/ra-engine/src/rewrite_physical.rs
fn join_to_hash_join() -> Rewrite<RelLang, RelAnalysis> {
    rewrite!("join->hash_join";
             "(join ?cond ?l ?r)" =>
             "(hash-join ?cond ?l ?r)"
             if is_equi_join("?cond"))
}

fn join_to_merge_join() -> Rewrite<RelLang, RelAnalysis> {
    rewrite!("join->merge_join";
             "(join ?cond ?l ?r)" =>
             "(merge-join ?cond ?l ?r)"
             if is_equi_join("?cond"))
}

fn join_to_nest_loop() -> Rewrite<RelLang, RelAnalysis> {
    // No equi-join precondition: NestLoop handles arbitrary
    // join conditions.
    rewrite!("join->nest_loop"; "(join ?cond ?l ?r)" => "(nest-loop ?cond ?l ?r)")
}
```

Three rules per logical operator. The cost extractor picks
the cheapest among `Join`, `HashJoin`, `MergeJoin`, `NestLoop`
in each e-class.

Critical correctness invariant: the lowering rules are *one-
way*. `Join -> HashJoin` is valid; the reverse rule
`HashJoin -> Join` is not part of the rule set. This keeps
saturation finite — once an e-class has been lowered, it
won't logically rewrite back.

### Phase 3: per-physical-method cost estimates

The existing `IntegratedCostFn` returns a `Cost` per
e-node. Today it costs `Join` uniformly (using a generic
join-cost heuristic). After this RFC:

```rust
// crates/ra-engine/src/cost.rs
match enode {
    RelLang::HashJoin([_, l, r]) => {
        let l_rows = analysis(*l).cardinality;
        let r_rows = analysis(*r).cardinality;
        Cost {
            cpu: l_rows + r_rows,
            memory: r_rows * row_width,
            startup_cpu: r_rows * hash_build_cost,
            // ...
        }
    }
    RelLang::MergeJoin([_, l, r]) => {
        let l_rows = analysis(*l).cardinality;
        let r_rows = analysis(*r).cardinality;
        let l_sorted = analysis(*l).is_sorted_on(join_key);
        let r_sorted = analysis(*r).is_sorted_on(join_key);
        Cost {
            cpu: l_rows + r_rows,
            startup_cpu:
                if l_sorted { 0.0 } else { sort_cost(l_rows) }
                + if r_sorted { 0.0 } else { sort_cost(r_rows) },
            // ...
        }
    }
    RelLang::NestLoop([_, l, r]) => {
        let l_rows = analysis(*l).cardinality;
        let r_rows = analysis(*r).cardinality;
        Cost {
            cpu: l_rows * r_rows,
            startup_cpu: 0.0,
            // ...
        }
    }
    // ...
}
```

`RelAnalysis::is_sorted_on` is a new analysis predicate
needed for merge-join cost estimation. It propagates sort
order through `Sort`, `IndexScan`, `MergeJoin` (which
preserves order on the merge keys), and breaks on
`HashJoin`, `Aggregate`, etc.

### Phase 4: extraction enforces fully-physical plans

After saturation the extractor today picks the cheapest
plan tree. After this RFC, extraction must also enforce
that the *output* plan is fully physical: every `Join`
node in the extracted tree must be `HashJoin`, `MergeJoin`,
or `NestLoop`. Logical-only plans are pre-extraction
intermediate; the user always gets a physical plan.

```rust
// crates/ra-engine/src/extract/physical_extractor.rs
pub struct PhysicalExtractor<'a> {
    cost_fn: IntegratedCostFn,
    // Penalty applied to logical operators that have
    // physical alternatives in their e-class. Ensures the
    // extractor prefers physical even when the cost
    // estimate is close.
    logical_penalty: f64,
}

impl<'a> PhysicalExtractor<'a> {
    pub fn extract(&self, egraph: &EGraph<RelLang, RelAnalysis>,
                   root: Id) -> RecExpr<RelLang> {
        // Standard egg extractor with cost function that
        // adds logical_penalty to logical operators that
        // also have a physical sibling.
    }
}
```

### Phase 5: migrate `PhysicalChoices` sidecar

The sidecar from RFC 0087 doesn't go away — it becomes the
fallback for queries the e-graph extracts as logical
operators (no physical sibling fired during saturation).
This happens for:

- Operators we haven't added physical variants for yet
  (`Window`, `RecursiveCTE`, set operations).
- Saturation budget exhaustion: the e-graph ran out of
  iterations before the lowering rules fired.
- Disabled physical lowering (feature flag).

Plan-builder's existing `set_physical_choices` API stays.
The optimizer first tries to extract a fully-physical plan;
if the extracted plan still has logical `Join` nodes, those
nodes get their physical method from the sidecar. Same
mechanism, layered priority.

### Phase 6: supplied advice integration

`SET ra_planner.plan_advice = 'HASH_JOIN(b)'` continues to
work end-to-end. Internally:

- The advice biases the e-graph's lowering rules: when
  `HASH_JOIN(b)` is in effect, `join_to_hash_join` fires
  unconditionally on joins where `b` is the inner side, and
  `join_to_merge_join` / `join_to_nest_loop` are gated
  off for that join.
- The biased lowering plus the existing
  `Cost::DISABLE_PENALTY` for non-conforming plans
  guarantees the extractor picks `HashJoin`.

### Phase 7: feature gating and rollout

Default off behind `ra_planner.enable_physical_lowering`.
Initial rollout: opt-in only. Promote to default-on after
the test matrix confirms zero regressions on Ra's existing
benchmarks (TPC-H SF=0.01, JOB).

### Implementation phases (timeline estimate)

| Phase | Description | Estimate |
|-------|-------------|----------|
| 1 | `RelLang` physical variants + roundtrip | 2 weeks |
| 2 | Lowering rewrite rules (3 join + 2 agg) | 2 weeks |
| 3 | Per-physical cost estimates + sort-order analysis | 3 weeks |
| 4 | `PhysicalExtractor` with logical penalty | 1.5 weeks |
| 5 | Sidecar fallback layering | 1 week |
| 6 | Supplied-advice biasing | 1.5 weeks |
| 7 | Feature flag, GUC, rollout | 1 week |
| **Total** | | **12 weeks** |

Add 4 weeks for benchmark regressions, 2 weeks for
performance tuning of the sort-order analysis, 2 weeks for
RFC review: **20 weeks calendar**.

### Test plan

1. **Unit tests** — cost-comparison tests for each lowering
   pair (`HashJoin` vs `MergeJoin` vs `NestLoop` on small/
   medium/large inputs).
2. **E-graph saturation tests** — verify the lowering rules
   fire in expected scenarios; verify saturation terminates.
3. **Extraction tests** — every extracted plan is fully
   physical (no bare `Join` nodes); verify cost ordering.
4. **Sidecar fallback tests** — when physical lowering is
   disabled, behavior matches today exactly.
5. **Supplied-advice tests** — every existing plan-advice
   test from the current suite passes with physical
   lowering enabled.
6. **Differential tests** — compare extracted plans against
   the sidecar approach on TPC-H, JOB; both must produce
   correct results, plan shapes may differ.
7. **Benchmark suite** — measure planning time impact
   (expected: 1.5-3× slower than sidecar on simple queries
   due to larger e-graph; same or faster on analytical
   queries due to better plans). End-to-end query latency
   on TPC-DS where physical-aware ordering matters.

## Drawbacks

- **12+ weeks of engineering** for the core, plus rollout.
  Larger investment than any other plan-advice RFC.
- **E-graph blowup**. Adding 3 physical variants per
  logical join doubles or triples the e-class count. Egg's
  saturation cost is O(e-classes × rules × iterations);
  this directly slows planning. Ra's planning-time
  advantage shrinks.
- **Cost-model complexity**. Per-physical cost estimates
  require sort-order analysis, hash-build memory tracking,
  and per-method calibration constants. Each is its own
  small engineering project.
- **Test surface area**. Every existing test that pattern-
  matches `RelExpr::Join` needs to handle the physical
  variants. Estimated 50-100 test sites.
- **Backward compatibility risk**. Plans extracted under
  this RFC differ in shape from today's plans. Even when
  the result set is identical, downstream consumers
  (provenance trackers, plan caches, monitoring tools)
  see different EXPLAIN output and may regress.

## Rationale and alternatives

### Why this design?

The Cascades / Volcano pattern is the textbook approach to
physical-aware optimization. egg supports it naturally via
e-class equivalence with cost-based extraction. Adopting it
puts Ra on a well-understood foundation for analytical
workload optimization without inventing novel architecture.

### Alternative 1: keep the sidecar, add cardinality-aware ordering

Instead of e-graph physical lowering, augment the sidecar
to take join-ordering hints from a cardinality estimator.
Pros: minimal architectural change, ships in 4 weeks. Cons:
doesn't actually solve the workload demand — physical
method choice still happens after extraction. Punts the
problem.

### Alternative 2: separate physical optimization phase

Run e-graph saturation on logical operators only, extract
the best logical plan, then run a second pass that picks
physical methods (via dynamic programming over the
extracted tree). Pros: smaller e-graph; physical decisions
benefit from cost. Cons: two-pass approach can't compare
"this order with merge join" vs "that order with hash join"
because the second pass sees only one logical plan. Same
problem as the sidecar, slightly differently expressed.

### Alternative 3: hybrid — sidecar for OLTP, e-graph for OLAP

Detect query complexity in the speculative router (existing
`OptRoute` enum) and use the sidecar for `Skip` / `LeftDeep`
/ `EGraphLow` routes; use e-graph physical lowering for
`EGraphMedium` / `EGraphHigh`. Pros: keeps the OLTP
planning-time advantage. Cons: two code paths to maintain;
unclear how to make the routing decision without already
running half the optimization. Maybe the right answer
long-term but feels premature.

### Why not the alternatives?

The honest reason is workload-dependent: today's Ra
workload is OLTP and the sidecar wins. This RFC's design
becomes the right answer when analytical workloads
dominate. The alternative most likely to ship if we
*don't* do this RFC is alternative 3 (hybrid), since it
directly addresses the planning-time concern.

## Prior art

- **Cascades / Volcano framework**: Goetz Graefe's
  generation of optimizers (Microsoft SQL Server, Apache
  Calcite, CockroachDB, DuckDB). All use rule-based
  physical lowering with cost-based extraction. Ra's egg-
  based saturation is a natural fit.
- **DuckDB's optimizer**: pipeline of physical optimization
  passes (`Optimizer::Optimize` invokes
  `PhysicalPlanGenerator` after logical optimization). Two-
  pass approach (alternative 2 above); informs trade-offs.
- **Apache Calcite**: `RelOptPlanner` with `PhysicalRel`
  class hierarchy. Calcite's separation of logical and
  physical convention systems is more elaborate than this
  RFC proposes; we adopt the spirit (lowering rules) but
  not the full convention machinery.
- **Egg's own benchmarks** (Willsey et al., 2021):
  demonstrate that adding physical variants to e-graphs
  scales linearly with e-class count and rule count. Our
  estimate of 1.5-3× planning-time slowdown is consistent
  with their measurements.

## Unresolved questions

- **How many physical variants per logical operator?** This
  RFC proposes 3 for Join (Hash/Merge/NestLoop) and 2 for
  Aggregate (Hash/Sort). Should we add more (e.g.,
  Indexed-Nested-Loop as a NestLoop variant when the inner
  side has a usable index)? Defer to phase 2 review.
- **Aggregate physical lowering**: scope creep risk. Maybe
  ship Join lowering first, defer Aggregate to a follow-up
  RFC.
- **Sort-order analysis scope**: minimal version tracks
  per-e-class "sorted on column list" as a new
  `RelAnalysis` field. Full version tracks "sorted on any
  prefix of column list with directions" — more useful but
  more complex. Decide at design review.
- **Plan cache compatibility**: plans cached today have
  bare `RelExpr::Join` nodes. After this RFC plans have
  physical variants. Cache eviction strategy needed.

## Future work

- Aggregate physical lowering (HashAggregate /
  SortAggregate / StreamingAggregate).
- IndexNestedLoop physical variant (NestLoop with inner-side
  index probe).
- Materialize / Memoize as physical operators in the e-graph.
- Cross-RFC: integrate with RFC 0086 (Ballista plan
  emission) so distributed plans can also be cost-
  selected via e-graph extraction.
- Physical-method-aware join enumeration heuristics in the
  speculative router so OLAP queries route directly to
  physical-lowering-enabled paths.

## References

- RFC 0087 — physical-operator selection (the sidecar this
  RFC eventually subsumes for the analytical workload
  segment).
- RFC 0088 — FDW pushdown (orthogonal but produces
  another physical variant: `T_ForeignScan` for a join).
- `crates/ra-engine/src/egraph/mod.rs` — `RelLang`
  definition this RFC extends.
- `crates/ra-engine/src/cost.rs::IntegratedCostFn` — cost
  function this RFC extends with per-physical estimates.
- `crates/ra-engine/src/plan_advice_physical.rs` — sidecar
  this RFC layers as fallback.
- Goetz Graefe, "The Cascades Framework for Query
  Optimization" (1995) —
  https://www.cse.iitb.ac.in/infolab/Data/Courses/CS632/Papers/Cascades-graefe.pdf
- Willsey et al., "egg: Fast and Extensible Equality
  Saturation" (POPL 2021) —
  https://arxiv.org/abs/2004.03082
