# Rule: "Column Pruning for Columnar Storage"

**Category:** logical/column-pruning
**File:** `rules/database-specific/clickhouse/clickhouse-column-pruning.rra`

## Metadata

- **ID:** `clickhouse-column-pruning`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Column Pruning for Columnar Storage

## Description

Eliminates unused columns from intermediate results in columnar storage.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(project [a,b,c,d] (scan table))

-- After
(project [a,b] (scan table))
```

## Preconditions

- Columns c,d not referenced in remaining operators

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
