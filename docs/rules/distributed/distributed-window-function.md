# Rule: Distributed Window Function Execution

**Category:** distributed/distributed-sort
**File:** `rules/distributed/distributed-sort/distributed-window-function.rra`

## Metadata

- **ID:** `distributed-window-function`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum
- **Tags:** distributed, window, partition, sort, ranking
- **Authors:** "RA Contributors"


# Distributed Window Function Execution

## Description

Distributes window function computation by repartitioning data on the
PARTITION BY columns. Each node computes the window function independently
on its local partition(s), since window functions are independent across
partitions. If there is no PARTITION BY, data must be gathered to a
single node.

**When to apply**: A query contains window functions (ROW_NUMBER, RANK,
SUM OVER, etc.) and data is distributed across nodes.

**Why it works**: Window functions are computed independently within each
partition. If data is hash-partitioned on the PARTITION BY key, each
node has complete partitions and can compute window values locally
without any cross-node communication.

## Relational Algebra

```algebra
-- With PARTITION BY:
Window[f OVER (PARTITION BY k ORDER BY s)](R)
  -> Window_local[f OVER (ORDER BY s)](
       Exchange[hash(k)](R)
     )

-- Without PARTITION BY (global window):
Window[f OVER (ORDER BY s)](R)
  -> Window_local[f OVER (ORDER BY s)](
       Exchange[gather](R)
     )
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("distributed-window-partitioned";
    "(window ?func ?partition_keys ?order_keys ?child)" =>
    "(window_local ?func ?order_keys
        (exchange hash_partition ?child ?partition_keys))"
    if has_partition_keys("?partition_keys")
),

rw!("distributed-window-global";
    "(window ?func () ?order_keys ?child)" =>
    "(window_local ?func ?order_keys
        (exchange gather ?child))"
),
```

## Preconditions

```rust
fn applicable(
    partition_keys: &[Column],
    child: &RelNode,
) -> bool {
    if partition_keys.is_empty() {
        // Global window: must gather
        !child.distribution().is_singleton()
    } else {
        // Partitioned window: repartition if not already
        !child.distribution()
            .is_hash_partitioned_on(partition_keys)
    }
}
```

**Restrictions:**
- Global window functions (no PARTITION BY) require gathering all data
  to one node, creating a bottleneck
- If child is already partitioned on the PARTITION BY key, no exchange
  is needed
- ORDER BY within the window requires local sorting within each partition
- Some window frames (RANGE BETWEEN) require seeing the full partition

## Cost Model

```rust
fn distributed_window_cost(
    total_rows: f64,
    num_partitions: f64,
    num_nodes: u32,
    row_bytes: f64,
    network_bandwidth: f64,
) -> f64 {
    let shuffle_fraction =
        (num_nodes - 1) as f64 / num_nodes as f64;
    let network_cost =
        total_rows * row_bytes * shuffle_fraction
        / network_bandwidth;
    let rows_per_node = total_rows / num_nodes as f64;
    let sort_cost = rows_per_node * (rows_per_node).ln() * 50e-9;
    let window_eval_cost = rows_per_node * 20e-9;
    network_cost + sort_cost + window_eval_cost
}
```

**Typical benefit**: For partitioned windows, computation is fully
parallel. For global windows, gather is unavoidable but at least
pre-sorting on each node reduces coordinator work.

## Test Cases

```sql
-- Positive: partitioned window function
SELECT user_id, event_time,
    ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY event_time)
FROM events;

-- Plan:
-- WindowLocal(ROW_NUMBER() ORDER BY event_time)
--   Exchange[hash(user_id)]
--     Scan(events)
-- Each node computes ROW_NUMBER for its user partitions
```

```sql
-- Positive: already partitioned on window key
-- events DISTRIBUTED BY HASH(user_id)
SELECT user_id,
    SUM(amount) OVER (PARTITION BY user_id ORDER BY event_time)
FROM events;

-- Plan (no exchange needed):
-- WindowLocal(SUM(amount) ORDER BY event_time)
--   Scan(events)  -- already partitioned on user_id
```

```sql
-- Negative: global window requires gather
SELECT event_time,
    ROW_NUMBER() OVER (ORDER BY event_time)
FROM events;

-- Plan:
-- WindowLocal(ROW_NUMBER() ORDER BY event_time)
--   Exchange[gather]
--     Scan(events)
-- All data sent to coordinator: bottleneck
```

## References

Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planWindow()
Spark SQL: sql/core/src/main/scala/org/apache/spark/sql/execution/window/WindowExec.scala
CockroachDB: pkg/sql/opt/xform/window_funcs.go
Greenplum: src/backend/executor/nodeWindowAgg.c
Cao et al., "Optimizing Parallel Window Queries" (VLDB 2012)
