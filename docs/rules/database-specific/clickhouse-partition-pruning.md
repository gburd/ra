# Rule: "MergeTree Partition Pruning"

**Category:** logical/partition-pruning
**File:** `rules/database-specific/clickhouse/clickhouse-partition-pruning.rra`

## Metadata

- **ID:** `clickhouse-partition-pruning`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# MergeTree Partition Pruning

## Description

Eliminates MergeTree partitions that cannot contain matching rows based on partition expressions.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(filter (scan orders) (gt date '2024-01-01'))

-- After
(filter (scan orders_2024_01) (gt date '2024-01-01'))
```

## Preconditions

- Table partitioned by date expression

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
