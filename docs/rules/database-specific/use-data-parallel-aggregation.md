# Rule: Use Data-Parallel Aggregation

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/use-data-parallel-aggregation.rra`

## Metadata

- **ID:** `clickhouse-use-data-parallel-aggregation`
- **Version:** 1.0.0
- **Databases:** clickhouse
- **Tags:** database-specific, clickhouse, aggregation, parallel, optimization
- **Authors:** "RA Contributors"


# Use Data-Parallel Aggregation

## Description

Converts single-threaded aggregation to data-parallel aggregation by partitioning data across multiple threads. Each thread maintains its own hash table, which are merged at the end.

**When to apply**: Large aggregations that can benefit from parallelism.

**Why it works**: Parallel aggregation on P threads reduces time from O(n) to O(n/P) for the aggregation phase. Modern servers have many cores; using them improves throughput.

**Database version**: ClickHouse v1.0+ (core feature)

## Relational Algebra

```algebra
GroupBy[keys, aggs](R)
  -> MergeGroupBy[keys, aggs](
       $\forall$t $\in$ threads: GroupBy[keys, aggs](Partition_t(R))
     )
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("clickhouse-use-data-parallel-aggregation";
    "(group_by ?keys ?aggs ?input)" =>
    "(merge_aggregation
        (parallel_group_by ?keys ?aggs ?input))"
    if is_database("clickhouse")
    if is_large_aggregation("?input")
),
```

**Typical benefit**: 30-70% with many cores and large datasets

## References

**Source code:**
- ClickHouse: `src/Processors/QueryPlan/Optimizations/useDataParallelAggregation.cpp`
  - Commit: 35f2d31186cca2f8c50f7ba4bd93817da490da85
