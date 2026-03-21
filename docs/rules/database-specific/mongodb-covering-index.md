# Rule: "Covering Index Query Optimization"

**Category:** physical/covering-index
**File:** `rules/database-specific/mongodb/mongodb-covering-index.rra`

## Metadata

- **ID:** `mongodb-covering-index`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Covering Index Query Optimization

## Description

Uses covering indexes to avoid collection fetches when all fields are in index.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(project [name, email, age] (filter (scan users) (eq status 'active')))

-- After
(project [name, email, age] (filter (index-scan users idx_status_fields) (eq status 'active')))
```

## Preconditions

- Index contains all projected and filtered fields

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
