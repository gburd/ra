# Rule: Derive TopN from Window Function

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/derive-topn-from-window.rra`

## Metadata

- **ID:** `tidb-derive-topn-from-window`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** distributed, window-function, topn, optimization, pushdown, tidb
- **Authors:** "RA Contributors"


# Derive TopN from Window Function

## Description

Derives a TopN operator from a window function combined with a filter.
When a query uses ROW_NUMBER(), RANK(), or DENSE_RANK() with a
subsequent filter (e.g., WHERE rn <= 10), the optimizer recognizes this
pattern and inserts a TopN operator below the window function. This
avoids computing the window function for all rows when only the top-k
are needed.

**When to apply**: A window function (ROW_NUMBER, RANK, DENSE_RANK)
is followed by a filter on the window function's output column, and
the filter is a <= or < comparison with a constant.

**Why it works**: Without this optimization, the window function
computes over all rows, and the filter discards most results. With the
derived TopN, each partition only materializes the top-k rows before
the window function computes, dramatically reducing memory and CPU
usage.

## Relational Algebra

```algebra
sigma[rn <= k](
    Window[ROW_NUMBER() OVER (PARTITION BY p ORDER BY o) AS rn](R))
  -> sigma[rn <= k](
       Window[ROW_NUMBER() OVER (PARTITION BY p ORDER BY o) AS rn](
         PartitionTopN[k, p, o](R)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derive-topn-from-window";
    "(filter (le ?wincol ?k)
        (window [(row_number (partition_by ?p) (order_by ?o)
                    (as ?wincol))]
            ?input))" =>
    "(filter (le ?wincol ?k)
        (window [(row_number (partition_by ?p) (order_by ?o)
                    (as ?wincol))]
            (partition_topn ?k ?p ?o ?input)))"
    if is_constant("?k")
),
```

## Preconditions

```rust
fn applicable(
    window_fn: &WindowFunction,
    filter: &Expression,
) -> bool {
    // Window function must be ROW_NUMBER, RANK, or DENSE_RANK
    matches!(window_fn, RowNumber | Rank | DenseRank)
    // Filter must be <= or < on the window column
    && filter.is_upper_bound_on(window_fn.output_col())
    // Upper bound must be a constant
    && filter.bound_value().is_constant()
    // For RANK/DENSE_RANK, the TopN limit must account for ties
    && (window_fn == RowNumber
        || limit_accounts_for_ties(filter, window_fn))
}
```

**Restrictions:**
- For RANK and DENSE_RANK, ties mean more than k rows may share
  the same rank, so the TopN must be approximate (over-fetch)
- NTILE and other window functions are not supported
- The filter must directly reference the window function output
- Frame specifications that don't cover the full partition may
  prevent this optimization
- The TopN is per-partition, not global

## Cost Model

```rust
fn derived_topn_benefit(
    total_rows: f64,
    num_partitions: f64,
    k: f64,
    sort_cost_per_row: f64,
) -> f64 {
    let rows_per_partition = total_rows / num_partitions;
    // Without TopN: sort entire partition
    let without = total_rows * rows_per_partition.log2()
        * sort_cost_per_row;
    // With TopN: only maintain heap of k per partition
    let with = total_rows * k.log2() * sort_cost_per_row;
    without - with
}
```

## Test Cases

```sql
-- Positive: ROW_NUMBER with top-3 filter
SELECT * FROM (
    SELECT *, ROW_NUMBER() OVER (
        PARTITION BY department_id ORDER BY salary DESC
    ) AS rn
    FROM employees
) t WHERE rn <= 3;

-- Derived TopN: only top-3 per department are materialized
-- before ROW_NUMBER computes
```

```sql
-- Positive: RANK with top-10 filter
SELECT * FROM (
    SELECT *, RANK() OVER (ORDER BY score DESC) AS rnk
    FROM students
) t WHERE rnk <= 10;
-- TopN may over-fetch slightly to handle ties
```

```sql
-- Negative: filter is not an upper bound
SELECT * FROM (
    SELECT *, ROW_NUMBER() OVER (ORDER BY id) AS rn
    FROM items
) t WHERE rn >= 10;
-- Lower bound filter; cannot derive TopN
```

```sql
-- Negative: window function is not ranking
SELECT * FROM (
    SELECT *, SUM(amount) OVER (ORDER BY date) AS running_total
    FROM transactions
) t WHERE running_total < 1000;
-- SUM window function; not a ranking function
```

## References

TiDB: pkg/planner/core/rule_derive_topn_from_window.go:24 - DeriveTopNFromWindow (commit e2184a2)
TiDB: logical plan DeriveTopN method
Graefe, "The Cascades Framework for Query Optimization" (1995)
