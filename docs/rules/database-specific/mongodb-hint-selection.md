# Rule: "Query Hint for Index Selection"

**Category:** physical/hint
**File:** `rules/database-specific/mongodb/mongodb-hint-selection.rra`

## Metadata

- **ID:** `mongodb-hint-selection`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Query Hint for Index Selection

## Description

Guides optimizer to use specific index via hint.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(filter (scan users) (eq email 'user@example.com'))

-- After
(filter (index-scan users idx_email (hint)) (eq email 'user@example.com'))
```

## Preconditions

- Hint explicitly specified in query

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
