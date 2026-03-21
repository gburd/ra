# Rule: Convert Outer Join to Inner Join

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/outer-join-to-inner.rra`

## Metadata

- **ID:** `clickhouse-outer-join-to-inner`
- **Version:** 1.0.0
- **Databases:** clickhouse
- **Tags:** database-specific, clickhouse, join, outer-join, inner-join, null-rejection
- **Authors:** "RA Contributors"


# Convert Outer Join to Inner Join

## Description

Converts LEFT/RIGHT/FULL outer joins to inner joins when subsequent filters reject NULL values from the optional side. If a filter requires non-NULL values from the right side of a LEFT JOIN, the join becomes effectively an INNER JOIN.

**When to apply**: Outer join with filters that reject NULLs from the optional side.

**Why it works**: Inner joins are simpler and can use more join algorithms (e.g., hash join without special NULL handling).

**Database version**: ClickHouse v20.3+

## Relational Algebra

```algebra
Filter[r.col IS NOT NULL](LeftJoin[c](L, R))
  -> Filter[r.col IS NOT NULL](InnerJoin[c](L, R))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("clickhouse-outer-join-to-inner";
    "(filter ?cond (left_join ?left ?right ?join_cond))" =>
    "(filter ?cond (inner_join ?left ?left ?join_cond))"
    if is_database("clickhouse")
    if rejects_nulls_from_right("?cond", "?right")
),
```

## Preconditions

```rust
fn applicable(
    filter: &Expr,
    right: &RelNode,
    join_type: JoinType,
) -> bool {
    match join_type {
        JoinType::LeftOuter => {
            // Filter must reject NULLs from right side
            filter_rejects_nulls_from(filter, right)
        }
        JoinType::RightOuter => {
            // Check left side
            false // handled by symmetric rule
        }
        _ => false,
    }
}
```

**Restrictions:**
- Only applies to ClickHouse
- Filter must reject NULL values from optional side
- Most common with WHERE clauses on outer-joined tables

## Cost Model

```rust
fn estimated_benefit() -> f64 {
    0.3 // Inner join may enable better algorithms
}
```

**Typical benefit**: 20-60% from simpler join execution

## Test Cases

```sql
SELECT * FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
WHERE c.country = 'US';

-- c.country = 'US' rejects NULLs from c
-- Convert to INNER JOIN
```

## References

**Source code:**
- ClickHouse: `src/Processors/QueryPlan/Optimizations/convertOuterJoinToInnerJoin.cpp`
  - Commit: 35f2d31186cca2f8c50f7ba4bd93817da490da85
