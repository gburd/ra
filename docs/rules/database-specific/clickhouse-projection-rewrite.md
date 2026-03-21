# Rule: "Use Materialized Projections"

**Category:** logical/materialized-view
**File:** `rules/database-specific/clickhouse/clickhouse-projection-rewrite.rra`

## Metadata

- **ID:** `clickhouse-projection-rewrite`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Use Materialized Projections

## Description

Rewrites queries to use precomputed projections with different primary keys.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(group-by [col1] (sum col2) (scan orders))

-- After
(scan orders_by_col1_sum)
```

## Preconditions

- Projection exists matching aggregation

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
