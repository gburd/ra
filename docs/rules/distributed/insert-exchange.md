# Rule: Exchange Operator Insertion

**Category:** distributed/exchange-placement
**File:** `rules/distributed/exchange-placement/insert-exchange.rra`

## Metadata

- **ID:** `insert-exchange`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, greenplum
- **Tags:** distributed, exchange, shuffle, broadcast, parallelism
- **Authors:** "RA Contributors"


# Exchange Operator Insertion

## Description

Inserts exchange operators into a query plan to satisfy the distribution
requirements of each operator. An exchange redistributes data between
nodes so that downstream operators receive rows partitioned (or
replicated) correctly.

**When to apply**: After logical optimization produces a plan, the
distributed planner walks bottom-up and compares the output distribution
of each child against the input requirement of its parent. Where they
disagree an exchange is inserted.

**Why it works**: Distributed operators (e.g., hash join, grouped
aggregation) require their inputs to be distributed on specific keys. By
inserting the minimal set of exchanges, we satisfy these requirements
while preserving parallelism.

## Relational Algebra

```algebra
-- When parent requires HashPartitioned(k) and child produces Random:
Op(child) -> Op(Exchange[hash(k)](child))

-- When parent requires Singleton and child produces HashPartitioned:
Op(child) -> Op(Exchange[gather](child))

-- When parent requires Replicated and child is small:
Op(child) -> Op(Exchange[broadcast](child))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("insert-hash-exchange";
    "(join hash ?cond ?left ?right)" =>
    "(join hash ?cond
        (exchange hash_partition ?left ?join_keys_left)
        (exchange hash_partition ?right ?join_keys_right))"
    if needs_repartition("?left", "?right", "?join_keys_left")
),

rw!("insert-gather-exchange";
    "(aggregate global ?agg_fn ?child)" =>
    "(aggregate global ?agg_fn (exchange gather ?child))"
    if child_is_distributed("?child")
),
```

## Preconditions

```rust
fn applicable(
    parent_required: &Distribution,
    child_actual: &Distribution,
) -> bool {
    // Insert exchange only when distributions disagree
    !child_actual.satisfies(parent_required)
}
```

**Restrictions:**
- Do not insert redundant exchanges when the child already satisfies the
  parent's distribution requirement
- Prefer repartition over broadcast when the smaller side exceeds the
  broadcast threshold
- Gather exchanges create a single-node bottleneck; avoid unless required

## Cost Model

```rust
fn exchange_cost(
    rows: f64,
    row_bytes: f64,
    num_nodes: u32,
    network_bandwidth: f64,
) -> f64 {
    let transfer_bytes = rows * row_bytes;
    let serialization_cost = transfer_bytes * 0.1;
    let network_cost = transfer_bytes / network_bandwidth;
    serialization_cost + network_cost
}
```

**Assumptions:**
- Network bandwidth is uniform across nodes
- Serialization cost is ~10% of raw data size
- Data is evenly distributed (no skew)

**Typical benefit**: Exchange insertion itself adds cost; the benefit comes
from enabling parallel execution of downstream operators.

## Test Cases

```sql
-- Positive: join on non-co-located keys requires exchange
-- Before
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;
-- orders is hash-partitioned on order_id, customers on id

-- After (exchange inserted to repartition orders on customer_id)
-- Plan:
-- HashJoin(cond: o.customer_id = c.id)
--   Exchange[hash(customer_id)](Scan(orders))
--   Scan(customers)   -- already partitioned on id
```

```sql
-- Negative: tables already co-partitioned on join key
-- Before
SELECT * FROM orders o JOIN order_items i ON o.id = i.order_id;
-- Both partitioned on the join key

-- After: no exchange needed
-- Plan:
-- HashJoin(cond: o.id = i.order_id)
--   Scan(orders)
--   Scan(order_items)
```

## References

Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java
Spark SQL: sql/catalyst/src/main/scala/org/apache/spark/sql/catalyst/plans/physical/partitioning.scala
Greenplum: src/backend/cdb/cdbllize.c - apply_motion()
Graefe, "Encapsulation of Parallelism in the Volcano Query Processing System" (SIGMOD 1990)
