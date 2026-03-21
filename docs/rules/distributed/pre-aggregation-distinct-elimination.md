# Rule: Pre-Aggregation Distinct Elimination

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/pre-aggregation-distinct-elimination.rra`

## Metadata

- **ID:** `pre-aggregation-distinct-elimination`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, greenplum
- **Tags:** distributed, aggregation, pre-aggregation, distinct, dedup
- **Authors:** "RA Contributors"


# Pre-Aggregation Distinct Elimination

## Description

When aggregation follows a DISTINCT on the same columns as GROUP BY,
the DISTINCT is redundant. Remove it to avoid unnecessary deduplication
before the aggregation phase.

**When to apply**: DISTINCT columns are a subset of GROUP BY keys.

## Relational Algebra

```algebra
-- Before
gamma[g, agg(a)](delta(R))  -- delta = DISTINCT

-- After (when DISTINCT cols subset of group keys)
gamma[g, agg(a)](R)
```

## Test Cases

```sql
-- Positive: redundant DISTINCT
SELECT DISTINCT region, SUM(sales)
FROM orders GROUP BY region;
-- DISTINCT on (region, SUM(sales)) is redundant after GROUP BY region

-- Negative: DISTINCT on different columns
SELECT DISTINCT customer_id, region, SUM(sales)
FROM orders GROUP BY region;
-- customer_id not in GROUP BY
```
