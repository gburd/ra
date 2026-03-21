# Rule: Distributed Top-N

**Category:** distributed/distributed-sort
**File:** `rules/distributed/distributed-sort/distributed-topn.rra`

## Metadata

- **ID:** `distributed-topn`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum
- **Tags:** distributed, sort, topn, limit, merge-sort
- **Authors:** "RA Contributors"


# Distributed Top-N

## Description

Decomposes a global Top-N (ORDER BY ... LIMIT N) into local Top-N on
each node followed by a merge-sort gather that produces the final N rows.
Each node maintains a heap of size N, discarding rows that cannot be in
the global top N.

**When to apply**: A query has ORDER BY with LIMIT, and data is
distributed across multiple nodes.

**Why it works**: Each node independently computes its local top-N using
a bounded heap. Only N rows per node are sent to the coordinator, which
merge-sorts them. For small N relative to total rows, this avoids
sorting and transferring the full dataset.

## Relational Algebra

```algebra
Limit[n](Sort[k](R))
  -> Limit[n](MergeSortGather[k](TopN[n, k](R_partition)))

-- Per partition:
TopN[n, k](R_i) uses a size-n heap, O(|R_i| * log n) time
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("distributed-topn";
    "(limit ?n (sort ?keys (exchange gather ?child)))" =>
    "(limit ?n
        (merge_sort_gather ?keys
            (topn ?n ?keys ?child)))"
),
```

## Preconditions

```rust
fn applicable(limit_n: u64, total_rows: f64) -> bool {
    // Top-N is beneficial when N << total_rows
    (limit_n as f64) < total_rows * 0.01
}
```

**Restrictions:**
- Requires a sort-preserving gather (merge-sort) instead of arbitrary
  gather
- OFFSET must be handled: local Top-N should produce
  `OFFSET + LIMIT` rows
- Ties (duplicate sort keys at the boundary) may require sending
  slightly more than N rows per node
- Window functions with ORDER BY LIMIT patterns need careful handling

## Cost Model

```rust
fn distributed_topn_cost(
    total_rows: f64,
    limit_n: u64,
    num_nodes: u32,
    row_bytes: f64,
    network_bandwidth: f64,
) -> f64 {
    let rows_per_node = total_rows / num_nodes as f64;
    // Local heap operation: O(rows_per_node * log(n))
    let local_cpu = rows_per_node * (limit_n as f64).ln();
    // Network: n rows per node
    let network = limit_n as f64 * num_nodes as f64 * row_bytes
        / network_bandwidth;
    // Merge sort at coordinator: O(n * num_nodes * log(num_nodes))
    let merge_cpu =
        limit_n as f64 * num_nodes as f64
        * (num_nodes as f64).ln();
    local_cpu + network + merge_cpu
}
```

**Typical benefit**: For LIMIT 100 on 1B rows across 100 nodes, transfers
10K rows instead of 1B (99.999% reduction).

## Test Cases

```sql
-- Positive: classic Top-N
SELECT * FROM events ORDER BY timestamp DESC LIMIT 10;

-- Plan:
-- Limit(10)
--   MergeSortGather(timestamp DESC)
--     TopN(10, timestamp DESC)
--       Scan(events)  -- each node picks local top 10
-- Network: 10 * num_nodes rows
```

```sql
-- Positive: Top-N with offset
SELECT * FROM events ORDER BY timestamp DESC LIMIT 10 OFFSET 90;

-- Plan:
-- Limit(10, offset=90)
--   MergeSortGather(timestamp DESC)
--     TopN(100, timestamp DESC)  -- OFFSET + LIMIT
--       Scan(events)
```

```sql
-- Negative: LIMIT is large relative to data
SELECT * FROM small_table ORDER BY id LIMIT 10000;
-- small_table has 15000 rows -> Top-N has minimal benefit
-- Full sort + gather may be simpler
```

## References

Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/iterative/rule/PushTopNThroughExchange.java
Spark SQL: sql/catalyst/src/main/scala/org/apache/spark/sql/execution/limit.scala - TakeOrderedAndProjectExec
CockroachDB: pkg/sql/opt/norm/rules/limit.opt
Greenplum: src/backend/executor/nodeSort.c - sort with limit
