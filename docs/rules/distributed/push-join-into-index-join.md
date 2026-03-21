# Rule: Push Join into Index Join

**Category:** distributed/distributed-joins
**File:** `rules/distributed/distributed-joins/push-join-into-index-join.rra`

## Metadata

- **ID:** `push-join-into-index-join`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** distributed, join, index-join, lookup, cardinality-reduction
- **Authors:** "RA Contributors"


# Push Join into Index Join

## Description

Pushes an InnerJoin below an IndexJoin by converting the IndexJoin into
a LookupJoin. When an InnerJoin reduces cardinality significantly, it
is cheaper to perform the join first (using the index scan output) and
then do the lookup (index join) only for the surviving rows.

**When to apply**: The left input of an InnerJoin is an IndexJoin, the
right input has no outer columns, and the join condition only references
columns from the index scan input and the right side.

**Why it works**: An IndexJoin fetches additional columns from the
primary index for every row. If a subsequent join eliminates most rows,
those primary index lookups were wasted. By pushing the join below the
IndexJoin, only the rows that survive the join require the primary
index lookup.

## Relational Algebra

```algebra
InnerJoin[cond](IndexJoin(IndexScan(T), T), R)
  -> LookupJoin(
       InnerJoin[cond](IndexScan(T), R),
       T
     )
  where cond references only IndexScan(T).cols and R.cols
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("push-join-into-index-join";
    "(join inner
        (index_join ?index_input ?index_private)
        ?right
        ?on ?join_private)" =>
    "(lookup_join
        (join inner ?index_input ?right ?on ?join_private)
        ?index_private)"
    if no_outer_cols("?right")
    if filters_bound_by("?on", "?index_input", "?right")
    if no_join_hints("?join_private")
),
```

## Preconditions

```rust
fn applicable(
    index_input: &RelExpr,
    right: &RelExpr,
    on: &FiltersExpr,
    join_private: &JoinPrivate,
) -> bool {
    // Right input must not have outer columns
    !right.has_outer_cols()
    // ON condition must only reference index scan cols and right cols
    && on.is_bound_by(
        &index_input.output_cols().union(&right.output_cols()))
    // No join hints that would prevent reordering
    && join_private.no_hints()
}
```

**Restrictions:**
- Only applies to InnerJoin (LeftJoin could theoretically work but
  no use case has been found)
- The join condition must not reference columns that come from the
  index join (i.e., columns not in the secondary index)
- The resulting LookupJoin fetches the full row from the primary
  index for each surviving joined row

## Cost Model

```rust
fn push_join_benefit(
    index_scan_rows: f64,
    join_selectivity: f64,
    primary_lookup_cost: f64,
) -> f64 {
    let original = index_scan_rows * primary_lookup_cost;
    let optimized =
        index_scan_rows * join_selectivity * primary_lookup_cost;
    original - optimized
}
```

**Typical benefit**: If the join eliminates 90% of rows, 90% of
primary index lookups are avoided.

## Test Cases

```sql
-- Positive: join reduces cardinality before index lookup
-- t has secondary index on (a) that doesn't cover column b
-- r is a small reference table
SELECT t.a, t.b, r.name
FROM t JOIN r ON t.a = r.key
WHERE t.a BETWEEN 1 AND 100;

-- Without optimization:
-- InnerJoin(t.a = r.key)
--   IndexJoin(t)              -- fetches b for all 100 rows
--     IndexScan(t, idx_a)     -- 100 rows
--   Scan(r)                   -- 10 rows

-- With optimization:
-- LookupJoin(t)               -- fetches b for ~10 surviving rows
--   InnerJoin(t.a = r.key)
--     IndexScan(t, idx_a)     -- 100 rows
--     Scan(r)                 -- 10 rows
```

```sql
-- Negative: join condition references a looked-up column
SELECT t.a, t.b, r.name
FROM t JOIN r ON t.b = r.key;  -- b comes from index join
-- Cannot push join below index join because b is not in secondary index
```

## References

CockroachDB: pkg/sql/opt/xform/rules/join.opt:436 - PushJoinIntoIndexJoin (commit 51e808c)
CockroachDB: pkg/sql/opt/xform/join_funcs.go - ConvertIndexToLookupJoinPrivate
