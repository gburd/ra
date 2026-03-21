# Rule: Distinct Over Primary Key Elimination

**Category:** logical/distinct-elimination
**File:** `rules/logical/distinct-elimination/distinct-over-primary-key.rra`

## Metadata

- **ID:** `distinct-over-primary-key`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, mssql, oracle
- **Tags:** distinct, primary-key, elimination
- **Authors:** "RA Contributors"


# Distinct Over Primary Key Elimination

## Description

When SELECT DISTINCT includes the primary key of a table, the result is already unique. The DISTINCT can be eliminated since the primary key guarantees uniqueness.

**When to apply**: Projected columns include all columns of a unique key or primary key.

## Relational Algebra

```algebra
Distinct(Project[cols including pk](Scan[table]))
  -> Project[cols including pk](Scan[table])
  where pk subset_of cols and is_primary_key(pk, table)
```

## Implementation

```rust
rw!("distinct-over-primary-key";
    "(distinct (project ?cols (scan ?table)))" =>
    "(project ?cols (scan ?table))"
    if projection_includes_key("?cols", "?table")
),
```

## Test Cases

### Positive: Distinct with PK

```sql
SELECT DISTINCT id, name FROM users;
-- id is PK; DISTINCT is redundant
```

### Negative: Distinct without PK

```sql
SELECT DISTINCT name FROM users;
-- name is not unique; keep DISTINCT
```

## References

- Key-based distinct elimination in PostgreSQL
- Functional dependency inference in SQL optimizers
