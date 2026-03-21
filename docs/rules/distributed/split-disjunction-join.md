# Rule: Split Disjunction of Join Terms

**Category:** distributed/distributed-joins
**File:** `rules/distributed/distributed-joins/split-disjunction-join.rra`

## Metadata

- **ID:** `split-disjunction-join`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** distributed, join, disjunction, or, union, index
- **Authors:** "RA Contributors"


# Split Disjunction of Join Terms

## Description

Splits an OR expression in a join's ON clause into a UNION of two
joins, each handling one side of the disjunction. This enables each
join branch to use a different index to satisfy its predicate, avoiding
a cross join that would be needed to evaluate the OR expression.

**When to apply**: An inner or semi join has an OR expression in its ON
clause, and both sides of the OR can be satisfied by different indexes.

**Why it works**: A single join with an OR condition often cannot use
any index effectively, falling back to a cross join or full scan. By
splitting into two joins, each can independently use its optimal index.
The UNION ALL followed by deduplication ensures correctness.

## Relational Algebra

```algebra
Join[a = b OR c = d](L, R)
  -> DistinctOn(pk,
       UnionAll(
         Join[a = b](L, R),
         Join[c = d](L, R)
       )
     )
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("split-disjunction-of-join-terms";
    "(join ?type ?left ?right ?on ?private)" =>
    "(project
        (distinct_on ?pk
            (union_all
                (join ?type ?left ?right ?on_left ?private)
                (join ?type ?left ?right ?on_right ?private)))
        (output_cols_of_original))"
    if has_splittable_disjunction("?on")
),
```

## Preconditions

```rust
fn applicable(
    join_type: JoinType,
    on: &FiltersExpr,
) -> bool {
    // Must be inner join or semi join
    matches!(join_type, Inner | Semi)
    // ON clause must contain a splittable OR
    && on.has_splittable_disjunction()
    // Splitting must enable index usage on both sides
    && both_sides_use_indexes(on)
}
```

**Restrictions:**
- Only applies to InnerJoin and SemiJoin (anti joins use
  INTERSECT ALL instead of UNION ALL)
- Requires primary key columns to deduplicate the UNION ALL result
- If neither side of the OR benefits from an index, the split adds
  overhead without benefit
- The deduplication step (DistinctOn) adds sorting/hashing cost

## Cost Model

```rust
fn split_disjunction_cost(
    left_rows: f64,
    right_rows: f64,
    or_selectivity_left: f64,
    or_selectivity_right: f64,
    dedup_cost_per_row: f64,
) -> f64 {
    let branch_a = left_rows * or_selectivity_left;
    let branch_b = left_rows * or_selectivity_right;
    let union_rows = branch_a + branch_b;
    union_rows * dedup_cost_per_row
}
```

## Test Cases

```sql
-- Positive: OR in join condition, both sides indexable
-- t1 has INDEX(a), t2 has INDEX(b) and INDEX(c)
SELECT * FROM t1
JOIN t2 ON t1.a = t2.b OR t1.a = t2.c;

-- Without split: cross join with filter (expensive)
-- With split:
-- DistinctOn(t1.pk, t2.pk)
--   UnionAll
--     HashJoin(t1.a = t2.b) using idx_b
--     HashJoin(t1.a = t2.c) using idx_c
```

```sql
-- Positive: anti join uses INTERSECT ALL
SELECT * FROM t1
WHERE NOT EXISTS (
    SELECT 1 FROM t2 WHERE t1.a = t2.b OR t1.a = t2.c
);
-- Split into INTERSECT ALL of two anti joins
```

```sql
-- Negative: single column OR (handled by index scan)
SELECT * FROM t1 JOIN t2 ON t1.a = t2.b AND (t2.x = 1 OR t2.x = 2);
-- The OR on t2.x becomes an IN list, no split needed
```

## References

CockroachDB: pkg/sql/opt/xform/rules/join.opt:148 - SplitDisjunctionOfJoinTerms (commit 51e808c)
CockroachDB: pkg/sql/opt/xform/rules/join.opt:204 - SplitDisjunctionOfAntiJoinTerms
CockroachDB: pkg/sql/opt/xform/rules/select.opt:135 - SplitDisjunction (for Select)
