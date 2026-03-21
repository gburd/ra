# Rule: "Index Selection for Predicates"

**Category:** physical/index-selection
**File:** `rules/database-specific/mongodb/mongodb-index-selection.rra`

## Metadata

- **ID:** `mongodb-index-selection`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Index Selection for Predicates

## Description

Selects the most efficient index for query predicates.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(filter (scan orders) (and (eq status 'shipped') (gt total 100)))

-- After
(filter (index-scan orders idx_status_total) (and (eq status 'shipped') (gt total 100)))
```

## Preconditions

- Suitable index exists on status and total

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
