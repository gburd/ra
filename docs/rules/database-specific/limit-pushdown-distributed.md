# Rule: Push LIMIT Below Remote Exchange

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/limit-pushdown-distributed.rra`

## Metadata

- **ID:** `clickhouse-limit-pushdown-distributed`
- **Version:** 1.0.0
- **Databases:** clickhouse
- **Tags:** database-specific, clickhouse, limit, distributed, pushdown
- **Authors:** " RA Contributors"


# Push LIMIT Below Remote Exchange

## Description

Pushes a LIMIT operator below a remote read/exchange to reduce data transfer from remote nodes in distributed queries. Each node returns only LIMIT rows instead of all matching rows.

**When to apply**: LIMIT over a distributed read from remote nodes.

**Why it works**: Reduces network traffic by N * (total_rows - limit * n_nodes). Most beneficial when limit is small relative to total data.

**Database version**: ClickHouse v1.0+ (distributed tables)

## Relational Algebra

```algebra
Limit[k](RemoteRead[nodes](R))
  -> MergeLimit[k]($\forall$n $\in$ nodes: Limit[k](Read_n(R)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("clickhouse-limit-pushdown-distributed";
    "(limit ?k (remote_read ?nodes ?query))" =>
    "(merge_limit ?k (remote_read_with_limit ?nodes ?query ?k))"
    if is_database("clickhouse")
),
```

**Typical benefit**: 50-90% reduction in network traffic

## References

**Source code:**
- ClickHouse: `src/Processors/QueryPlan/Optimizations/limitPushDown.cpp`
  - Commit: 35f2d31186cca2f8c50f7ba4bd93817da490da85
