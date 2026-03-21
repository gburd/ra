# Rule: Calcite JoinPushExpressionsRule

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/join-push-expressions.rra`

## Metadata

- **ID:** `calcite-join-push-expressions`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql, oracle
- **Tags:** logical, calcite, join, expressions, pushdown, equi-join
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join ?type ?cond ?left ?right)"
    description: "Join with pushable expressions in condition"
  - type: "predicate"
    condition: "has_pushable_expressions(?cond, ?left, ?right)"
    description: "Join condition must contain expressions that reference only one side"
```


# Calcite JoinPushExpressionsRule

## Description

Pushes down expressions in equi-join conditions by adding projections
above the join inputs. Transforms complex join conditions like
`emp.deptno + 1 = dept.deptno` into a simple equi-join by computing
the expression `emp.deptno + 1` in a project below the join.

**When to apply**: A join condition contains expressions (not simple
column references) on one or both sides of an equality.

**Why it works**: Simple equi-join conditions enable hash join and
merge join algorithms. Expressions in join conditions prevent these
optimizations. By pre-computing the expression, the join becomes a
standard equi-join.

**Calcite class**: `org.apache.calcite.rel.rules.JoinPushExpressionsRule`

## Relational Algebra

```algebra
-- Before: expression in join condition
R JOIN[R.x + 1 = S.y] S

-- After: expression computed in project
pi[*](
    pi[*, x + 1 AS x_plus_1](R)
    JOIN[x_plus_1 = S.y]
    S
)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-join-push-expressions";
    "(join ?type (= (+ ?lcol ?val) ?rcol) ?left ?right)" =>
    "(project (remove-col ?extra)
        (join ?type (= ?extra ?rcol)
            (project (add-col (+ ?lcol ?val) ?extra) ?left)
            ?right))"
    if is_complex_expr("(+ ?lcol ?val)")
),
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(join: &Join) -> bool {
    let condition = join.condition();
    condition.equi_pairs().any(|(left_expr, right_expr)| {
        !left_expr.is_simple_column()
            || !right_expr.is_simple_column()
    })
}
```

**Restrictions:**
- Only applies to equality conditions
- Non-equal conditions are left unchanged
- The added projection must be removed by a subsequent ProjectRemoveRule

## Cost Model

```rust
fn estimated_benefit(
    left_rows: f64,
    expression_cost: f64,
) -> f64 {
    // Expression evaluated once per row instead of per probe
    left_rows * expression_cost * 0.001
}
```

**Typical benefit**: 5-30% by enabling hash/merge join algorithms.

## Test Cases

```sql
-- Expected: expression pre-computed in project before join
-- e.deptno + 1 should be computed in a project below the join
-- to enable a simple equi-join.
SELECT * FROM emp e
JOIN dept d ON e.deptno + 1 = d.deptno;
```

```sql
-- Expected: function pre-computed before join
-- UPPER(o.product_code) should be projected before the join.
SELECT * FROM orders o
JOIN products p ON UPPER(o.product_code) = p.code;
```

```sql
-- Expected: simple column reference needs no transformation
-- Already a plain equi-join, no expressions to push down.
SELECT * FROM emp e
JOIN dept d ON e.deptno = d.deptno;
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/JoinPushExpressionsRule.java (commit af6367d)
