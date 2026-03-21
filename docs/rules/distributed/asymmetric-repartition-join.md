# Rule: Asymmetric Repartition Join

**Category:** distributed/distributed-joins
**File:** `rules/distributed/distributed-joins/asymmetric-repartition-join.rra`

## Metadata

- **ID:** `asymmetric-repartition-join`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, greenplum, cockroachdb
- **Tags:** distributed, join, repartition, asymmetric, one-sided
- **Authors:** "RA Contributors"


# Asymmetric Repartition Join

## Description

When one side of a join is already hash-partitioned on the join key but
the other is not, only the non-partitioned side needs to be repartitioned.
This avoids shuffling both sides when only one needs to move.

**When to apply**: One join input is already hash-partitioned on its join
key, and the other input's partition key does not match the join key.

**Why it works**: Repartitioning one side instead of two saves half the
network transfer. The already-partitioned side stays in place.

## Relational Algebra

```algebra
Join[c](R hash(join_key), S hash(other_key))
  -> Join[c](R, Exchange[hash(join_key_S)](S))
  where R is already partitioned on join_key
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("asymmetric-repartition-left-stays";
    "(join ?type ?cond
        ?left
        (exchange hash_partition ?right ?rkeys))" =>
    "(join ?type ?cond
        ?left
        (exchange hash_partition ?right ?join_keys_right))"
    if left_partitioned_on_join_key("?left", "?cond")
),

rw!("asymmetric-repartition-right-stays";
    "(join ?type ?cond
        (exchange hash_partition ?left ?lkeys)
        ?right)" =>
    "(join ?type ?cond
        (exchange hash_partition ?left ?join_keys_left)
        ?right)"
    if right_partitioned_on_join_key("?right", "?cond")
),
```

## Preconditions

```rust
fn applicable(
    partitioned_side: &RelNode,
    join_key: &[Column],
) -> bool {
    partitioned_side.distribution()
        .is_hash_partitioned_on(join_key)
}
```

**Restrictions:**
- The partitioned side's distribution must exactly match the join key
  (including hash function)
- The non-partitioned side must be repartitioned to the same number
  of buckets as the partitioned side
- If the non-partitioned side is much smaller, broadcast may still be
  cheaper than asymmetric repartition

## Cost Model

```rust
fn asymmetric_vs_symmetric_cost(
    left_bytes: f64,
    right_bytes: f64,
    left_on_join_key: bool,
    num_nodes: u32,
    network_bandwidth: f64,
) -> (f64, f64) {
    let shuffle_fraction =
        (num_nodes - 1) as f64 / num_nodes as f64;
    let symmetric_cost =
        (left_bytes + right_bytes) * shuffle_fraction
        / network_bandwidth;
    let asymmetric_cost = if left_on_join_key {
        right_bytes * shuffle_fraction / network_bandwidth
    } else {
        left_bytes * shuffle_fraction / network_bandwidth
    };
    (asymmetric_cost, symmetric_cost)
}
```

**Typical benefit**: Saves 50% of shuffle cost when one side is already
correctly partitioned.

## Test Cases

```sql
-- Positive: left side already partitioned on join key
-- orders: DISTRIBUTED BY HASH(customer_id)
-- payments: DISTRIBUTED BY HASH(payment_id)
SELECT o.*, p.amount
FROM orders o JOIN payments p ON o.customer_id = p.customer_id;

-- Plan: only repartition payments
-- HashJoin(o.customer_id = p.customer_id)
--   Scan(orders)  -- already on customer_id
--   Exchange[hash(customer_id)](Scan(payments))
```

```sql
-- Positive: after a shuffle, intermediate result is partitioned
-- Stage 1 produced result partitioned on customer_id
-- Stage 2 joins with accounts on customer_id
-- No additional shuffle needed for the intermediate result
```

```sql
-- Negative: neither side partitioned on join key
-- orders: DISTRIBUTED BY HASH(order_date)
-- customers: DISTRIBUTED BY HASH(region)
-- Join on orders.customer_id = customers.id
-- Both sides need repartition -> symmetric shuffle
```

## References

Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java
Spark SQL: sql/catalyst/src/main/scala/org/apache/spark/sql/execution/exchange/EnsureRequirements.scala
Greenplum: src/backend/cdb/cdbllize.c - one-sided motion
