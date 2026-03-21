# Rule: Oracle Join Predicate Pushdown

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/join-predicate-pushdown.rra`

## Metadata

- **ID:** `oracle-join-predicate-pushdown`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, join, predicate, pushdown, view, lateral
- **Authors:** "RA Contributors"


# Oracle Join Predicate Pushdown

## Description

Pushes join predicates from the outer query into non-mergeable views
and lateral inline views.  Oracle's JPPD (Join Predicate Pushdown)
allows the inner view to use the pushed predicate for index access
or partition pruning, even when the view cannot be fully merged.

**When to apply**: An outer query joins with a view that contains
GROUP BY, DISTINCT, ROWNUM, or other constructs preventing view
merging, but the join predicate can be pushed inside.

**Why it works**: Without JPPD, Oracle must materialize the full view
result before applying the join predicate.  With JPPD, the predicate
is evaluated inside the view, allowing index lookups and partition
pruning that dramatically reduce the materialized data.

**Database version**: Oracle 11g+

## Relational Algebra

```algebra
-- Before: full view materialization then join
R join[R.k = V.k] (gamma[k; count=COUNT(*)](S) AS V)

-- After: join predicate pushed into view
R join[R.k = V.k]
    (gamma[k; count=COUNT(*)](sigma[S.k = R.k](S)) AS V)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-join-predicate-pushdown";
    "(join ?type (eq ?rk ?vk)
        ?outer
        (view ?vid (aggregate ?aggs ?groups ?inner)))" =>
    "(join ?type (eq ?rk ?vk)
        ?outer
        (view ?vid (aggregate ?aggs ?groups
            (filter (eq ?vk ?rk) ?inner))))"
    if is_database("oracle")
    if view_is_non_mergeable("?vid")
    if predicate_can_use_index("?vk", "?inner")
),
```

## Preconditions

```rust
fn applicable(
    view: &LogicalPlan,
    join_pred: &Expr,
) -> bool {
    !view.is_mergeable()
    && join_pred.is_equi_predicate()
    && view.inner_plan().has_usable_index(join_pred.columns())
}
```

**Restrictions:**
- Only equi-join predicates can be pushed (not range predicates)
- The view must support the pushed predicate (column must exist in view)
- CONNECT BY views prevent predicate pushdown
- Hint PUSH_PRED / NO_PUSH_PRED controls this

## Cost Model

```rust
fn estimated_benefit(
    view_full_rows: f64,
    view_with_pred_rows: f64,
    outer_rows: f64,
) -> f64 {
    // Without JPPD: materialize full view
    let without = view_full_rows * 0.01;
    // With JPPD: index lookup per outer row
    let with_jppd = outer_rows * view_with_pred_rows * 0.001;
    without - with_jppd
}
```

**Typical benefit**: For a view aggregating 10M rows, JPPD reduces
to index lookups returning ~100 rows each, 100x faster.

## Test Cases

```sql
-- Positive: predicate pushed into aggregated view
SELECT d.name, v.emp_count
FROM departments d
JOIN (SELECT dept_id, COUNT(*) emp_count
      FROM employees GROUP BY dept_id) v
ON d.id = v.dept_id
WHERE d.id = 100;
-- d.id = 100 pushed into view: only aggregates dept 100
```

```sql
-- Negative: view with ROWNUM (complex non-mergeable)
SELECT * FROM t1
JOIN (SELECT *, ROWNUM rn FROM t2) v ON t1.id = v.id;
-- ROWNUM prevents reliable predicate pushdown
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Join Predicate Pushdown"
Oracle: PUSH_PRED / NO_PUSH_PRED hints
