# Rule: Three-Phase Distinct Aggregation

**Category:** distributed/partial-aggregation
**File:** `rules/distributed/partial-aggregation/three-phase-distinct-agg.rra`

## Metadata

- **ID:** `three-phase-distinct-agg`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, greenplum
- **Tags:** distributed, aggregation, distinct, three-phase, count-distinct
- **Authors:** "RA Contributors"


# Three-Phase Distinct Aggregation

## Description

Handles COUNT(DISTINCT x) and other distinct aggregations in a
distributed setting using three phases: (1) local pre-aggregation to
deduplicate on each node, (2) shuffle by the distinct key to co-locate
duplicates, (3) final aggregation with exact count.

This avoids shuffling all raw rows when many duplicates exist within each
node's partition.

**When to apply**: A query uses COUNT(DISTINCT x) or similar distinct
aggregation, and the distinct column has significant duplication within
each node.

**Why it works**: Phase 1 eliminates local duplicates, reducing the
data that enters the shuffle. Phase 2 ensures all remaining copies of
each value are on the same node. Phase 3 counts each unique value
exactly once.

## Relational Algebra

```algebra
-- Naive (single phase)
gamma[g, COUNT(DISTINCT d)](R)

-- Three-phase
gamma[g, COUNT(*)].final(
    Exchange[hash(g)](
        gamma[g, d, COUNT(*)].intermediate(
            Exchange[hash(d)](
                gamma[g, d].local_dedup(R)
            )
        )
    )
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("three-phase-count-distinct";
    "(aggregate ?group (count_distinct ?col) ?child)" =>
    "(aggregate_final ?group (count ?col)
        (exchange hash_partition
            (aggregate_intermediate ?group (count ?col)
                (exchange hash_partition
                    (aggregate_dedup ?group ?col ?child)
                    ?col))
            ?group))"
),
```

## Preconditions

```rust
fn applicable(
    agg_fn: &AggFunction,
    col: &Column,
    input: &RelNode,
) -> bool {
    // Must be a distinct aggregation
    matches!(agg_fn, AggFunction::CountDistinct)
    // Beneficial when there are many duplicates per node
    && input.estimated_cardinality()
        > col.estimated_distinct_values() * 5
}
```

**Restrictions:**
- Only applies to distinct aggregations
- Phase 1 deduplication is only beneficial when there are local
  duplicates; if data is already unique per node, it adds overhead
- Multiple distinct aggregations on different columns require
  separate plan branches (or expand-based rewrite)
- When approximate results are acceptable, HyperLogLog (HLL) is faster

## Cost Model

```rust
fn three_phase_cost(
    input_rows: f64,
    distinct_values: f64,
    groups: f64,
    num_nodes: u32,
    row_bytes: f64,
    network_bandwidth: f64,
) -> f64 {
    let shuffle_fraction =
        (num_nodes - 1) as f64 / num_nodes as f64;
    // Phase 1 output: at most distinct_values per node
    let dedup_rows = distinct_values.min(
        input_rows / num_nodes as f64
    ) * num_nodes as f64;
    // Phase 2 shuffle
    let shuffle_1 = dedup_rows * row_bytes * shuffle_fraction
        / network_bandwidth;
    // Phase 3 shuffle (group keys only)
    let shuffle_2 = groups * num_nodes as f64 * 16.0
        * shuffle_fraction / network_bandwidth;
    shuffle_1 + shuffle_2
}
```

**Typical benefit**: For 1B rows with 1M distinct values, phase 1
reduces shuffle from 1B to ~1M * N rows (99%+ reduction per node).

## Test Cases

```sql
-- Positive: high duplication ratio
SELECT region, COUNT(DISTINCT customer_id)
FROM orders  -- 1B rows, 5M distinct customers
GROUP BY region;

-- Plan:
-- AggregateFinal(region, COUNT(customer_id))
--   Exchange[hash(region)]
--     AggregateIntermediate(region, COUNT(customer_id))
--       Exchange[hash(customer_id)]
--         AggregateDedup(region, customer_id)
--           Scan(orders)
```

```sql
-- Positive: multiple groups reduce shuffle further
SELECT year, month, COUNT(DISTINCT user_id)
FROM events
GROUP BY year, month;

-- Three phases with dedup per node reducing from billions to millions
```

```sql
-- Negative: column already unique, dedup adds overhead
SELECT COUNT(DISTINCT order_id) FROM orders;
-- order_id is primary key -> no duplicates to eliminate locally
-- Two-phase or single-phase is sufficient
```

## References

Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planDistinctAggregation()
Spark SQL: sql/catalyst/src/main/scala/org/apache/spark/sql/catalyst/optimizer/RewriteDistinctAggregates.scala
Greenplum: src/backend/optimizer/plan/planagg.c - plan_distinct_agg()
