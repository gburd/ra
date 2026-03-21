# Rule: WCOJ to Binary Join Fallback

**Category:** experimental/wcoj
**File:** `rules/experimental/wcoj/wcoj-to-binary-fallback.rra`

## Metadata

- **ID:** `wcoj-to-binary-fallback`
- **Version:** "1.0.0"
- **Databases:** duckdb, postgresql
- **Tags:** wcoj, hybrid, binary-join, fallback, adaptive
- **Authors:** "Freitag et al. 2020", "RA Contributors"


# WCOJ to Binary Join Fallback

## Description

Implements a hybrid strategy that dynamically chooses between worst-case
optimal join (WCOJ) and traditional binary join plans based on runtime
cardinality observations. The optimizer generates both plans and uses
a switching predicate: if the first few tuples from WCOJ show that
intermediate sizes are small (acyclic-like behavior), it falls back
to the cheaper binary plan. Conversely, if binary join intermediates
explode, it switches to WCOJ.

**When to apply**: Queries where it is unclear at optimization time
whether WCOJ or binary joins will be faster. This is common for
queries on graphs with unknown degree distributions or skewed data.

**Why it works**: WCOJ has lower asymptotic worst-case complexity but
higher constant factors (intersection overhead). For many real-world
instances, binary plans with good cardinality estimates outperform WCOJ.
The hybrid approach gets the best of both worlds by monitoring runtime
behavior and switching.

## Relational Algebra

```algebra
join[preds](R, S, T)
  -> hybrid_join(
       wcoj_plan: generic_join(R, S, T),
       binary_plan: (R hash_join S) hash_join T,
       switch_predicate: runtime_cardinality_check,
       sampling_tuples: 1000
     )
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("wcoj-binary-hybrid";
    "(join ?pred1 (join ?pred2 ?r1 ?r2) ?r3)" =>
    "(hybrid_join
       (wcoj_plan (generic_join ?r1 ?r2 ?r3
                   (merge_preds ?pred1 ?pred2)))
       (binary_plan (hash_join ?pred1
                     (hash_join ?pred2 ?r1 ?r2) ?r3))
       (switch_threshold 10)
       (sample_size 1000))"
    if is_uncertain_benefit()
    if relation_count_ge_3("?r1", "?r2", "?r3")
),
```

## Preconditions

```rust
fn applicable(
    relations: &[RelExpr],
    predicates: &[JoinPredicate],
) -> bool {
    // At least 3 relations
    if relations.len() < 3 {
        return false;
    }

    // Cardinality estimates are uncertain
    let agm_bound = compute_agm_bound(relations, predicates);
    let binary_est = estimate_binary_join_cost(
        relations, predicates,
    );

    // Both plans are within 10x of each other
    let ratio = if agm_bound > binary_est {
        agm_bound / binary_est
    } else {
        binary_est / agm_bound
    };

    ratio < 10.0
}
```

**Restrictions:**
- Both plans must be generated (double optimization cost)
- Switching has overhead (partial results from abandoned plan are discarded)
- Sample size must be large enough for reliable estimate
- Not beneficial when one plan clearly dominates

## Cost Model

```rust
fn estimated_benefit(
    relations: &[Statistics],
    predicates: &[JoinPredicate],
) -> f64 {
    let agm_cost = compute_agm_bound(relations, predicates);
    let binary_cost = estimate_binary_join_cost(
        relations, predicates,
    );

    // Hybrid cost: min(WCOJ, binary) + switching overhead
    let switch_overhead = 1000.0 * 2.0; // sample tuples * factor
    let hybrid_cost = agm_cost.min(binary_cost) + switch_overhead;

    let best_static = agm_cost.min(binary_cost);
    let worst_static = agm_cost.max(binary_cost);

    // Benefit is avoiding the worst-case static choice
    if worst_static > hybrid_cost {
        (worst_static - hybrid_cost) / worst_static
    } else {
        0.0
    }
}
```

**Typical benefit**: 2x-5x improvement over wrong static choice.
Insurance policy against cardinality estimation errors.

## Test Cases

### Positive: Uncertain graph density

```sql
SELECT e1.v1, e2.v1, e3.v1
FROM edges e1, edges e2, edges e3
WHERE e1.v2 = e2.v1 AND e2.v2 = e3.v1 AND e3.v2 = e1.v1;

-- Dense graph: WCOJ better (many triangles, high intermediate)
-- Sparse graph: binary joins better (few intermediates)
-- Hybrid: sample 1000 tuples, measure intermediate blowup, choose
```

### Negative: Known dense graph

```sql
-- Social network with known high clustering coefficient
SELECT COUNT(*)
FROM friends f1, friends f2, friends f3
WHERE f1.b = f2.a AND f2.b = f3.a AND f3.b = f1.a;

-- Known to be dense: WCOJ is clearly better, no hybrid needed
```

## References

**Academic papers:**
- Freitag et al., "Adopting Worst-Case Optimal Joins in Relational Database Systems", VLDB 2020
- Mhedhbi, Salihoglu, "Optimizing Subgraph Queries by Combining Binary and Worst-Case Optimal Joins", VLDB 2019

**Key insights:**
- Runtime switching avoids committing to wrong plan at optimization time
- Sample-based switching converges in O(1000) tuples
- Complementary to adaptive query processing (EDDY)
- DuckDB explored hybrid WCOJ integration (Freitag et al.)
