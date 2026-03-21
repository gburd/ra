# Rule: Parallel Aggregation

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/parallel-aggregation.rra`

## Metadata

- **ID:** `parallel-aggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, clickhouse
- **Tags:** parallelization, aggregation
- **Authors:** "RA Contributors"


# Parallel Aggregation

## Description

Performs local aggregation in parallel workers, then combines results.

**When to apply**: Large GROUP BY aggregations.

**Why it works**: Two-phase parallel aggregation; local + global combine.

## Relational Algebra

```algebra
aggregate[group, agg](R)
  -> final_aggregate(
       parallel_gather(
         partial_aggregate(R_partition_i)
       ))
```

## Implementation

```rust
rw!("parallelize-aggregation";
    "(aggregate ?groups ?aggs ?input)" =>
    "(final-aggregate ?groups ?aggs
       (parallel-gather
         (map-workers ?w
           (partial-aggregate ?groups ?aggs
             (partition-data ?input ?w)))))"
),
```

## Cost Model

```rust
fn cost(input: u64, workers: usize) -> f64 {
    (input as f64 / workers as f64) * 1.5 + 100.0 // Combine overhead
}
```

**Typical benefit**: 50-85% with decomposable aggregates

## References

- PostgreSQL: Parallel aggregate
- ClickHouse: Distributed aggregation
