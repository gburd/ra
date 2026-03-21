# Rule: "TOP-N Pushdown (ORDER BY LIMIT)"

**Category:** physical/order-limit
**File:** `rules/database-specific/tidb/tidb-top-n-pushdown.rra`

## Metadata

- **ID:** `tidb-top-n-pushdown`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# TOP-N Pushdown (ORDER BY LIMIT)

## Description

Pushes ORDER BY with LIMIT to storage layer early.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(limit (sort (scan orders) BY created_at DESC) 10)

-- After
(sort (index-seek orders idx_created_at_desc LIMIT 10))
```

## Preconditions

- Descending index exists on ORDER BY column

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
