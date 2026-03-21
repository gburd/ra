# Rule: Global Aggregation to Single Node

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/global-agg-single-node.rra`

## Metadata

- **ID:** `global-agg-single-node`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum
- **Tags:** distributed, aggregation, global, gather, single-node
- **Authors:** "RA Contributors"


# Global Aggregation to Single Node

## Description

For aggregation without GROUP BY (e.g., SELECT COUNT(*) FROM t),
all data must converge to a single node. Optimize by running local
partial aggregations first, then gathering only the partial results
to the coordinator for final aggregation.

**When to apply**: No GROUP BY clause (global aggregate), distributed
input.

## Relational Algebra

```algebra
-- Before (gather all rows)
gamma[agg(a)](Exchange[gather](R))

-- After (partial + gather + final)
gamma[merge_agg(partial)](
    Exchange[gather](
        gamma[partial_agg(a)](R)
    )
)
```

## Test Cases

```sql
-- Positive: global count
SELECT COUNT(*) FROM orders;
-- Each node counts locally, coordinator sums

-- Positive: global sum and average
SELECT SUM(amount), AVG(amount), MIN(created_at) FROM orders;
-- Local: SUM, SUM, COUNT, MIN -> Gather -> Final

-- Negative: non-decomposable global aggregate
SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY amount)
FROM orders;
-- Must gather all rows
```
