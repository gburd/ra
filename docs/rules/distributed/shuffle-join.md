# Rule: Shuffle (Repartition) Join

**Category:** distributed/distributed-joins
**File:** `rules/distributed/distributed-joins/shuffle-join.rra`

## Metadata

- **ID:** `shuffle-join`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum
- **Tags:** distributed, join, shuffle, repartition, hash
- **Authors:** "RA Contributors"


# Shuffle (Repartition) Join

## Description

Both sides of a join are repartitioned (shuffled) on the join key so that
matching rows end up on the same node. This is the default distributed
join strategy when neither side is small enough for broadcast and the
tables are not co-partitioned on the join key.

**When to apply**: Both join inputs are large and not co-partitioned on
the join key.

**Why it works**: After repartitioning both sides on the join key, rows
with matching keys are guaranteed to be on the same node. Each node then
performs a local join on its partition, and the union of all partitions
produces the correct result.

## Relational Algebra

```algebra
Join[c](R, S) -> Join[c](
    Exchange[hash(join_key_R)](R),
    Exchange[hash(join_key_S)](S)
)
where R and S are not co-partitioned on join keys
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("shuffle-join";
    "(join ?type ?cond ?left ?right)" =>
    "(join ?type ?cond
        (exchange hash_partition ?left ?join_keys_left)
        (exchange hash_partition ?right ?join_keys_right))"
    if not_copartitioned("?left", "?right", "?cond")
    if not_broadcastable("?left", "?right")
),
```

## Preconditions

```rust
fn applicable(
    left: &RelNode,
    right: &RelNode,
    join_keys: &JoinKeys,
    broadcast_threshold: u64,
) -> bool {
    // Not already co-partitioned
    !left.distribution().is_hash_partitioned_on(&join_keys.left)
        || !right.distribution().is_hash_partitioned_on(&join_keys.right)
    // Neither side is small enough to broadcast
    && left.estimated_size_bytes() > broadcast_threshold
    && right.estimated_size_bytes() > broadcast_threshold
}
```

**Restrictions:**
- Both sides must be repartitioned on compatible join key types
- Hash function must be consistent across all nodes
- Does not apply to non-equi joins (theta joins) because hash
  partitioning cannot guarantee co-location for inequality predicates
- Skewed join keys cause uneven data distribution (see skew-aware join)

## Cost Model

```rust
fn shuffle_join_cost(
    left_rows: f64,
    left_bytes: f64,
    right_rows: f64,
    right_bytes: f64,
    num_nodes: u32,
    network_bandwidth: f64,
) -> f64 {
    let shuffle_fraction = (num_nodes - 1) as f64 / num_nodes as f64;
    let left_shuffle = left_bytes * shuffle_fraction;
    let right_shuffle = right_bytes * shuffle_fraction;
    let network_cost =
        (left_shuffle + right_shuffle) / network_bandwidth;
    let local_join_cost =
        (left_rows + right_rows) / num_nodes as f64;
    network_cost + local_join_cost
}
```

**Typical benefit**: Enables parallel join execution across all nodes.
Each node handles 1/N of the data.

## Test Cases

```sql
-- Positive: large-large join requires shuffle
SELECT o.*, i.product_id, i.quantity
FROM orders o         -- 100M rows, partitioned on order_date
JOIN order_items i    -- 500M rows, partitioned on item_id
  ON o.id = i.order_id;

-- Plan:
-- HashJoin(o.id = i.order_id)
--   Exchange[hash(id)](Scan(orders))
--   Exchange[hash(order_id)](Scan(order_items))
```

```sql
-- Positive: only one side needs repartition
SELECT o.*, c.name
FROM orders o         -- partitioned on customer_id
JOIN customers c      -- partitioned on id
  ON o.customer_id = c.id;
-- customers already on id, only orders needs repartition

-- Plan:
-- HashJoin(o.customer_id = c.id)
--   Exchange[hash(customer_id)](Scan(orders))
--   Scan(customers)  -- already on id
```

```sql
-- Negative: tables co-partitioned on join key
-- orders and order_items both partitioned on order_id
SELECT * FROM orders o
JOIN order_items i ON o.id = i.order_id;
-- No shuffle needed -> co-located join
```

## References

Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java
Spark SQL: sql/core/src/main/scala/org/apache/spark/sql/execution/joins/ShuffledHashJoinExec.scala
CockroachDB: pkg/sql/opt/xform/join_funcs.go
Greenplum: src/backend/cdb/cdbllize.c - make_redistribute_motion()
DeWitt et al., "Implementation Techniques for Main Memory Database Systems" (SIGMOD 1984)
