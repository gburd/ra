# Rule: "Elide Unnecessary Projections"

**Category:** logical/projection
**File:** `rules/database-specific/cockroachdb/cockroachdb-elide-projection.rra`

## Metadata

- **ID:** `cockroachdb-elide-projection`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Elide Unnecessary Projections

## Description

Removes unnecessary projection operators when column sets match exactly.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(project [a,b,c] (project [a,b,c] (scan table)))

-- After
(project [a,b,c] (scan table))
```

## Preconditions

- Inner projection columns identical to outer

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
