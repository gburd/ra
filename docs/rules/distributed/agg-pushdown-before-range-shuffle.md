# Rule: Aggregation Pushdown Before Range Shuffle

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/agg-pushdown-before-range-shuffle.rra`

## Metadata

- **ID:** `agg-pushdown-before-range-shuffle`
- **Version:** "1.0.0"
- **Databases:** spark, greenplum, cockroachdb
- **Tags:** distributed, aggregation, pushdown, range-partition, sort
- **Authors:** "RA Contributors"


# Aggregation Pushdown Before Range Shuffle

## Description

When aggregation precedes a range-partitioned exchange (used for
ordered output or range-based distribution), push partial aggregation
below the range shuffle. Each partition pre-aggregates locally,
reducing the data that enters the range-based redistribution.

**When to apply**: Aggregate with ORDER BY uses range partitioning
for the exchange, and the aggregate function is decomposable.

## Relational Algebra

```algebra
-- Before
sort[g](gamma[g, agg(a)](Exchange[range(g)](R)))

-- After
sort[g](gamma[g, merge_agg(partial)](
    Exchange[range(g)](
        gamma[g, partial_agg(a)](R)
    )
))
```

## Test Cases

```sql
-- Positive: ordered group-by with range distribution
SELECT date_trunc('day', ts) AS day, SUM(events)
FROM metrics GROUP BY day ORDER BY day;
-- Range partition on day preserves sort order

-- Negative: no ordering requirement
SELECT region, SUM(sales) FROM orders GROUP BY region;
-- Hash partition is more appropriate
```
