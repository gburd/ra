# Rule: Three-Phase High Cardinality COUNT

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/three-phase-high-cardinality-count.rra`

## Metadata

- **ID:** `three-phase-high-cardinality-count`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, greenplum
- **Tags:** distributed, aggregation, three-phase, high-cardinality, count-distinct
- **Authors:** "RA Contributors"


# Three-Phase High Cardinality COUNT

## Description

For COUNT(DISTINCT x) with high-cardinality columns, use three phases:
(1) local deduplication, (2) shuffle by distinct key, (3) final count.
This handles the case where COUNT(DISTINCT) cannot be decomposed in
two phases because duplicates span multiple nodes.

**When to apply**: COUNT(DISTINCT x) where x has high cardinality
relative to the number of nodes, and significant local duplication.

## Relational Algebra

```algebra
-- Three-phase COUNT DISTINCT
gamma[g, COUNT(*)].final(
    Exchange[hash(g)](
        gamma[g, d, COUNT(*)].intermediate(
            Exchange[hash(d)](
                gamma[g, d].local_dedup(R)
            )
        )
    )
)
```

## Test Cases

```sql
-- Positive: high-cardinality distinct count with duplication
SELECT region, COUNT(DISTINCT customer_id)
FROM orders GROUP BY region;

-- Negative: column is primary key (no local duplicates)
SELECT region, COUNT(DISTINCT order_id) FROM orders GROUP BY region;
```
