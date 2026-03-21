# Rule: Calcite SortUnionTransposeRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/sort-union-transpose.rra`

## Metadata

- **ID:** `calcite-sort-union-transpose`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, sort, union, pushdown
- **Authors:** "RA Contributors"


# Calcite SortUnionTransposeRule

## Description

Pushes a sort with LIMIT below a UNION ALL by applying the
sort+limit to each branch. This enables early termination:
each branch only needs to produce its top-K rows, and the
final merge only combines 2*K rows instead of the full union.

**When to apply**: A `LogicalSort` with a LIMIT sits above a
`LogicalUnion` (ALL).

**Why it works**: Without the pushdown, all rows from both
branches are materialized before sorting. With the pushdown,
each branch produces at most LIMIT rows, dramatically reducing
the data processed by the final sort-merge.

**Calcite class**: `org.apache.calcite.rel.rules.SortUnionTransposeRule`

## Relational Algebra

```algebra
-- Before: sort above union
tau[a ASC] LIMIT k (R UNION ALL S)

-- After: sort pushed to branches with limit
tau[a ASC] LIMIT k (
    tau[a ASC] LIMIT k (R)
    UNION ALL
    tau[a ASC] LIMIT k (S)
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-sort-union-transpose";
    "(limit ?n (sort ?keys (union-all ?left ?right)))" =>
    "(limit ?n (sort ?keys
        (union-all
            (limit ?n (sort ?keys ?left))
            (limit ?n (sort ?keys ?right)))))"
),
```

## Preconditions

```rust
fn applicable(
    has_limit: bool,
    union_is_all: bool,
) -> bool {
    // Must have a LIMIT (otherwise pushing sort doesn't
    // reduce work) and union must be ALL (DISTINCT unions
    // need all rows for deduplication)
    has_limit && union_is_all
}
```

**Restrictions:**
- Without LIMIT, pushing the sort provides no benefit
  (both branches still fully sorted)
- UNION DISTINCT requires all rows for deduplication before
  sorting, so this rule does not apply
- OFFSET handling: each branch needs LIMIT (offset + limit)

## Cost Model

```rust
fn estimated_benefit(
    left_rows: f64,
    right_rows: f64,
    limit: f64,
) -> f64 {
    let total = left_rows + right_rows;
    let after = 2.0 * limit;
    if total > 0.0 && after < total {
        (total - after) / total
    } else {
        0.0
    }
}
```

**Typical benefit**: 10-50% depending on limit relative to
total rows.

## Test Cases

```sql
-- Positive: push sort+limit into union branches
(SELECT name, sal FROM emp_us
 UNION ALL
 SELECT name, sal FROM emp_eu)
ORDER BY sal DESC LIMIT 10;
-- Each branch returns top 10, final merge on 20 rows
```

```sql
-- Negative: no limit present
(SELECT name FROM emp_us
 UNION ALL
 SELECT name FROM emp_eu)
ORDER BY name;
-- No limit, pushing sort doesn't help
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/SortUnionTransposeRule.java
