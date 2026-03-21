# Rule: "LIMIT Pushdown Through Operators"

**Category:** logical/limit-pushdown
**File:** `rules/database-specific/tidb/tidb-limit-pushdown.rra`

## Metadata

- **ID:** `tidb-limit-pushdown`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# LIMIT Pushdown Through Operators

## Description

Pushes LIMIT operators closer to data source to reduce processing.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(limit (sort (join a b)) 10)

-- After
(sort (join (limit a 10) (limit b 10)))
```

## Preconditions

- LIMIT independent of aggregate results

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
