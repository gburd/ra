# Rule: TiDB Outer Join Elimination

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/outer-join-elimination.rra`

## Metadata

- **ID:** `tidb-outer-join-elimination`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** join, outer-join, elimination, optimization
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Outer Join Elimination

## Description

Simplifies outer joins to inner joins when predicates on the null-supplying
side guarantee that NULL-extended rows will be filtered out anyway.

## Relational Algebra

```algebra
Filter[pred on S](R LEFT JOIN S ON R.a = S.b)
  -> Filter[pred](R INNER JOIN S ON R.a = S.b)
  where pred rejects nulls
```

## Implementation

```rust
fn eliminate_outer_join(outer: &OuterJoin, filter: &Filter) -> Option<InnerJoin> {
    if filter.rejects_nulls_from(outer.null_side) {
        Some(InnerJoin::new(outer.left, outer.right, outer.predicate))
    } else {
        None
    }
}
```

## Cost Model

Inner joins are more efficient and enable more optimization opportunities.

## Test Cases

```sql
-- LEFT JOIN with filter on right side
SELECT * FROM orders o LEFT JOIN customers c ON o.customer_id = c.id
WHERE c.status = 'active';
-- Simplified: INNER JOIN (status filter rejects NULLs)
```

## References
- Source: `pkg/planner/core/rule_outer_join_elimination.go`
