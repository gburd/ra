# Rule: Aggregation Pushdown Before Broadcast

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/agg-pushdown-before-broadcast.rra`

## Metadata

- **ID:** `agg-pushdown-before-broadcast`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark
- **Tags:** distributed, aggregation, pushdown, broadcast, small-table
- **Authors:** "RA Contributors"


# Aggregation Pushdown Before Broadcast

## Description

When an aggregate result is broadcast to all nodes (e.g., for a
broadcast join with an aggregate subquery), push the aggregation
below the broadcast to compute it once locally rather than
broadcasting raw data and aggregating everywhere.

**When to apply**: Aggregate subquery is used as the build side of
a broadcast join, and the aggregate result is small.

## Relational Algebra

```algebra
-- Before (aggregate after broadcast)
gamma[g, agg(a)](Exchange[broadcast](R))

-- After (aggregate before broadcast)
Exchange[broadcast](gamma[g, agg(a)](R))
```

## Test Cases

```sql
-- Positive: small aggregate in subquery
SELECT * FROM orders o
JOIN (SELECT region, AVG(amount) AS avg_amt
      FROM orders GROUP BY region) agg
ON o.region = agg.region;
-- Aggregate produces few rows -> broadcast result, not raw data

-- Negative: large aggregate result
SELECT * FROM orders o
JOIN (SELECT user_id, SUM(amount) FROM orders GROUP BY user_id) agg
ON o.user_id = agg.user_id;
-- Many users -> result too large for broadcast
```
