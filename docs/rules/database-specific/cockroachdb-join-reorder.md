# Rule: "Join Reorder via Graph"

**Category:** logical/join-reorder
**File:** `rules/database-specific/cockroachdb/cockroachdb-join-reorder.rra`

## Metadata

- **ID:** `cockroachdb-join-reorder`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Join Reorder via Graph

## Description

Reorders joins to minimize intermediate result sizes using join graph construction.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(inner-join (inner-join a b) (inner-join c d))

-- After
(inner-join (inner-join a c) (inner-join b d))
```

## Preconditions

- Multiple join operators present

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
