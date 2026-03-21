# Rule: "DISTINCT Elimination via Uniqueness"

**Category:** logical/distinct-elimination
**File:** `rules/database-specific/tidb/tidb-distinct-elimination.rra`

## Metadata

- **ID:** `tidb-distinct-elimination`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# DISTINCT Elimination via Uniqueness

## Description

Removes DISTINCT when output columns guarantee uniqueness.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(distinct (project [id] (scan users)))

-- After
(project [id] (scan users))
```

## Preconditions

- Projected columns are unique constraint or primary key

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
