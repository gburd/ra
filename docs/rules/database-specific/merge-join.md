# Rule: MonetDB Ordered Merge Join

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/merge-join.rra`

## Metadata

- **ID:** `monetdb-merge-join`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, merge-join, sorted, BAT, ordered
- **Authors:** "RA Contributors"


# MonetDB Ordered Merge Join

## Description

When both join columns are physically ordered (sorted BATs), MonetDB
uses a merge join that scans both columns simultaneously with two
cursors.  This avoids building a hash table and produces output in
sorted order, which benefits downstream operations.

**When to apply**: Both join columns are sorted (the BAT's tail
column is ordered) and the join is an equi-join.

**Why it works**: Merge join on sorted data is O(n+m) with zero hash
table overhead and excellent cache behavior (sequential access on
both sides).  The sorted output eliminates a subsequent sort if
ORDER BY matches.

**Database version**: MonetDB 5+

## Relational Algebra

```algebra
-- Before: hash join on sorted columns
hash-join[a.key = b.key](sorted_a, sorted_b)

-- After: merge join (no hash table)
merge-join[a.key = b.key](sorted_a, sorted_b)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-merge-join";
    "(hash-join ?pred ?left ?right)" =>
    "(merge-join ?pred ?left ?right)"
    if is_database("monetdb")
    if both_sorted_on_join_key("?left", "?right", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    left: &Bat,
    right: &Bat,
    pred: &JoinPredicate,
) -> bool {
    left.is_sorted_on(pred.left_column())
    && right.is_sorted_on(pred.right_column())
    && pred.is_equi_join()
}
```

**Restrictions:**
- Requires both sides to be physically sorted on the join key
- If only one side is sorted, MonetDB may sort the other and then
  merge (cost depends on sort cost vs hash cost)
- Not applicable to non-equi joins

## Cost Model

```rust
fn estimated_benefit(
    left_rows: f64,
    right_rows: f64,
) -> f64 {
    let hash_cost = left_rows.min(right_rows) * 0.005
        + left_rows.max(right_rows) * 0.001;
    let merge_cost = (left_rows + right_rows) * 0.0008;
    hash_cost - merge_cost
}
```

**Typical benefit**: 1.5-3x over hash join when both sides are
already sorted.

## Test Cases

```sql
-- Positive: join on primary key (both sorted)
SELECT * FROM orders o JOIN lineitem l
    ON o.o_orderkey = l.l_orderkey;
-- Both tables clustered on orderkey; merge join
```

```sql
-- Negative: unsorted join columns
SELECT * FROM t1 JOIN t2 ON t1.random_col = t2.random_col;
-- Neither side sorted; hash join preferred
```

## References

MonetDB: BAT algebra merge join implementation
Source: gdk/gdk_join.c, `mergejoin()`
