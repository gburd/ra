# Rule: Window Limit Pushdown

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-limit-pushdown.rra`

## Metadata

- **ID:** `window-limit-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, mssql, oracle
- **Tags:** window, limit, pushdown, top-n
- **Authors:** "RA Contributors"


# Window Limit Pushdown

## Description

When a ROW_NUMBER window function is filtered to rn <= N (top-N pattern), push the limit into the input sort to use a top-N heap sort instead of a full sort.

**When to apply**: Filter on ROW_NUMBER() result with <= or < comparison.

## Relational Algebra

```algebra
Filter[rn <= N](Window[ROW_NUMBER() ORDER BY keys as rn](input))
  -> Window[ROW_NUMBER() ORDER BY keys as rn](TopN[N, keys](input))
```

## Implementation

```rust
rw!("window-limit-pushdown";
    "(filter (<= ?rn_col ?n) (window ?fns ?input))" =>
    "(filter (<= ?rn_col ?n) (window ?fns (limit ?n 0 (sort ?keys ?input))))"
    if is_row_number_filter("?rn_col", "?fns")
    if extract_window_order("?fns", "?keys")
),
```

## Test Cases

### Positive: Top-N per group

```sql
SELECT * FROM (
    SELECT *, ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary DESC) as rn
    FROM employees
) t WHERE rn <= 3;

-- Push top-3 limit into each partition
```

## References

- Top-N query optimization (Carey & Kossmann, 1997)
- Row-limited window functions in PostgreSQL
