# Rule: "Inline Projections Through Operators"

**Category:** logical/projection
**File:** `rules/database-specific/cockroachdb/cockroachdb-inline-projections.rra`

## Metadata

- **ID:** `cockroachdb-inline-projections`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Inline Projections Through Operators

## Description

Pushes projections down through operators to reduce intermediate row complexity.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(project [a,b] (sort (scan table)))

-- After
(sort (project [a,b] (scan table)))
```

## Preconditions

- All sort keys present in projection

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
