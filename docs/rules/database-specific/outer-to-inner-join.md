# Rule: Apache Derby Outer-to-Inner Join Conversion

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/outer-to-inner-join.rra`

## Metadata

- **ID:** `derby-outer-to-inner-join`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, outer-join, inner-join, simplification
- **Authors:** "RA Contributors"


# Apache Derby Outer-to-Inner Join Conversion

## Description

Derby converts outer joins to inner joins when a WHERE predicate on
the null-supplying side guarantees that NULL-extended rows would be
filtered out anyway.  An inner join allows more optimization
opportunities including join reordering and additional predicate
pushdown.

**When to apply**: A LEFT/RIGHT OUTER JOIN has a WHERE predicate that
rejects NULLs on the null-supplying side (e.g., `WHERE b.col IS NOT
NULL` or `WHERE b.col = value`).

**Why it works**: If the WHERE clause filters out all rows where the
outer join would produce NULLs, the outer join is semantically
equivalent to an inner join.  Inner joins are more flexible for the
optimizer.

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Before: outer join + null-rejecting filter
sigma[b.x = 5](A LEFT JOIN[a.id = b.fk] B)

-- After: converted to inner join
sigma[b.x = 5](A INNER JOIN[a.id = b.fk] B)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-outer-to-inner";
    "(filter ?pred
        (left-join ?join_pred ?left ?right))" =>
    "(filter ?pred
        (join ?join_pred ?left ?right))"
    if is_database("derby")
    if predicate_rejects_null("?pred", "?right")
),
```

## Preconditions

```rust
fn applicable(
    where_pred: &Predicate,
    null_side: &Relation,
) -> bool {
    where_pred.columns().iter().any(|c| {
        null_side.contains_column(c)
        && where_pred.rejects_null_on(c)
    })
}
```

**Restrictions:**
- Only applies when WHERE (not ON) rejects NULLs
- Cannot convert if the NULL-extended rows are actually needed
- Applies to both LEFT and RIGHT outer joins

## Cost Model

```rust
fn estimated_benefit(
    _rows: f64,
) -> f64 {
    0.0 // No direct performance benefit; enables further optimizations
}
```

**Typical benefit**: Enables join reordering and additional predicate
pushdown.

## Test Cases

```sql
-- Positive: null-rejecting WHERE on outer side
SELECT * FROM orders o
LEFT JOIN returns r ON o.id = r.order_id
WHERE r.reason = 'damaged';
-- Converted to inner join; r.reason = 'damaged' rejects NULLs
```

```sql
-- Negative: no null-rejecting predicate
SELECT * FROM orders o
LEFT JOIN returns r ON o.id = r.order_id;
-- Must keep as outer join; NULLs are needed
```

## References

Apache Derby: Optimizer documentation
Source: org.apache.derby.impl.sql.compile.JoinNode,
  `convertOuterToInner()`
