# Rule: Calcite JoinPushThroughJoinRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/join-push-through-join.rra`

## Metadata

- **ID:** `calcite-join-push-through-join`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, join, reorder, associativity
- **Authors:** "RA Contributors"


# Calcite JoinPushThroughJoinRule

## Description

Reorders joins by pushing one join through another, exploiting
join associativity and commutativity. This is Calcite's primary
mechanism for join enumeration in the Volcano/Cascades-style
planner. Given `(A join B) join C`, the rule produces
`(A join C) join B` when the join conditions allow it.

**When to apply**: Two adjacent inner joins where reordering
may reduce intermediate result sizes.

**Why it works**: Different join orderings can produce
dramatically different intermediate cardinalities. Pushing a
more selective join earlier reduces the data flowing through
the plan.

**Calcite class**: `org.apache.calcite.rel.rules.JoinPushThroughJoinRule`

## Relational Algebra

```algebra
-- Before: left-deep join tree
(R join[p1] S) join[p2] T

-- After: reordered (when p2 references R and T)
(R join[p2] T) join[p1'] S
  where p1' adjusts column references for the new ordering
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-join-push-through-join-right";
    "(join inner ?c2
        (join inner ?c1 ?a ?b)
        ?c)" =>
    "(join inner ?c1_adj
        (join inner ?c2 ?a ?c)
        ?b)"
    if join_condition_compatible("?c2", "?a", "?c")
    if can_adjust_condition("?c1", "?a", "?b", "?c")
),
```

## Preconditions

```rust
fn applicable(
    outer_cond: &Expr,
    inner_left: &RelExpr,
    inner_right: &RelExpr,
    outer_right: &RelExpr,
) -> bool {
    // Outer condition must reference inner_left and
    // outer_right (not inner_right)
    let outer_refs = outer_cond.referenced_columns();
    let left_cols = inner_left.output_columns();
    let right_cols = outer_right.output_columns();

    outer_refs.iter().all(|c| {
        left_cols.contains(c) || right_cols.contains(c)
    })
}
```

**Restrictions:**
- Only applies to inner joins
- Join conditions must be compatible with the new ordering
- Column references in conditions must be re-mapped

## Cost Model

```rust
fn estimated_benefit(
    card_a: f64,
    card_b: f64,
    card_c: f64,
    sel_p1: f64,
    sel_p2: f64,
) -> f64 {
    let cost_before = card_a * card_b * sel_p1 * card_c;
    let cost_after = card_a * card_c * sel_p2 * card_b;
    (cost_before - cost_after) / cost_before
}
```

**Typical benefit**: 10-80% when join reordering moves more
selective joins earlier.

## Test Cases

```sql
-- Positive: reorder to push selective join first
-- Before: (emp JOIN dept) JOIN small_table
SELECT * FROM emp e
JOIN dept d ON e.deptno = d.deptno
JOIN bonus b ON e.empno = b.empno;
-- If bonus is small, reorder to (emp JOIN bonus) JOIN dept
```

```sql
-- Negative: cannot reorder, conditions cross all three tables
SELECT * FROM a
JOIN b ON a.x = b.x
JOIN c ON a.y = c.y AND b.z = c.z;
-- Second join references both A and B, cannot isolate
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/JoinPushThroughJoinRule.java
Moerkotte & Neumann: "Analysis of Two Existing and One New Dynamic Programming Algorithm" (VLDB 2006)
