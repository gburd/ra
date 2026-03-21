# Rule: IFNULL/NVL to COALESCE Normalization

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/if-null-to-coalesce.rra`

## Metadata

- **ID:** `if-null-to-coalesce`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** function, ifnull, nvl, coalesce, normalization
- **Authors:** "RA Contributors"


# IFNULL/NVL to COALESCE Normalization

## Description

Normalizes database-specific NULL-handling functions (IFNULL, NVL, NVL2,
ISNULL) to the SQL-standard COALESCE function for consistent optimization.

**When to apply**: Any dialect-specific NULL-handling function that has an
equivalent COALESCE form.

## Relational Algebra

```algebra
IFNULL(a, b)     -> COALESCE(a, b)        -- MySQL/SQLite
NVL(a, b)        -> COALESCE(a, b)        -- Oracle
NVL2(a, b, c)    -> CASE WHEN a IS NOT NULL THEN b ELSE c END
ISNULL(a, b)     -> COALESCE(a, b)        -- mssql
IIF(cond, a, b)  -> CASE WHEN cond THEN a ELSE b END
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("ifnull-to-coalesce";
    "(ifnull ?a ?b)" => "(coalesce ?a ?b)"
),
rw!("nvl-to-coalesce";
    "(nvl ?a ?b)" => "(coalesce ?a ?b)"
),
rw!("isnull-to-coalesce";
    "(isnull ?a ?b)" => "(coalesce ?a ?b)"
),
rw!("nvl2-to-case";
    "(nvl2 ?a ?b ?c)" =>
    "(case (is-not-null ?a) ?b ?c)"
),
```

## Cost Model

```rust
fn estimated_benefit() -> f64 {
    0.05 // Normalization only; enables downstream COALESCE rules
}
```

## Test Cases

### Positive: MySQL IFNULL

```sql
SELECT IFNULL(middle_name, '') FROM users;
-- Normalize to: COALESCE(middle_name, '')
```

### Positive: Oracle NVL

```sql
SELECT NVL(commission, 0) FROM employees;
-- Normalize to: COALESCE(commission, 0)
```

### Positive: mssql ISNULL

```sql
SELECT ISNULL(discount, 0) FROM products;
-- Normalize to: COALESCE(discount, 0)
```

### Negative: Already standard COALESCE

```sql
SELECT COALESCE(a, b, c) FROM t;
-- Already in canonical form
```

## References

**Implementation:**
- PostgreSQL: Parser converts NVL during compatibility mode
- DuckDB: Function binding normalizes dialect-specific functions
