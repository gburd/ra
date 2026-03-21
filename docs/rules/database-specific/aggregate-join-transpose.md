# Rule: Calcite AggregateJoinTransposeRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/aggregate-join-transpose.rra`

## Metadata

- **ID:** `calcite-aggregate-join-transpose`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, aggregate, join, pushdown
- **Authors:** "RA Contributors"


# Calcite AggregateJoinTransposeRule

## Description

Pushes an aggregate below a join by splitting it into partial
aggregates on each join input, then combining the results
after the join. This is Calcite's eager aggregation rule.

**When to apply**: An aggregate sits above a join and the
grouping keys include the join key on at least one side.

**Why it works**: Pre-aggregating before a join reduces the
number of rows entering the join, potentially turning an
expensive N*M join into a much smaller n*m join.

**Calcite class**: `org.apache.calcite.rel.rules.AggregateJoinTransposeRule`

## Relational Algebra

```algebra
-- Before: aggregate above join
gamma[R.a; SUM(R.x)](R join[R.k = S.k] S)

-- After: aggregate pushed below join (eager aggregation)
(gamma[a, k; SUM(x)](R)) join[k = S.k] S
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-aggregate-join-transpose";
    "(aggregate ?group ?aggs
        (join inner ?cond ?left ?right))" =>
    "(join inner ?cond
        (aggregate ?left_group ?left_aggs ?left)
        ?right)"
    if aggs_reference_only_left("?aggs", "?left")
    if group_includes_join_key("?group", "?cond", "?left")
),
```

## Preconditions

```rust
fn applicable(
    agg_funcs: &[AggregateExpr],
    group_by: &[Expr],
    join_key_left: &Column,
) -> bool {
    // Aggregate functions must reference only one side
    let agg_cols: Vec<_> = agg_funcs.iter()
        .flat_map(|a| a.referenced_columns())
        .collect();
    let all_left = agg_cols.iter().all(|c| {
        left_columns.contains(c)
    });

    // Group-by must include the join key to preserve
    // correct grouping after the join
    let has_join_key = group_by.iter().any(|e| {
        matches!(e, Expr::Column(c) if c == join_key_left)
    });

    all_left && has_join_key
}
```

**Restrictions:**
- Only for decomposable aggregate functions (SUM, COUNT, MIN, MAX)
- AVG must be decomposed into SUM/COUNT
- DISTINCT aggregates require special handling
- Group-by must include the join key

## Cost Model

```rust
fn estimated_benefit(
    left_card: f64,
    right_card: f64,
    groups_left: f64,
) -> f64 {
    let before = left_card * right_card;
    let after = groups_left * right_card;
    (before - after) / before
}
```

**Typical benefit**: 20-90% when pre-aggregation
significantly reduces one input.

## Test Cases

```sql
-- Positive: push SUM below join
SELECT d.name, SUM(e.sal)
FROM emp e JOIN dept d ON e.deptno = d.deptno
GROUP BY d.name;
-- Pre-aggregate emp by deptno, then join with dept
```

```sql
-- Negative: AVG cannot be simply pushed down
SELECT d.name, AVG(e.sal)
FROM emp e JOIN dept d ON e.deptno = d.deptno
GROUP BY d.name;
-- Must decompose AVG into SUM/COUNT first
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/AggregateJoinTransposeRule.java
Yan & Larson: "Eager Aggregation and Lazy Aggregation" (VLDB 1995)
