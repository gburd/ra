# Rule: "Index Intersection for AND Predicates"

**Category:** physical/index-intersection
**File:** `rules/database-specific/mongodb/mongodb-index-intersection.rra`

## Metadata

- **ID:** `mongodb-index-intersection`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Index Intersection for AND Predicates

## Description

Combines multiple index scans for AND predicates.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(filter (scan orders) (and (eq status 'shipped') (eq region 'us-east')))

-- After
(filter (index-intersection (index-scan idx_status) (index-scan idx_region)))
```

## Preconditions

- Indexes exist on both status and region

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
