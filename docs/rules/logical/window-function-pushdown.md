# Rule: Window Function Filter Pushdown

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-function-pushdown.rra`

## Metadata

- **ID:** `window-function-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, clickhouse
- **Tags:** window, pushdown, filter, optimization
- **Authors:** "RA Contributors"


# Window Function Filter Pushdown

## Description

Pushes filters below window function operators when the filter predicate does not reference window function output columns.

**When to apply**: Filter above a Window node references only base columns.

**Why it works**: Filtering before window computation reduces the number of rows the window function must process.

## Relational Algebra

```algebra
filter[P](window[W](R))
  -> window[W](filter[P](R))
  where columns(P) ∩ output_cols(W) = ∅
```

## Implementation

```rust
rw!("window-filter-pushdown";
    "(filter ?pred (window ?funcs ?input))" =>
    "(window ?funcs (filter ?pred ?input))"
    if no_window_col_refs("?pred", "?funcs")
),
```

## Cost Model

```rust
fn benefit(input_rows: u64, selectivity: f64) -> f64 {
    let rows_before = input_rows as f64;
    let rows_after = rows_before * selectivity;
    let window_cost_saved = (rows_before - rows_after) * 0.05;
    window_cost_saved / (rows_before * 0.05)
}
```

**Typical benefit**: 20-60% for selective filters

## Test Cases

### Positive: Filter on base column

```sql
SELECT *, ROW_NUMBER() OVER (ORDER BY id) as rn
FROM orders WHERE status = 'active';

-- Push status filter below window
```

### Negative: Filter on window output

```sql
SELECT * FROM (
  SELECT *, ROW_NUMBER() OVER (ORDER BY id) as rn FROM orders
) t WHERE rn <= 10;

-- Cannot push: rn is a window output
```

## References

- PostgreSQL: Window function optimization
- DuckDB: Parallel window function execution
