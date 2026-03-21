# Rule: MySQL Block Nested-Loop Join

**Category:** database-specific/mysql
**File:** `rules/database-specific/mysql/join-buffer-bnl.rra`

## Metadata

- **ID:** `mysql-join-buffer-bnl`
- **Version:** "1.0.0"
- **Databases:** mysql
- **Tags:** database-specific, mysql, join, block-nested-loop, buffer
- **Authors:** "RA Contributors"


# MySQL Block Nested-Loop Join

## Description

When no index is available for a join and the MySQL version is below
8.0.18 (pre-hash-join), MySQL uses Block Nested-Loop (BNL).  BNL
reads chunks of the outer table into a join buffer, then scans the
inner table once per buffer-full, comparing each inner row against all
buffered outer rows.  This reduces the number of inner table scans
from O(outer_rows) to O(outer_rows / buffer_capacity).

**When to apply**: No usable index on the inner table and MySQL
version < 8.0.18 (after which hash join replaces BNL).

**Why it works**: Amortizes inner-table scans across multiple outer
rows.  Without BNL, each outer row triggers a full inner scan.

**Database version**: MySQL 5.1-8.0.17 (replaced by hash join in 8.0.18+)

## Relational Algebra

```algebra
-- Before: simple nested-loop (no index)
outer NLJ inner

-- After: block nested-loop with buffer
outer BNL[buffer_size=256KB] inner
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mysql-join-buffer-bnl";
    "(nested-loop-join ?pred ?outer ?inner)" =>
    "(block-nested-loop ?pred ?outer ?inner)"
    if is_database("mysql")
    if mysql_version_below("8.0.18")
    if no_usable_index("?inner", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    join: &Join,
    inner: &Table,
) -> bool {
    !inner.has_usable_index(join.inner_columns())
    && join.outer_estimate() > 1
}
```

**Restrictions:**
- Superseded by hash join in MySQL 8.0.18+
- `join_buffer_size` controls buffer capacity
- Only inner joins and left outer joins supported
- Does not support full outer joins

## Cost Model

```rust
fn estimated_benefit(
    outer_rows: f64,
    inner_rows: f64,
    buffer_capacity: f64,
) -> f64 {
    let nlj_scans = outer_rows;
    let bnl_scans = (outer_rows / buffer_capacity).ceil();
    (nlj_scans - bnl_scans) * inner_rows * 0.001
}
```

**Typical benefit**: 100-1000x reduction in inner scans for large
joins without indexes.

## Test Cases

```sql
-- Positive: join without index (MySQL < 8.0.18)
SELECT * FROM t1, t2 WHERE t1.a = t2.b;
-- BNL: buffer t1 rows, scan t2 per buffer
```

```sql
-- Negative: index available
CREATE INDEX idx_b ON t2(b);
SELECT * FROM t1 JOIN t2 ON t1.a = t2.b;
-- Uses index nested-loop, not BNL
```

## References

MySQL: "Nested-Loop Join Algorithms" in MySQL Reference Manual
MySQL: `optimizer_switch` flag `block_nested_loop=on`
Source: sql/sql_executor.cc
