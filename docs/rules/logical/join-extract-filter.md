# Rule: Calcite JoinExtractFilterRule

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/join-extract-filter.rra`

## Metadata

- **ID:** `calcite-join-extract-filter`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql
- **Tags:** logical, calcite, join, filter, extract, cartesian
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join ?type (and ?join_cond ?filter_pred) ?left ?right)"
    description: "Join with compound condition containing a filter"
  - type: "predicate"
    condition: "references_only(?filter_pred, ?left) || references_only(?filter_pred, ?right)"
    description: "Filter part must reference only one side of the join"
```


# Calcite JoinExtractFilterRule

## Description

Extracts the join condition from an inner join and places it as a
filter above a cartesian join. This canonical form allows the
condition to be combined with other filters above the join.

**When to apply**: An inner join has a non-trivial join condition
that could benefit from being combined with filters above.

**Why it works**: By separating the join condition into a filter,
other rules (like FilterMergeRule or predicate pushdown) can combine
or rearrange conditions more effectively.

**Calcite class**: `org.apache.calcite.rel.rules.JoinExtractFilterRule`

## Relational Algebra

```algebra
-- Before: inner join with condition
R JOIN[R.k = S.k AND R.x > 5] S

-- After: filter above cartesian join
sigma[R.k = S.k AND R.x > 5](R CROSS JOIN S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-join-extract-filter";
    "(join inner ?cond ?left ?right)" =>
    "(filter ?cond (join inner true ?left ?right))"
    if has_non_trivial_condition("?cond")
),
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(join: &Join) -> bool {
    join.join_type() == JoinType::Inner
        && !join.condition().is_always_true()
}
```

**Restrictions:**
- Only applies to INNER joins (not outer joins)
- Condition must not be trivially TRUE
- This is primarily a normalization step; the resulting cross join
  must not be executed as-is

## Cost Model

```rust
fn estimated_benefit(_: f64) -> f64 {
    // No direct benefit; enables other optimizations
    0.0
}
```

**Typical benefit**: 0-20% indirectly through enabling further rewrites.

## Test Cases

```sql
-- Expected: join condition extracted to filter
-- The equi-join condition is extracted as a filter above a cross join
-- to enable further predicate combination.
SELECT * FROM emp e
JOIN dept d ON e.deptno = d.deptno;
```

```sql
-- Expected: outer join condition stays in place
-- LEFT JOIN semantics differ so the condition cannot be extracted.
-- The optimizer may still apply other transforms.
SELECT * FROM emp e
LEFT JOIN dept d ON e.deptno = d.deptno;
```

```sql
-- Expected: cross join has no condition to extract
-- Already a cartesian product with no condition.
SELECT * FROM emp e, dept d;
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/JoinExtractFilterRule.java (commit af6367d)
