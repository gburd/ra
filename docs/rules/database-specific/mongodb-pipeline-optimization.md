# Rule: "Aggregation Pipeline Stage Reordering"

**Category:** physical/aggregation
**File:** `rules/database-specific/mongodb/mongodb-pipeline-optimization.rra`

## Metadata

- **ID:** `mongodb-pipeline-optimization`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** database-mining, mongodb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Aggregation Pipeline Stage Reordering

## Description

Optimizes MongoDB aggregation pipelines by reordering stages.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
[, , , ]

-- After
[, , , ]
```

## Preconditions

-  stage filters can be applied before 

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
