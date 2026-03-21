# Rule: "Array Join Specialization"

**Category:** physical/array-operations
**File:** `rules/database-specific/clickhouse/clickhouse-array-join-optimization.rra`

## Metadata

- **ID:** `clickhouse-array-join-optimization`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Array Join Specialization

## Description

Specializes array join processing for ClickHouse's array join semantics.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(join (scan table) (array-join array_col))

-- After
(optimized-array-join (scan table) array_col)
```

## Preconditions

- Array join on columnar array types

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
