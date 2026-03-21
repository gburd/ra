# Rule: Three-Phase High Cardinality SUM

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/three-phase-high-cardinality-sum.rra`

## Metadata

- **ID:** `three-phase-high-cardinality-sum`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, greenplum
- **Tags:** distributed, aggregation, three-phase, high-cardinality, sum
- **Authors:** "RA Contributors"


# Three-Phase High Cardinality SUM

## Description

For SUM with very high group cardinality and skewed distribution, use
three phases: (1) local partial SUM, (2) shuffle by a hash of group keys
to redistribute partially aggregated data, (3) final SUM. The extra
shuffle step helps when local pre-aggregation provides little reduction
due to high cardinality.

**When to apply**: Group cardinality > 50% of input rows and
distribution is skewed. Two-phase provides minimal benefit because
local reduction is small.

## Relational Algebra

```algebra
-- Three-phase SUM with repartition
gamma[g, SUM(partial_sum)](
    Exchange[hash(g)](
        gamma[g, SUM(a) as partial_sum](
            Exchange[hash(salt(g))](R)  -- salt to redistribute
        )
    )
)
```

## Test Cases

```sql
-- Positive: high cardinality groups with skew
SELECT user_id, SUM(event_value) FROM events GROUP BY user_id;
-- Millions of users, Zipf distribution

-- Negative: low cardinality (two-phase suffices)
SELECT region, SUM(sales) FROM orders GROUP BY region;
```
