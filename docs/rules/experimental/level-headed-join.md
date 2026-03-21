# Rule: LevelHeaded Join Algorithm

**Category:** experimental/wcoj
**File:** `rules/experimental/wcoj/level-headed-join.rra`

## Metadata

- **ID:** `level-headed-join`
- **Version:** "1.0.0"
- **Databases:** duckdb
- **Tags:** wcoj, level-headed, aggregation, factorized
- **Authors:** "Aberger et al. 2018", "RA Contributors"


# LevelHeaded Join Algorithm

## Description

LevelHeaded extends worst-case optimal joins to handle aggregations and
GROUP BY operations within the WCOJ framework. Instead of first computing
the full join result and then aggregating, LevelHeaded interleaves
aggregation with the variable-at-a-time enumeration, avoiding materialization
of the full join output when only aggregates are needed.

**When to apply**: Queries combining multi-way joins with aggregations
(COUNT, SUM, etc.) where the join output is much larger than the aggregated
result. Especially effective for graph analytics (triangle counting, path
counting).

**Why it works**: Traditional plans compute Join then Aggregate. LevelHeaded
pushes aggregation into the WCOJ loop: at each level (variable), it can
aggregate partial results rather than materializing all tuples. This reduces
memory from O(output) to O(aggregate groups).

## Relational Algebra

```algebra
aggregate[COUNT(*)](
  join[R.a=S.a, S.b=T.b, R.c=T.c](R, S, T)
)
  -> levelheaded_join(
       variable_order: [a, b, c],
       relations: {R(a,c), S(a,b), T(b,c)},
       aggregate: COUNT,
       annotation_semiring: (int, +, *)
     )
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("levelheaded-join";
    "(aggregate ?agg_fn
       (join ?pred1 (join ?pred2 ?r1 ?r2) ?r3))" =>
    "(levelheaded_join
       (variable_order (compute_order ?r1 ?r2 ?r3))
       (relations ?r1 ?r2 ?r3)
       (predicates (merge_preds ?pred1 ?pred2))
       (aggregate ?agg_fn)
       (semiring (infer_semiring ?agg_fn)))"
    if is_aggregation_over_multiway_join()
    if is_decomposable_aggregate("?agg_fn")
),
```

## Preconditions

```rust
fn applicable(
    query: &AggregateJoinQuery,
) -> bool {
    // Must be an aggregation over a multi-way join
    if query.join_relations().len() < 3 {
        return false;
    }

    // Aggregate must be decomposable (expressible as semiring)
    if !is_decomposable_aggregate(&query.aggregate) {
        return false;
    }

    // Join output must be significantly larger than aggregate result
    let join_output_est = estimate_join_output(query);
    let agg_output_est = estimate_aggregate_output(query);

    join_output_est > agg_output_est * 10.0
}

fn is_decomposable_aggregate(agg: &AggregateFunction) -> bool {
    matches!(
        agg,
        AggregateFunction::Count
        | AggregateFunction::Sum
        | AggregateFunction::Min
        | AggregateFunction::Max
    )
}
```

**Restrictions:**
- Aggregate function must be decomposable into a semiring (COUNT, SUM, MIN, MAX)
- AVG requires decomposition into SUM/COUNT
- MEDIAN and other order-statistics not directly supported
- All join predicates must be equi-joins

## Cost Model

```rust
fn estimated_benefit(
    query: &AggregateJoinQuery,
) -> f64 {
    let join_output = estimate_join_output(query);
    let agg_groups = estimate_aggregate_groups(query);

    // Traditional: materialize full join, then aggregate
    let traditional_cost = join_output * 2.0 + agg_groups;

    // LevelHeaded: aggregate during enumeration
    let agm_bound = compute_agm_bound(query);
    let semiring_overhead = 1.3; // annotation tracking
    let levelheaded_cost = agm_bound * semiring_overhead;

    if traditional_cost > levelheaded_cost {
        (traditional_cost - levelheaded_cost) / traditional_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 10x-100x for aggregate queries over cyclic joins
(e.g., triangle counting is O(N^1.5) instead of materializing O(N^2)
triangles then counting).

## Test Cases

### Positive: Triangle counting

```sql
SELECT COUNT(*)
FROM edges e1, edges e2, edges e3
WHERE e1.dst = e2.src AND e2.dst = e3.src AND e3.dst = e1.src;

-- Traditional: materialize all triangles, count them
-- LevelHeaded: count during enumeration, never materialize
-- Memory: O(1) vs O(triangles)
```

### Positive: Weighted path sum

```sql
SELECT SUM(e1.weight * e2.weight * e3.weight)
FROM edges e1, edges e2, edges e3
WHERE e1.dst = e2.src AND e2.dst = e3.src AND e3.dst = e1.src;

-- Semiring: (float, +, *) for weighted counting
-- LevelHeaded accumulates partial sums at each level
```

### Negative: Query needing full join output

```sql
SELECT e1.src, e2.src, e3.src
FROM edges e1, edges e2, edges e3
WHERE e1.dst = e2.src AND e2.dst = e3.src AND e3.dst = e1.src;

-- No aggregation: must materialize all tuples
-- LevelHeaded offers no advantage over plain WCOJ
```

## References

**Academic papers:**
- Aberger et al., "LevelHeaded: A Unified Engine for Business Intelligence and Linear Algebra", SIGMOD 2018
- Aberger et al., "EmptyHeaded: A Relational Engine for Graph Processing", SIGMOD 2017
- Abo Khamis et al., "FAQ: Questions Asked Frequently", PODS 2016

**Key insights:**
- Annotation semirings generalize COUNT (+,*), SUM (+,*), MIN (min,+), MAX (max,+)
- FAQ (Functional Aggregate Queries) framework unifies joins and aggregations
- LevelHeaded achieves both worst-case optimal join time and optimal aggregate time
- Particularly effective for graph analytics where output >> aggregate
