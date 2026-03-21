# Rule: Window Projection Pushdown

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-projection-pushdown.rra`

## Metadata

- **ID:** `window-projection-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, clickhouse
- **Tags:** window, projection, pushdown, column-pruning
- **Authors:** "RA Contributors"


# Window Projection Pushdown

## Description

Pushes column pruning below window functions to reduce the width of rows flowing through the window operator.

**When to apply**: Projection above window does not use all input columns; only keep columns needed by window function and final projection.

**Why it works**: Narrower rows reduce memory pressure during window computation (partitioning and sorting).

## Relational Algebra

```algebra
project[cols](window[W](R))
  -> project[cols](window[W](project[cols ∪ window_deps(W)](R)))
  where cols ∪ window_deps(W) ⊂ output(R)
```

## Implementation

```rust
rw!("window-projection-pushdown";
    "(project ?cols (window ?funcs ?input))" =>
    "(project ?cols (window ?funcs (project ?needed_cols ?input)))"
    if can_prune_below_window("?cols", "?funcs", "?input")
),
```

## Cost Model

```rust
fn benefit(total_cols: usize, needed_cols: usize, rows: u64) -> f64 {
    let col_ratio = needed_cols as f64 / total_cols as f64;
    (1.0 - col_ratio) * 0.3
}
```

**Typical benefit**: 10-30% for wide tables

## Test Cases

### Positive: Narrow projection over wide window

```sql
SELECT name, ROW_NUMBER() OVER (ORDER BY id) as rn
FROM users;  -- users has 20 columns

-- Only need id and name below window
```

### Negative: All columns needed

```sql
SELECT *, ROW_NUMBER() OVER (ORDER BY id) as rn
FROM users;

-- SELECT * needs all columns
```

## References

- DuckDB: Column pruning through window operators
- PostgreSQL: Window function optimization
