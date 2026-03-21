# Rule: "Sort Elimination via Index Ordering"

**Category:** logical/sort-elimination
**File:** `rules/database-specific/mongodb/mongodb-sort-elimination.rra`

## Metadata

- **ID:** `mongodb-sort-elimination`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Sort Elimination via Index Ordering

## Description

Removes sorts when index provides required ordering.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(sort (filter (scan users) (eq status 'active')) BY created_at DESC)

-- After
(sort (filter (index-scan users idx_status_created_at DESC) (eq status 'active')))
```

## Preconditions

- Index already ordered by sort column

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
