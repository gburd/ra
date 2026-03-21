# Rule: "Semi/Anti Join to Inner Join Conversion"

**Category:** logical/join-simplification
**File:** `rules/database-specific/tidb/tidb-semi-anti-join-to-inner.rra`

## Metadata

- **ID:** `tidb-semi-anti-join-to-inner`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Semi/Anti Join to Inner Join Conversion

## Description

Optimizes semi/anti joins with proof of equivalence.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(semi-join orders items (on o.id = i.order_id))

-- After
(distinct (inner-join orders items (on o.id = i.order_id)) [on o.id])
```

## Preconditions

- SEMI join over function dependency satisfied

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
