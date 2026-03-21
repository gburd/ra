# Rule: "Grouping Pushdown to Pipeline"

**Category:** logical/aggregation
**File:** `rules/database-specific/mongodb/mongodb-grouping-pushdown.rra`

## Metadata

- **ID:** `mongodb-grouping-pushdown`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Grouping Pushdown to Pipeline

## Description

Pushes GROUP BY into aggregation pipeline stages.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(group-by [status] (count *) (filter (scan orders) (gt total 100)))

-- After
(aggregation-pipeline [: {total: {: 100}}, : {_id: status}])
```

## Preconditions

- Aggregation can be expressed as pipeline stages

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
