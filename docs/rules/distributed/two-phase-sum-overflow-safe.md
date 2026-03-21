# Rule: Two-Phase SUM with Overflow Protection

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/two-phase-sum-overflow-safe.rra`

## Metadata

- **ID:** `two-phase-sum-overflow-safe`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb
- **Tags:** distributed, aggregation, two-phase, sum, overflow, safety
- **Authors:** "RA Contributors"


# Two-Phase SUM with Overflow Protection

## Description

For SUM on integer columns, local partial sums can overflow if the
partition is large. Use a wider intermediate type (e.g., BIGINT for
INT, DECIMAL for BIGINT) for the partial sum to prevent overflow.

**When to apply**: SUM on integer or decimal columns in distributed
aggregation where partial sums may exceed the column's type range.

## Relational Algebra

```algebra
-- Before
gamma[g, SUM(x::INT)](R)

-- After (widen to BIGINT for partial sum)
gamma[g, SUM(partial_sum)::INT](
    Exchange[hash(g)](
        gamma[g, SUM(CAST(x AS BIGINT)) AS partial_sum](R)
    )
)
```

## Test Cases

```sql
-- Positive: SUM on INT column with many rows
SELECT store_id, SUM(quantity) FROM line_items GROUP BY store_id;
-- quantity is INT, partial sum may exceed INT_MAX on large partitions

-- Negative: SUM on BIGINT (already widest integer type)
SELECT region, SUM(total_revenue) FROM reports GROUP BY region;
-- BIGINT partial sums are unlikely to overflow
```
