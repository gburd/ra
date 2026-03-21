# Rule: Pre-Aggregation Projection Pushdown

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/pre-aggregation-projection-pushdown.rra`

## Metadata

- **ID:** `pre-aggregation-projection-pushdown`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb
- **Tags:** distributed, aggregation, pre-aggregation, projection, pushdown
- **Authors:** "RA Contributors"


# Pre-Aggregation Projection Pushdown

## Description

Push projections below the local aggregation phase to reduce row width
before partial aggregation. Only columns needed for grouping and
aggregate arguments are retained, reducing memory and I/O.

**When to apply**: Input relation has columns not referenced by
group keys or aggregate arguments.

## Relational Algebra

```algebra
-- Before
gamma[g, agg(a)](R)  -- R has columns [g, a, x, y, z]

-- After
gamma[g, agg(a)](pi[g, a](R))  -- narrow to needed columns
```

## Test Cases

```sql
-- Positive: wide table with few agg columns
SELECT region, SUM(amount) FROM orders GROUP BY region;
-- orders has 20 columns, only need region + amount

-- Negative: all columns referenced
SELECT a, SUM(b), COUNT(c) FROM t GROUP BY a;
-- table only has columns a, b, c
```
