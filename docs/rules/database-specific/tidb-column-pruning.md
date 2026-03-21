# Rule: "Column Pruning in Intermediate Steps"

**Category:** logical/column-pruning
**File:** `rules/database-specific/tidb/tidb-column-pruning.rra`

## Metadata

- **ID:** `tidb-column-pruning`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Column Pruning in Intermediate Steps

## Description

Removes unused columns from intermediate query results.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(project [a,b,c] (join (scan t1 [a,b,d]) (scan t2 [c])))

-- After
(project [a,b,c] (join (scan t1 [a,b]) (scan t2 [c])))
```

## Preconditions

- Column d not referenced in output or remaining operators

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
