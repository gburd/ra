# Rule: "Segment Index Pruning"

**Category:** logical/index-pruning
**File:** `rules/database-specific/clickhouse/clickhouse-segment-index-pruning.rra`

## Metadata

- **ID:** `clickhouse-segment-index-pruning`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Segment Index Pruning

## Description

Prunes segments based on min/max statistics during query execution.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(scan orders WHERE ts > now() - 1h)

-- After
(scan orders_segment_pruned WHERE ts > now() - 1h)
```

## Preconditions

- Segment index available on timestamp column

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
