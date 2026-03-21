# Rule: Window Sort Elimination

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-sort-elimination.rra`

## Metadata

- **ID:** `window-sort-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, mssql, oracle
- **Tags:** window, sort, elimination
- **Authors:** "RA Contributors"


# Window Sort Elimination

## Description

When a Sort node immediately follows a Window operator and the sort order matches the window's ORDER BY, eliminate the redundant sort. The window function already produces output sorted by its ORDER BY clause.

**When to apply**: Sort keys match the window function ORDER BY.

## Relational Algebra

```algebra
Sort[keys](Window[fns with order_by=keys](input))
  -> Window[fns](input)
  where keys == window_order_by(fns)
```

## Implementation

```rust
rw!("window-sort-elimination";
    "(sort ?keys (window ?fns ?input))" =>
    "(window ?fns ?input)"
    if sort_matches_window_order("?keys", "?fns")
),
```

## Test Cases

### Positive: Redundant sort after window

```sql
SELECT id, ROW_NUMBER() OVER (ORDER BY created_at) as rn
FROM events
ORDER BY created_at;

-- Sort matches window ORDER BY; eliminate
```

### Negative: Different sort order

```sql
SELECT id, ROW_NUMBER() OVER (ORDER BY created_at) as rn
FROM events
ORDER BY id;

-- Different sort key; keep
```

## References

- Sort avoidance in PostgreSQL planner
- Order exploitation in window functions
