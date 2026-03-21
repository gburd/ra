# Rule: Two-Phase (Partial/Final) Aggregation

**Category:** physical/aggregation
**File:** `rules/physical/aggregation/two-phase-aggregation.rra`

## Metadata

- **ID:** `two-phase-aggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, spark, presto, clickhouse
- **Tags:** physical, aggregation, partial, two-phase, parallel, distributed
- **Authors:** "Graefe, Goetz", "Selinger et al."


# Two-Phase (Partial/Final) Aggregation

## Description

Splits an aggregation into a partial (local) phase and a final (global)
phase. The partial phase computes partial aggregates on each partition
or thread, producing a smaller intermediate result. The final phase
merges partial results into the complete answer. This enables parallel
execution and reduces data movement in distributed systems.

**When to apply**: Aggregations with decomposable functions (SUM, COUNT,
MIN, MAX, AVG via SUM+COUNT) on partitioned or parallelized data.

## Relational Algebra

```algebra
-- Before: single-phase aggregation
gamma[dept; SUM(salary)](employees)

-- After: two-phase
gamma_final[dept; SUM(partial_sum)](
    gamma_partial[dept; SUM(salary) AS partial_sum](employees)
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("two-phase-agg";
    "(aggregate ?groups ?agg_fn ?input)" =>
    "(final-aggregate ?groups ?agg_fn
        (partial-aggregate ?groups ?agg_fn ?input))"
    if is_decomposable("?agg_fn")
    if input_is_partitioned_or_parallel("?input")
),
```

## Preconditions

```rust
fn applicable(agg: &Aggregate) -> bool {
    agg.function().is_decomposable()
        && (agg.input().is_partitioned()
            || agg.input().parallelism() > 1)
        // Must have enough groups to benefit
        && agg.estimated_group_count() > 1
}
```

**Restrictions:**
- DISTINCT aggregates require extra coordination
- ORDER-dependent aggregates (ARRAY_AGG, STRING_AGG) not decomposable
- AVG requires decomposition: SUM/COUNT in partial, division in final

## Cost Model

```rust
fn estimated_benefit(
    input_rows: f64,
    group_count: f64,
    parallelism: usize,
) -> f64 {
    let single_phase = input_rows;
    let partial_cost = input_rows / parallelism as f64;
    let merge_cost = group_count * parallelism as f64;
    single_phase - (partial_cost + merge_cost)
}
```

**Typical benefit**: 20-80% with sufficient parallelism.

## Test Cases

```sql
-- Positive: aggregation on parallel scan
SELECT dept, SUM(salary), COUNT(*) FROM employees GROUP BY dept;
-- Partial: per-thread SUM + COUNT; Final: merge

-- Negative: non-decomposable aggregate
SELECT dept, PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY salary)
FROM employees GROUP BY dept;
```

## References

- Graefe, G. "Query Evaluation Techniques" (ACM Computing Surveys 1993)
- Spark: Partial/Final Aggregation in Catalyst
