# Rule: TiDB Aggregation Elimination

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/aggregation-elimination.rra`

## Metadata

- **ID:** `tidb-aggregation-elimination`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** aggregation, elimination, optimization
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Aggregation Elimination

## Description

Eliminates unnecessary GROUP BY when all rows are guaranteed to be unique
based on unique constraints or primary keys. If grouping columns form a
unique key, the aggregation becomes a no-op and can be removed.

## Relational Algebra

```algebra
Agg[group_cols, aggs](R)
  -> Project[aggs](R)
  where has_unique_constraint(R, group_cols)
```

## Implementation

```rust
fn eliminate_aggregation(agg: &Aggregation) -> Option<Projection> {
    if agg.group_by_cols.is_unique_in_child() {
        Some(Projection::new(agg.agg_funcs))
    } else {
        None
    }
}
```

## Cost Model

Eliminates aggregation overhead entirely when grouping columns are unique.

## Test Cases

```sql
-- Primary key as GROUP BY
SELECT id, SUM(amount) FROM orders GROUP BY id;
-- Eliminated: id is unique, no grouping needed
```

## References
- Source: `pkg/planner/core/rule_aggregation_push_down.go`
- TiDB Docs: https://docs.pingcap.com/tidb/stable/sql-logical-optimization
