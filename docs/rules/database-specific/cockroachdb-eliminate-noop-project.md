# Rule: "Eliminate Identity Projections"

**Category:** logical/projection
**File:** `rules/database-specific/cockroachdb/cockroachdb-eliminate-noop-project.rra`

## Metadata

- **ID:** `cockroachdb-eliminate-noop-project`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Eliminate Identity Projections

## Description

Removes identity projections that don't transform data or reorder columns.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(project [a,b,c] (scan table [a,b,c]))

-- After
(scan table [a,b,c])
```

## Preconditions

- Projection preserves all columns in same order

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
