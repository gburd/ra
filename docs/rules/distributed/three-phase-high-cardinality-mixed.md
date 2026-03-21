# Rule: Three-Phase High Cardinality Mixed Aggregates

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/three-phase-high-cardinality-mixed.rra`

## Metadata

- **ID:** `three-phase-high-cardinality-mixed`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark
- **Tags:** distributed, aggregation, three-phase, high-cardinality, mixed
- **Authors:** "RA Contributors"


# Three-Phase High Cardinality Mixed Aggregates

## Description

For queries with multiple aggregate functions of mixed types (some
decomposable, some requiring distinct), use a three-phase strategy
that handles both types in a unified plan. Decomposable aggregates
use partial/merge, while distinct aggregates use the three-phase
dedup approach.

**When to apply**: Query has both decomposable aggregates (SUM, COUNT)
and non-decomposable aggregates (COUNT DISTINCT) on the same group keys.

## Relational Algebra

```algebra
-- Mixed: SUM + COUNT(DISTINCT)
-- Split into two branches, join on group keys
Join[g](
    gamma[g, SUM(partial_sum)](  -- decomposable branch
        Exchange[hash(g)](
            gamma[g, SUM(a) as partial_sum](R)
        )
    ),
    gamma[g, COUNT(*)].final(    -- distinct branch
        Exchange[hash(g)](
            gamma[g, d].dedup(
                Exchange[hash(d)](R)
            )
        )
    )
)
```

## Test Cases

```sql
-- Positive: mixed aggregate types
SELECT region, SUM(amount), COUNT(DISTINCT customer_id)
FROM orders GROUP BY region;

-- Negative: all aggregates same type
SELECT region, SUM(a), SUM(b) FROM orders GROUP BY region;
-- Standard two-phase handles this
```
