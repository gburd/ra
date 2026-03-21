# Rule: "MergeTree Primary Key Index Selection"

**Category:** physical/index-selection
**File:** `rules/database-specific/clickhouse/clickhouse-primary-key-selection.rra`

## Metadata

- **ID:** `clickhouse-primary-key-selection`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# MergeTree Primary Key Index Selection

## Description

Uses MergeTree primary key for efficient range scans with early filtering.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(filter (scan orders) (and (eq user_id 123) (gt ts '2024-01-01')))

-- After
(filter (index-seek orders pk_user_ts 123) (gt ts '2024-01-01'))
```

## Preconditions

- Primary key exists on (user_id, ts)

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
