# Rule: "Join Reordering for Selectivity"

**Category:** logical/join-reorder
**File:** `rules/database-specific/tidb/tidb-join-reorder.rra`

## Metadata

- **ID:** `tidb-join-reorder`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Join Reordering for Selectivity

## Description

Reorders joins to minimize intermediate result sizes.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(inner-join (inner-join a b) (inner-join c d))

-- After
(inner-join (inner-join a c) (inner-join b d))
```

## Preconditions

- Selectivity estimates available for all predicates

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
