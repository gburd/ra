# Rule: Two-Phase MIN/MAX Optimization

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/two-phase-min-max.rra`

## Metadata

- **ID:** `two-phase-min-max`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum, citus
- **Tags:** distributed, aggregation, two-phase, min, max, extrema
- **Authors:** "RA Contributors"


# Two-Phase MIN/MAX Optimization

## Description

MIN and MAX are perfectly decomposable: MIN(local_mins) = global MIN,
MAX(local_maxs) = global MAX. Each node computes local extrema, and
the global phase takes the extremum of the partial results.

**When to apply**: MIN or MAX aggregate on distributed data.

## Relational Algebra

```algebra
-- MIN decomposition
gamma[g, MIN(partial_min)](
    Exchange[hash(g)](
        gamma[g, MIN(x) AS partial_min](R)
    )
)

-- MAX decomposition
gamma[g, MAX(partial_max)](
    Exchange[hash(g)](
        gamma[g, MAX(x) AS partial_max](R)
    )
)
```

## Test Cases

```sql
-- Positive: simple MIN
SELECT category, MIN(price) FROM products GROUP BY category;

-- Positive: combined MIN and MAX
SELECT region, MIN(temperature), MAX(temperature)
FROM weather GROUP BY region;

-- Positive: indexed MIN (can use index scan)
SELECT user_id, MIN(created_at) FROM events GROUP BY user_id;
```
