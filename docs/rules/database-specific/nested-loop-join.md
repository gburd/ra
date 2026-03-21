# Rule: Apache Derby Index Nested-Loop Join

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/nested-loop-join.rra`

## Metadata

- **ID:** `derby-nested-loop-join`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, nested-loop, index, join
- **Authors:** "RA Contributors"


# Apache Derby Index Nested-Loop Join

## Description

Derby's primary join strategy is nested-loop join with index lookup.
For each row from the outer table, Derby looks up matching rows in
the inner table using an index.  This is the default join strategy
when an index exists on the inner table's join column.

**When to apply**: An equi-join or range join has a usable index on
the inner table's join column.

**Why it works**: Index lookup reduces the inner table access from
O(m) per outer row to O(log m) per outer row (B-tree depth).
Combined with Derby's cost-based optimizer choosing the smaller table
as outer, this is efficient for selective joins.

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Index nested-loop join
for each row r in outer:
    lookup(inner, index, r.join_key)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-index-nlj";
    "(join ?pred ?outer ?inner)" =>
    "(index-nested-loop ?pred ?outer ?inner)"
    if is_database("derby")
    if has_index("?inner", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    join: &Join,
    inner: &Table,
) -> bool {
    inner.has_usable_index(join.inner_columns())
}
```

**Restrictions:**
- Requires index on inner table's join columns
- Outer table should have fewer rows for efficiency
- Falls back to hash join when no index is available

## Cost Model

```rust
fn estimated_benefit(
    outer_rows: f64,
    index_depth: f64,
    rows_per_lookup: f64,
) -> f64 {
    let nlj_cost = outer_rows * (index_depth + rows_per_lookup)
        * 0.01;
    nlj_cost
}
```

**Typical benefit**: Standard join strategy; 10-100x over full scan
when inner index is selective.

## Test Cases

```sql
-- Positive: index on inner join column
CREATE INDEX idx_fk ON items(order_id);
SELECT * FROM orders o JOIN items i ON o.id = i.order_id;
-- Index NLJ: for each order, lookup items by index
```

```sql
-- Negative: no index on join column
SELECT * FROM t1 JOIN t2 ON t1.a = t2.b;
-- No index on t2.b; hash join or full NLJ
```

## References

Apache Derby: Developer's Guide, "Join Strategies"
Source: org.apache.derby.impl.sql.compile.NestedLoopJoinStrategy
