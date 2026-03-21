# Rule: Skew-Aware Join

**Category:** distributed/distributed-joins
**File:** `rules/distributed/distributed-joins/skew-aware-join.rra`

## Metadata

- **ID:** `skew-aware-join`
- **Version:** "1.0.0"
- **Databases:** spark, presto, trino, greenplum
- **Tags:** distributed, join, skew, load-balancing, adaptive
- **Authors:** "RA Contributors"


# Skew-Aware Join

## Description

Handles data skew in distributed joins by detecting hot keys (keys with
disproportionate frequency) and treating them specially. For hot keys,
the build side is replicated (broadcast) while the rest uses a normal
shuffle join. This prevents a single node from becoming a bottleneck.

**When to apply**: Join key distribution is skewed, with some key values
having orders of magnitude more rows than the median. Statistics or
runtime sampling indicate that a few partitions would receive a
disproportionate amount of data.

**Why it works**: In a naive shuffle join with skewed keys, the node
receiving the hot partition does most of the work while other nodes sit
idle. By broadcasting the build side for hot keys and shuffling the rest,
work is distributed evenly.

## Relational Algebra

```algebra
-- Split probe into hot and cold partitions
Join[c](R, S)
  -> Union(
       Join[c](sigma[k IN hot_keys](R), Exchange[broadcast](S)),
       Join[c](Exchange[hash(k)](sigma[k NOT IN hot_keys](R)),
               Exchange[hash(k)](S))
     )
  where hot_keys = {k : freq(k, R) > skew_threshold}
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("skew-aware-join";
    "(join ?type ?cond
        (exchange hash_partition ?left ?lkeys)
        (exchange hash_partition ?right ?rkeys))" =>
    "(union
        (join ?type ?cond
            (filter (in ?lkeys ?hot_keys) ?left)
            (exchange broadcast ?right))
        (join ?type ?cond
            (exchange hash_partition
                (filter (not_in ?lkeys ?hot_keys) ?left) ?lkeys)
            (exchange hash_partition ?right ?rkeys)))"
    if has_skew("?left", "?lkeys", SKEW_THRESHOLD)
),
```

## Preconditions

```rust
fn applicable(
    left: &RelNode,
    join_keys: &[Column],
    skew_threshold: f64,
) -> bool {
    let stats = left.column_statistics(join_keys);
    // Check if any key value has freq > threshold * avg_freq
    let avg_freq = stats.total_rows / stats.distinct_values;
    stats.max_frequency > skew_threshold * avg_freq
}
```

**Restrictions:**
- Requires statistics on key frequencies (histograms or samples)
- Hot key detection may use runtime sampling (adaptive) or static
  statistics (compile-time)
- Adds plan complexity: two join branches and a union
- In Spark, AQE (Adaptive Query Execution) handles skew at runtime by
  splitting large shuffle partitions

## Cost Model

```rust
fn skew_join_cost(
    total_rows: f64,
    hot_rows: f64,
    cold_rows: f64,
    build_bytes: f64,
    num_nodes: u32,
    network_bandwidth: f64,
) -> f64 {
    // Hot path: broadcast build side
    let hot_cost =
        build_bytes * num_nodes as f64 / network_bandwidth;
    // Cold path: normal shuffle
    let cold_fraction = (num_nodes - 1) as f64 / num_nodes as f64;
    let cold_cost =
        cold_rows * 100.0 * cold_fraction / network_bandwidth;
    hot_cost + cold_cost
}
```

**Typical benefit**: Prevents worst-case scenarios where a single
skewed partition causes 10-100x slowdown versus the median partition.

## Test Cases

```sql
-- Positive: popular product skew
SELECT o.*, p.name
FROM order_items o      -- 500M rows
JOIN products p ON o.product_id = p.id;
-- product_id=42 ("iPhone") has 50M rows (10%), others average 500

-- Plan (skew-aware):
-- Union
--   BroadcastHashJoin(o.product_id = p.id)
--     Filter(product_id = 42)       -- hot partition
--       Scan(order_items)
--     Exchange[broadcast](Scan(products))
--   ShuffleHashJoin(o.product_id = p.id)
--     Exchange[hash(product_id)]
--       Filter(product_id != 42)    -- cold partitions
--         Scan(order_items)
--     Exchange[hash(id)]
--       Scan(products)
```

```sql
-- Positive: Spark AQE runtime skew detection
-- spark.sql.adaptive.skewJoin.enabled = true
-- spark.sql.adaptive.skewJoin.skewedPartitionThresholdInBytes = 256MB
SELECT * FROM users u JOIN events e ON u.id = e.user_id;
-- At runtime, AQE detects user_id=0 (anonymous) is 40% of events
-- and splits that partition across multiple tasks
```

```sql
-- Negative: uniform distribution, no skew handling needed
SELECT * FROM orders o JOIN items i ON o.id = i.order_id;
-- 1:N relationship with uniform distribution -> normal shuffle
```

## References

Spark SQL: AQE skew join optimization - sql/core/src/main/scala/org/apache/spark/sql/execution/adaptive/OptimizeSkewedJoin.scala
Presto/Trino: Skew handling via scaled writers
DeWitt et al., "Practical Skew Handling in Parallel Joins" (VLDB 1992)
Xu et al., "Handling Data Skew in MapReduce Cluster" (FCS 2014)
