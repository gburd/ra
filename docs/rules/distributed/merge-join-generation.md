# Rule: Merge Join Generation from Interesting Orderings

**Category:** distributed/distributed-joins
**File:** `rules/distributed/distributed-joins/merge-join-generation.rra`

## Metadata

- **ID:** `merge-join-generation`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** distributed, join, merge, ordering, physical
- **Authors:** "RA Contributors"


# Merge Join Generation from Interesting Orderings

## Description

Generates MergeJoin operators for equi-joins by exploiting interesting
orderings from the input relations. If an input already provides an
ordering on the join equality columns (e.g., from an index scan), a
merge join avoids the cost of building a hash table. In distributed
settings, merge joins can also stream results without materializing
the entire input.

**When to apply**: An equi-join has at least one equality column, and
at least one side provides (or can cheaply provide) an ordering on
the equality columns.

**Why it works**: Merge join runs in O(n + m) time when both inputs
are sorted. If indexes already provide the sort order, the sort cost
is zero. In distributed systems, merge join enables pipelining since
rows are processed in order without full materialization.

## Relational Algebra

```algebra
Join[L.k1 = R.k2](L, R)
  -> MergeJoin[L.k1 = R.k2](
       Sort[k1](L) or L if already ordered,
       Sort[k2](R) or R if already ordered
     )
  where interesting_orderings(L) covers k1
     or interesting_orderings(R) covers k2
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("generate-merge-join";
    "(join ?type ?left ?right ?on ?private)" =>
    "(merge_join ?type ?left ?right ?on
        (merge_ordering ?left_eq ?right_eq))"
    if has_equality_columns("?on")
    if has_interesting_ordering("?left", "?on")
),
```

## Preconditions

```rust
fn applicable(
    left: &RelExpr,
    right: &RelExpr,
    on: &FiltersExpr,
    join_private: &JoinPrivate,
) -> bool {
    // Merge join must not be disabled by hints
    !join_private.flags.has(DisallowMergeJoin)
    // Must have at least one equality column pair
    && extract_join_equality_columns(on).len() > 0
    // At least one side must have an interesting ordering on eq cols
    && (has_interesting_ordering_on_eq_cols(left, on)
        || has_interesting_ordering_on_eq_cols(right, on)
        // Or all eq cols are constant (trivial ordering)
        || eq_cols_are_constant(left, on))
}
```

**Restrictions:**
- Requires equality join predicates (cannot handle theta joins)
- Both sides must ultimately be sorted on equality columns; if
  neither side has a natural ordering, the sort cost may exceed
  hash join cost
- In distributed settings, merge join still requires co-location
  or repartitioning of both inputs by the join key
- When join reorder limit is 0 or hints are present, orderings
  from both sides are considered (join won't be commuted)

## Cost Model

```rust
fn merge_join_cost(
    left_rows: f64,
    right_rows: f64,
    left_sort_needed: bool,
    right_sort_needed: bool,
    row_size: f64,
) -> f64 {
    let mut cost = (left_rows + right_rows) * row_size * MERGE_COMPARE;
    if left_sort_needed {
        cost += left_rows * left_rows.log2() * SORT_COST;
    }
    if right_sort_needed {
        cost += right_rows * right_rows.log2() * SORT_COST;
    }
    cost
}
```

**Typical benefit**: When an index provides the sort order, merge join
eliminates the O(n) hash table build cost and enables streaming
execution with bounded memory.

## Test Cases

```sql
-- Positive: both sides have index on join key
-- orders has index on (customer_id), customers has PK on (id)
SELECT o.*, c.name
FROM orders o JOIN customers c ON o.customer_id = c.id;

-- Plan: MergeJoin(o.customer_id = c.id)
--   IndexScan(orders, idx_customer_id)  -- ordered by customer_id
--   Scan(customers)                     -- ordered by id (PK)
```

```sql
-- Positive: one side ordered, other side small (sort cheap)
SELECT * FROM large_table l
JOIN small_lookup s ON l.key = s.key
ORDER BY l.key;
-- MergeJoin avoids hash + re-sort, preserves ordering
```

```sql
-- Negative: no equality columns
SELECT * FROM t1 JOIN t2 ON t1.a > t2.b;
-- Theta join cannot use merge join
```

```sql
-- Negative: neither side has useful ordering and both are large
SELECT * FROM huge_a JOIN huge_b ON huge_a.x = huge_b.y;
-- Hash join likely cheaper than sorting both inputs
```

## References

CockroachDB: pkg/sql/opt/xform/rules/join.opt:255 - GenerateMergeJoins (commit 51e808c)
CockroachDB: pkg/sql/opt/xform/join_funcs.go:29 - GenerateMergeJoins implementation
CockroachDB: pkg/sql/opt/ordering/ - DeriveRestrictedInterestingOrderings
Graefe, "Sort-Merge-Join: An Idea Whose Time Has(h) Passed?" (ICDE 1994)
