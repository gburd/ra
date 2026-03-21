# Rule: MySQL Batched Key Access (BKA)

**Category:** database-specific/mysql
**File:** `rules/database-specific/mysql/batched-key-access.rra`

## Metadata

- **ID:** `mysql-batched-key-access`
- **Version:** "1.0.0"
- **Databases:** mysql
- **Tags:** database-specific, mysql, join, BKA, MRR, batched
- **Authors:** "RA Contributors"


# MySQL Batched Key Access (BKA)

## Description

Batched Key Access combines the join buffer with Multi-Range Read to
convert random index lookups into sequential disk reads.  For each
batch of rows from the driving table, BKA collects the join keys,
sorts them into rowid order, and issues a single multi-range read to
the inner table's storage engine.

**When to apply**: A nested-loop join performs index lookups on the
inner table and the inner table is large enough that random I/O
dominates.

**Why it works**: Random I/O on spinning disks is 100x slower than
sequential I/O.  By sorting keys before lookup, BKA converts random
reads into sequential reads.  Even on SSDs, batching reduces per-row
overhead.

**Database version**: MySQL 5.6+

## Relational Algebra

```algebra
-- Before: nested-loop with random index lookups
outer NLJ[o.key = i.key] inner

-- After: batched key access with sorted lookups
outer BKA-join[o.key = i.key, batch_size=256] inner
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mysql-batched-key-access";
    "(nested-loop-join ?pred ?outer ?inner)" =>
    "(bka-join ?pred ?outer ?inner)"
    if is_database("mysql")
    if inner_has_index("?inner", "?pred")
    if batch_beneficial("?outer", "?inner")
),
```

## Preconditions

```rust
fn applicable(
    join: &Join,
    inner: &Table,
) -> bool {
    join.is_equi_join()
    && inner.has_index_on(join.inner_columns())
    && join.outer_estimate() > 100
}
```

**Restrictions:**
- Requires `optimizer_switch='batched_key_access=on'` (off by default)
- Requires MRR to also be enabled
- Only works with InnoDB and MyISAM
- Join buffer size limits batch size (`join_buffer_size`)

## Cost Model

```rust
fn estimated_benefit(
    outer_rows: f64,
    random_io_cost: f64,
    sequential_io_cost: f64,
) -> f64 {
    let random_cost = outer_rows * random_io_cost;
    let batch_count = (outer_rows / 256.0).ceil();
    let sorted_cost = batch_count * 256.0 * sequential_io_cost;
    random_cost - sorted_cost
}
```

**Typical benefit**: 5-20x for disk-bound joins on HDD. 1.5-3x on SSD.

## Test Cases

```sql
-- Positive: join with index on inner table
SELECT * FROM orders o
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE o.o_orderdate > '2023-01-01';
-- BKA batches order keys, sorts, reads lineitem sequentially
```

```sql
-- Negative: inner table has no index
SELECT * FROM t1 JOIN t2 ON t1.a = t2.b;
-- No index on t2.b; BKA cannot apply
```

## References

MySQL: "Batched Key Access Joins" in MySQL Reference Manual
MySQL: `optimizer_switch` flag `batched_key_access=on`
Source: sql/sql_join_buffer.cc
