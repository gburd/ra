# Rule: Project Exchange Transpose

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/project-exchange-transpose.rra`

## Metadata

- **ID:** `calcite-project-exchange-transpose`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, cockroachdb, citus
- **Tags:** logical, projection, exchange, distributed, pushdown
- **Authors:** "Apache Calcite"


# Project Exchange Transpose

## Description

Pushes a projection below a distributed exchange (shuffle/broadcast)
operator. In distributed query execution, exchanges serialize and transfer
tuples across the network. Narrowing tuples before transfer reduces
network bandwidth, which is often the bottleneck.

**When to apply**: A Project above an Exchange where not all columns are
needed by the distribution key or downstream operators.

## Relational Algebra

```algebra
-- Before
pi[a, b](Exchange[hash(a)](R(a, b, c, d)))

-- After
Exchange[hash(a)](pi[a, b](R))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw\!("project-exchange-transpose";
    "(project ?cols (exchange ?dist ?input))" =>
    "(exchange ?dist (project (union-cols ?cols ?dist) ?input))"
    if dist_cols_in_project("?dist", "?cols")
),
```

## Preconditions

```rust
fn applicable(project: &Project, exchange: &Exchange) -> bool {
    let dist_cols = exchange.distribution_columns();
    dist_cols.is_subset(&project.output_columns())
}
```

**Restrictions:**
- Distribution key columns must be preserved
- Broadcast exchanges have no column restriction

## Cost Model

```rust
fn estimated_benefit(rows: f64, cols_removed: usize, network_bw_mbps: f64) -> f64 {
    let bytes_saved = rows * cols_removed as f64 * 8.0;
    bytes_saved / (network_bw_mbps * 1024.0 * 1024.0 / 8.0)
}
```

**Typical benefit**: 10-50% reduction in network transfer time.

## Test Cases

```sql
-- Positive: narrow before shuffle
SELECT o.order_id, o.total
FROM orders o JOIN customers c ON o.cust_id = c.id
-- If exchange hashes on cust_id, prune unneeded customer columns before shuffle

-- Negative: all columns needed by distribution
SELECT * FROM orders DISTRIBUTE BY (order_id, cust_id, total);
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ProjectExchangeTransposeRule.java
