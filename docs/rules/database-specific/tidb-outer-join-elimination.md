# Rule: "Convert OUTER JOIN to INNER JOIN"

**Category:** logical/join-simplification
**File:** `rules/database-specific/tidb/tidb-outer-join-elimination.rra`

## Metadata

- **ID:** `tidb-outer-join-elimination`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Convert OUTER JOIN to INNER JOIN

## Description

Converts OUTER JOIN to INNER JOIN when provably safe.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(left-join orders customers (on o.customer_id = c.id))

-- After
(inner-join orders customers (on o.customer_id = c.id))
```

## Preconditions

- customer_id is NOT NULL guaranteed by business logic

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
