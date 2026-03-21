# Rule: Apache Derby Sort Avoidance via Index

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/sort-avoidance.rra`

## Metadata

- **ID:** `derby-sort-avoidance`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, sort, avoidance, order-by, index
- **Authors:** "RA Contributors"


# Apache Derby Sort Avoidance via Index

## Description

Derby avoids explicit sort operations when an index provides rows in
the required ORDER BY sequence.  The optimizer compares the ORDER BY
columns with available index columns and chooses an index scan that
produces rows in the desired order.

**When to apply**: The ORDER BY columns match a prefix of an available
index on the same table.

**Why it works**: An explicit sort is O(n log n) and requires
temporary storage.  An ordered index scan is O(n) with no additional
memory beyond the scan buffer.

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Before: scan + sort
sort[created_at](scan(events))

-- After: ordered index scan
index_scan[idx_created](events)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-sort-avoidance";
    "(sort ?order (scan ?table))" =>
    "(index-ordered-scan ?table ?order)"
    if is_database("derby")
    if has_matching_index("?table", "?order")
),
```

## Preconditions

```rust
fn applicable(
    order: &[OrderColumn],
    table: &Table,
) -> bool {
    table.indexes().iter().any(|idx| {
        idx.matches_order(order)
    })
}
```

**Restrictions:**
- Index must match ORDER BY direction (ASC/DESC)
- Derby does not support mixed-direction indexes in a single scan
- Sort avoidance may not be chosen if the index scan is much more
  expensive than table scan + sort

## Cost Model

```rust
fn estimated_benefit(
    total_rows: f64,
) -> f64 {
    let sort_cost = total_rows * total_rows.log2() * 0.001;
    sort_cost
}
```

**Typical benefit**: Eliminates sort step; significant for large
result sets.

## Test Cases

```sql
-- Positive: ORDER BY matches index
CREATE INDEX idx_ts ON events(created_at);
SELECT * FROM events ORDER BY created_at;
-- Index scan in order; no sort needed
```

```sql
-- Negative: ORDER BY on non-indexed column
SELECT * FROM events ORDER BY payload;
-- No matching index; sort required
```

## References

Apache Derby: Tuning Guide, "Sort Avoidance"
Source: org.apache.derby.impl.sql.compile.OrderByList
