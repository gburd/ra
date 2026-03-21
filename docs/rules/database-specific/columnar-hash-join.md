# Rule: MonetDB Columnar Hash Join

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/columnar-hash-join.rra`

## Metadata

- **ID:** `monetdb-columnar-hash-join`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, hash-join, columnar, BAT
- **Authors:** "RA Contributors"


# MonetDB Columnar Hash Join

## Description

MonetDB implements hash joins in the BAT algebra by building a hash
table on the join column of the smaller relation and probing with the
larger relation.  Unlike row-store hash joins that process entire
tuples, MonetDB's join operates on individual columns (BATs) and
produces OID pairs that are used to fetch values from other columns
only when needed.

**When to apply**: An equi-join between two relations where neither
side has a pre-existing ordered index on the join column.

**Why it works**: Operating on dense, type-homogeneous BAT columns
enables SIMD-accelerated hashing and comparison.  The late
materialization of non-join columns avoids copying unnecessary data
during the join phase.

**Database version**: MonetDB 5+

## Relational Algebra

```algebra
-- Columnar hash join on BATs
OID_pairs = hashjoin(orders.cust_id, customers.id)
result = project(OID_pairs, [orders.total, customers.name])
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-columnar-hash-join";
    "(join (= ?left_col ?right_col) ?left ?right)" =>
    "(bat-hash-join ?left_col ?right_col ?left ?right)"
    if is_database("monetdb")
    if is_equi_join_columns("?left_col", "?right_col")
),
```

## Preconditions

```rust
fn applicable(
    join: &Join,
) -> bool {
    join.is_equi_join()
    && !join.smaller_side().has_ordered_index(
        join.smaller_columns()
    )
}
```

**Restrictions:**
- Only equi-joins; theta joins use nested-loop or band-join
- Hash table must fit in memory (or spill to disk via partitioning)
- For ordered columns, merge join may be preferred

## Cost Model

```rust
fn estimated_benefit(
    build_rows: f64,
    probe_rows: f64,
) -> f64 {
    let nlj_cost = build_rows * probe_rows * 0.001;
    let hash_cost = build_rows * 0.005 + probe_rows * 0.001;
    nlj_cost - hash_cost
}
```

**Typical benefit**: 10-1000x over nested-loop for medium-to-large
joins.

## Test Cases

```sql
-- Positive: equi-join without ordered index
SELECT o.total, c.name
FROM orders o JOIN customers c ON o.cust_id = c.id;
-- Hash join on cust_id column BAT
```

```sql
-- Negative: merge join preferred for sorted columns
SELECT * FROM sorted_a a JOIN sorted_b b ON a.key = b.key;
-- Both sides sorted; merge join is O(n+m) without hash overhead
```

## References

MonetDB: "MonetDB/X100: Hyper-Pipelining Query Execution" (CIDR 2005)
Source: monetdb5/modules/kernel/bat5.c, `BATjoin()`
