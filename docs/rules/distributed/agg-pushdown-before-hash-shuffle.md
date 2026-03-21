# Rule: Aggregation Pushdown Before Hash Shuffle

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/agg-pushdown-before-hash-shuffle.rra`

## Metadata

- **ID:** `agg-pushdown-before-hash-shuffle`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum
- **Tags:** distributed, aggregation, pushdown, shuffle, hash-partition
- **Authors:** "RA Contributors"


# Aggregation Pushdown Before Hash Shuffle

## Description

Push partial aggregation below a hash-partition exchange to reduce
network transfer. This is the core optimization in two-phase
aggregation: the partial aggregate runs before the shuffle so that
only aggregated rows cross the network.

**When to apply**: An aggregate sits above a hash-partitioned exchange,
and the aggregate function is decomposable.

## Relational Algebra

```algebra
-- Before
gamma[g, agg(a)](Exchange[hash(g)](R))

-- After
gamma[g, merge_agg(partial)](
    Exchange[hash(g)](
        gamma[g, partial_agg(a)](R)
    )
)
```

## Test Cases

```sql
-- Positive: basic pushdown
SELECT department, SUM(salary) FROM employees GROUP BY department;
-- Push partial SUM below exchange

-- Positive: multiple aggregates
SELECT region, SUM(qty), MAX(price), COUNT(*)
FROM sales GROUP BY region;
-- Push all three partial aggregates below exchange
```
