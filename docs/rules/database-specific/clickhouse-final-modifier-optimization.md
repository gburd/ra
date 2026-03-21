# Rule: "FINAL Modifier Deduplication"

**Category:** physical/merge-tree-variant
**File:** `rules/database-specific/clickhouse/clickhouse-final-modifier-optimization.rra`

## Metadata

- **ID:** `clickhouse-final-modifier-optimization`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# FINAL Modifier Deduplication

## Description

Optimizes ReplacingMergeTree queries using FINAL modifier.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(final (scan replacingmergetree_table))

-- After
(scan replacingmergetree_deduplicated)
```

## Preconditions

- Table is ReplacingMergeTree variant

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
