# Rule: "Aggregate Function Elimination"

**Category:** logical/aggregate-elimination
**File:** `rules/database-specific/tidb/tidb-aggregate-elimination.rra`

## Metadata

- **ID:** `tidb-aggregate-elimination`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Aggregate Function Elimination

## Description

Simplifies GROUP BY aggregates when grouping columns form a unique key.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(group-by [id] (count *) (scan users))

-- After
(project [id] (scan users))
```

## Preconditions

- GROUP BY columns form unique/primary key

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
