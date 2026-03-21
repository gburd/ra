# Rule: MySQL ORDER BY Index Optimization

**Category:** database-specific/mysql
**File:** `rules/database-specific/mysql/order-by-optimization.rra`

## Metadata

- **ID:** `mysql-order-by-optimization`
- **Version:** "1.0.0"
- **Databases:** mysql
- **Tags:** database-specific, mysql, order-by, index, filesort, avoidance
- **Authors:** "RA Contributors"


# MySQL ORDER BY Index Optimization

## Description

MySQL avoids filesort when an index satisfies the ORDER BY clause.
If the query's ORDER BY columns match a prefix of an available index
(in the same direction), MySQL reads rows in index order and skips
the sort entirely.  For DESC ordering, MySQL uses backward index
scans (MySQL 8.0+ supports DESC indexes natively).

**When to apply**: The ORDER BY columns match a prefix of an
available index, and the query does not require a filesort for other
reasons (e.g., GROUP BY on different columns).

**Why it works**: Filesort is O(n log n) and requires memory or
temp files.  Reading in index order is O(n) with no extra memory.

**Database version**: MySQL 5.0+ (ASC), MySQL 8.0+ (DESC indexes)

## Relational Algebra

```algebra
-- Before: scan + filesort
sort[created_at DESC](scan(events))

-- After: backward index scan
index_scan[idx_created, direction=DESC](events)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mysql-order-by-index";
    "(sort ?order_cols (scan ?table))" =>
    "(index-ordered-scan ?table ?order_cols)"
    if is_database("mysql")
    if has_matching_index("?table", "?order_cols")
),
```

## Preconditions

```rust
fn applicable(
    order_cols: &[OrderColumn],
    table: &Table,
) -> bool {
    table.has_index_matching_order(order_cols)
}
```

**Restrictions:**
- Mixed ASC/DESC requires MySQL 8.0 DESC indexes
- Cannot avoid sort if ORDER BY references expressions not in index
- LIMIT + ORDER BY can use index to avoid full sort even without
  covering all ORDER BY columns

## Cost Model

```rust
fn estimated_benefit(
    total_rows: f64,
    has_limit: bool,
    limit_value: f64,
) -> f64 {
    let sort_cost = total_rows * total_rows.log2() * 0.001;
    if has_limit {
        sort_cost - limit_value * 0.005
    } else {
        sort_cost
    }
}
```

**Typical benefit**: Eliminates filesort entirely. For LIMIT queries,
reads only needed rows.

## Test Cases

```sql
-- Positive: ORDER BY matches index
CREATE INDEX idx_ts ON events(created_at);
SELECT * FROM events ORDER BY created_at DESC LIMIT 10;
-- Backward index scan, reads only 10 rows
```

```sql
-- Negative: ORDER BY on non-indexed column
SELECT * FROM events ORDER BY payload_size;
-- No matching index; filesort required
```

## References

MySQL: "ORDER BY Optimization" in MySQL Reference Manual
MySQL: EXPLAIN shows "Using filesort" when sort is needed
Source: sql/sql_optimizer.cc, `test_if_skip_sort_order()`
