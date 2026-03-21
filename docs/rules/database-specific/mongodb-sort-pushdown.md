# Rule: "Sort Pushdown Through Index"

**Category:** physical/sort
**File:** `rules/database-specific/mongodb/mongodb-sort-pushdown.rra`

## Metadata

- **ID:** `mongodb-sort-pushdown`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Sort Pushdown Through Index

## Description

Pushes ordering to index seek when possible.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(sort (index-scan users idx_email) BY updated_at DESC LIMIT 10)

-- After
(index-scan users idx_updated_at_desc LIMIT 10)
```

## Preconditions

- Descending index exists on updated_at

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
