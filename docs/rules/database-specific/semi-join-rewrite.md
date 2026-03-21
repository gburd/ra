# Rule: TiDB Semi-Join Rewrite

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/semi-join-rewrite.rra`

## Metadata

- **ID:** `tidb-semi-join-rewrite`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** join, semi-join, rewrite, optimization
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Semi-Join Rewrite

## Description

Transforms semi-join (IN/EXISTS) to inner join when the right side has
unique keys, allowing more efficient join algorithms and better optimization.

## Relational Algebra

```algebra
R WHERE R.a IN (SELECT S.b FROM S)
  -> R JOIN (SELECT DISTINCT S.b FROM S) ON R.a = S.b
  where S.b is unique
```

## Implementation

```rust
fn rewrite_semi_join(semi: &SemiJoin) -> Option<InnerJoin> {
    if semi.right_has_unique_key() {
        Some(InnerJoin::new(semi.left, semi.right, semi.predicate))
    } else {
        None
    }
}
```

## Cost Model

Inner join can use hash/merge join instead of nested loop semi-join.

## Test Cases

```sql
-- Semi-join with unique key
SELECT * FROM orders WHERE customer_id IN
  (SELECT id FROM customers WHERE status = 'premium');
-- Rewritten: INNER JOIN on customer_id = id
```

## References
- Source: `pkg/planner/core/rule_semi_join_rewrite.go`
