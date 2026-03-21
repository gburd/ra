# Rule: Window Function Filter Pushdown

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-filter-pushdown.rra`

## Metadata

- **ID:** `window-filter-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, mssql, oracle
- **Tags:** window, filter, pushdown
- **Authors:** "RA Contributors"


# Window Function Filter Pushdown

## Description

Push filters that do not reference window function output columns below the Window operator. This reduces the number of rows processed by the window function.

**When to apply**: Filter predicate references only base columns, not window function results.

## Relational Algebra

```algebra
Filter[p](Window[fns](input))
  -> Window[fns](Filter[p](input))
  where !references_window_output(p, fns)
```

## Implementation

```rust
rw!("window-filter-pushdown";
    "(filter ?pred (window ?fns ?input))" =>
    "(window ?fns (filter ?pred ?input))"
    if pred_independent_of_window("?pred", "?fns")
),
```

## Test Cases

### Positive: Filter on base column

```sql
SELECT *, ROW_NUMBER() OVER (ORDER BY id) as rn
FROM orders
WHERE status = 'shipped';

-- Push status filter below window
```

### Negative: Filter on window result

```sql
SELECT * FROM (
    SELECT *, ROW_NUMBER() OVER (ORDER BY id) as rn FROM orders
) t WHERE rn <= 10;

-- Cannot push rn filter below window
```

## References

- Window function optimization in SQL Server
- Filter pushdown in Apache Calcite
