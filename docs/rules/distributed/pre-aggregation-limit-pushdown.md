# Rule: Pre-Aggregation Limit Pushdown

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/pre-aggregation-limit-pushdown.rra`

## Metadata

- **ID:** `pre-aggregation-limit-pushdown`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark
- **Tags:** distributed, aggregation, pre-aggregation, limit, top-n
- **Authors:** "RA Contributors"


# Pre-Aggregation Limit Pushdown

## Description

When a LIMIT follows aggregation with ORDER BY, push a local top-K
filter into the partial aggregation phase. Each node keeps only the
top K groups, reducing shuffle volume.

**When to apply**: Query has ORDER BY + LIMIT after GROUP BY, and
the ordering is on an aggregate result (e.g., ORDER BY SUM(x) DESC LIMIT 10).

## Relational Algebra

```algebra
-- Before
limit[K](sort[agg DESC](gamma[g, agg(a)](Exchange[hash(g)](R))))

-- After
limit[K](sort[agg DESC](
    gamma[g, merge_agg(partial_a)](
        Exchange[hash(g)](
            topk[K, partial_agg DESC](
                gamma[g, partial_agg(a)](R)
            )
        )
    )
))
```

## Test Cases

```sql
-- Positive: top-K aggregation
SELECT region, SUM(sales) AS total
FROM orders GROUP BY region ORDER BY total DESC LIMIT 10;

-- Negative: no LIMIT clause
SELECT region, SUM(sales) FROM orders GROUP BY region;
```
