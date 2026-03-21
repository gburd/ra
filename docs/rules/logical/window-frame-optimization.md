# Rule: Window Frame Optimization

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-frame-optimization.rra`

## Metadata

- **ID:** `window-frame-optimization`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, mssql, oracle
- **Tags:** window, frame, optimization
- **Authors:** "RA Contributors"


# Window Frame Optimization

## Description

Simplifies window frame specifications. For ranking functions (ROW_NUMBER, RANK, DENSE_RANK), the frame clause is irrelevant and can be removed. For running aggregates with ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW, the default frame can be used.

**When to apply**: Window function has an unnecessary or default frame specification.

## Relational Algebra

```algebra
Window[ROW_NUMBER() OVER (... ROWS BETWEEN ...)](input)
  -> Window[ROW_NUMBER() OVER (... )  -- no frame](input)
  where is_ranking_function(fn)
```

## Implementation

```rust
rw!("window-frame-optimization";
    "(window-expr ?fn ?arg ?part ?order (frame ?mode ?start ?end))" =>
    "(window-expr ?fn ?arg ?part ?order none)"
    if is_ranking_function("?fn")
),
```

## Test Cases

### Positive: Ranking with unnecessary frame

```sql
SELECT ROW_NUMBER() OVER (
    ORDER BY id
    ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
) FROM users;

-- Frame is unnecessary for ROW_NUMBER; remove
```

### Negative: Aggregate with meaningful frame

```sql
SELECT SUM(amount) OVER (
    ORDER BY date
    ROWS BETWEEN 3 PRECEDING AND CURRENT ROW
) FROM transactions;

-- Frame is meaningful; keep
```

## References

- SQL standard window frame defaults
- Window function optimization in analytical databases
