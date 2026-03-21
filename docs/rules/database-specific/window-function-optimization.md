# Rule: MySQL Window Function Optimization

**Category:** database-specific/mysql
**File:** `rules/database-specific/mysql/window-function-optimization.rra`

## Metadata

- **ID:** `mysql-window-function-optimization`
- **Version:** "1.0.0"
- **Databases:** mysql
- **Tags:** database-specific, mysql, window-function, frame, buffering
- **Authors:** "RA Contributors"


# MySQL Window Function Optimization

## Description

MySQL 8.0 introduced native window function support.  The optimizer
groups window functions that share the same PARTITION BY and ORDER BY
into a single sort pass and computes them together.  Frame-aware
aggregates (e.g., running SUM) use optimized frame buffer management
to avoid re-scanning the frame for each row.

**When to apply**: A query has multiple window functions that share
partition and order specifications.

**Why it works**: Without grouping, each window function would require
a separate sort.  Sharing the sort amortizes the O(n log n) cost
across all window functions in the group.  Incremental frame
computation reduces per-row work from O(frame_size) to O(1).

**Database version**: MySQL 8.0+

## Relational Algebra

```algebra
-- Before: separate sorts per window function
window[ROW_NUMBER() OVER (PARTITION BY dept ORDER BY sal)](
  window[SUM(sal) OVER (PARTITION BY dept ORDER BY sal)](
    sort[dept, sal](scan(employees))))

-- After: single sort, shared window pass
window[ROW_NUMBER(), SUM(sal)
    OVER (PARTITION BY dept ORDER BY sal)](
  sort[dept, sal](scan(employees)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mysql-window-function-merge";
    "(window ?func1 ?spec
        (window ?func2 ?spec ?rel))" =>
    "(window-merged (?func1 ?func2) ?spec ?rel)"
    if is_database("mysql")
    if same_window_spec("?spec")
),
```

## Preconditions

```rust
fn applicable(
    windows: &[WindowFunction],
) -> bool {
    windows.len() >= 2
    && windows.windows(2).all(|pair| {
        pair[0].partition_by() == pair[1].partition_by()
        && pair[0].order_by() == pair[1].order_by()
    })
}
```

**Restrictions:**
- Window functions with different PARTITION BY or ORDER BY cannot
  share a sort
- Frame specifications can differ within the same window group
- ROWS BETWEEN UNBOUNDED PRECEDING uses running aggregation;
  RANGE BETWEEN requires different logic

## Cost Model

```rust
fn estimated_benefit(
    total_rows: f64,
    num_merged_windows: usize,
) -> f64 {
    // Save (n-1) sort operations
    let sorts_saved = (num_merged_windows - 1) as f64;
    sorts_saved * total_rows * total_rows.log2() * 0.001
}
```

**Typical benefit**: 2-5x for queries with multiple window functions
on the same partition/order.

## Test Cases

```sql
-- Positive: two windows with same spec
SELECT dept_id,
    ROW_NUMBER() OVER (PARTITION BY dept_id ORDER BY salary),
    SUM(salary) OVER (PARTITION BY dept_id ORDER BY salary)
FROM employees;
-- Single sort pass for both functions
```

```sql
-- Negative: different partition specs
SELECT
    ROW_NUMBER() OVER (PARTITION BY dept_id ORDER BY salary),
    RANK() OVER (PARTITION BY location ORDER BY salary)
FROM employees;
-- Requires two separate sorts
```

## References

MySQL: "Window Function Optimization" in MySQL 8.0 Reference Manual
Source: sql/sql_window.cc, `Window::setup_windows()`
