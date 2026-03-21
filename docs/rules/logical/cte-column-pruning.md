# Rule: CTE Column Pruning

**Category:** logical/cte-optimization
**File:** `rules/logical/cte-optimization/cte-column-pruning.rra`

## Metadata

- **ID:** `cte-column-pruning`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite
- **Tags:** cte, column-pruning, projection
- **Authors:** "RA Contributors"


# CTE Column Pruning

## Description

Removes unused columns from a CTE definition by pushing a projection into it. Reduces materialization cost by narrowing the intermediate result.

**When to apply**: The body references only a subset of columns produced by the CTE definition.

**Why it works**: Fewer columns means less memory for materialization, less I/O, and smaller hash tables if the CTE is used in joins.

## Relational Algebra

```algebra
CTE[name, def](body)
  -> CTE[name, Project[used_cols](def)](body)
  where used_cols = referenced_columns(name, body)
    and |used_cols| < |output_cols(def)|
```

## Implementation

```rust
rw!("cte-column-pruning";
    "(cte ?name ?def ?body)" =>
    "(cte ?name (project ?used_cols ?def) ?body)"
    if columns_can_be_pruned("?name", "?def", "?body")
),
```

## Test Cases

### Positive: Only id used from wide CTE

```sql
WITH wide AS (SELECT id, name, email, age, dept FROM users)
SELECT id FROM wide WHERE id > 10;

-- Prune to: WITH wide AS (SELECT id FROM users) ...
```

### Negative: All columns used

```sql
WITH all_cols AS (SELECT id, name FROM users)
SELECT id, name FROM all_cols;

-- No pruning needed
```

## References

- Column pruning in Apache Spark Catalyst
- PostgreSQL CTE optimization (PG 12+)
