# Rule: CTE Projection Pushdown

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/cte-projection-pushdown.rra`

## Metadata

- **ID:** `cte-projection-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, clickhouse
- **Tags:** cte, projection-pushdown, column-pruning
- **Authors:** "RA Contributors"


# CTE Projection Pushdown

## Description

Pushes projection (column pruning) from the CTE body into the CTE definition, reducing the width of materialized results.

**When to apply**: Body uses only a subset of CTE output columns.

**Why it works**: Narrower rows reduce memory and I/O for materialized CTEs.

## Relational Algebra

```algebra
project[cols](CTE[name, def](body))
  -> CTE[name, project[cols $\cap$ output(def)](def)](body)
  where cols $\subset$ output(def)
```

## Implementation

```rust
rw!("cte-projection-pushdown";
    "(project ?cols (cte ?name ?def ?body))" =>
    "(cte ?name (project ?used_cols ?def) ?body)"
    if can_prune_cte_columns("?cols", "?def")
),
```

## Cost Model

```rust
fn benefit(total_cols: usize, used_cols: usize, rows: u64) -> f64 {
    let reduction = 1.0 - (used_cols as f64 / total_cols as f64);
    reduction * 0.5 // Memory savings proportional to pruned columns
}
```

**Typical benefit**: 10-40% for wide CTEs with partial usage

## Test Cases

### Positive: Using subset of CTE columns

```sql
WITH all_data AS (SELECT id, name, email, phone, addr FROM users)
SELECT id, name FROM all_data;

-- Prune email, phone, addr from CTE
```

### Negative: All columns used

```sql
WITH t AS (SELECT id, name FROM users)
SELECT * FROM t;

-- All columns used; nothing to prune
```

## References

- DuckDB: Column pruning through CTEs
- PostgreSQL: Projection pushdown in subqueries
