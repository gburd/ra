# Rule: Calcite AggregateUnionAggregateRule

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-union-aggregate.rra`

## Metadata

- **ID:** `calcite-aggregate-union-aggregate`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql
- **Tags:** logical, calcite, aggregate, union, pullup, deduplication
- **Authors:** "RA Contributors"


# Calcite AggregateUnionAggregateRule

## Description

Matches aggregates beneath a UNION (DISTINCT) and pulls them up,
replacing with a single aggregate that removes duplicates. When both
branches of a UNION already perform the same grouping, the duplicate
aggregates can be consolidated.

**When to apply**: A UNION (not ALL) has aggregates on one or both
inputs that perform the same grouping as the implicit DISTINCT of
the UNION.

**Why it works**: UNION DISTINCT implicitly groups to remove
duplicates. If both branches already group by the same columns,
the branch-level aggregates are redundant with the UNION's
deduplication.

**Calcite class**: `org.apache.calcite.rel.rules.AggregateUnionAggregateRule`

## Relational Algebra

```algebra
-- Before: aggregates below union
gamma[g](R) UNION gamma[g](S)

-- After: single aggregate on union-all
gamma[g](R UNION ALL S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-aggregate-union-aggregate-both";
    "(union-distinct
        (aggregate ?group ?aggs ?left)
        (aggregate ?group ?aggs ?right))" =>
    "(aggregate ?group ?aggs (union-all ?left ?right))"
),

rw!("calcite-aggregate-union-aggregate-first";
    "(union-distinct
        (aggregate ?group ?aggs ?left)
        ?right)" =>
    "(aggregate ?group ?aggs (union-all ?left ?right))"
    if output_matches_group("?right", "?group")
),
```

## Preconditions

```rust
fn applicable(
    union: &Union,
    left_agg: Option<&Aggregate>,
    right_agg: Option<&Aggregate>,
) -> bool {
    !union.all
        && (left_agg.is_some() || right_agg.is_some())
}
```

**Restrictions:**
- Only applies to UNION DISTINCT (not UNION ALL)
- Branch aggregates must have compatible grouping sets
- The merged aggregate must produce the same result

## Cost Model

```rust
fn estimated_benefit(
    left_groups: f64,
    right_groups: f64,
) -> f64 {
    // Avoid double-aggregation overhead
    let double_cost = left_groups + right_groups + left_groups + right_groups;
    let single_cost = left_groups + right_groups;
    if double_cost > 0.0 { 1.0 - single_cost / double_cost } else { 0.0 }
}
```

**Typical benefit**: 10-50% by consolidating redundant aggregation.

## Test Cases

```sql
-- Positive: matching aggregates pulled up
SELECT dept FROM emp WHERE year = 2023 GROUP BY dept
UNION
SELECT dept FROM emp WHERE year = 2024 GROUP BY dept;
-- Becomes: SELECT dept FROM (... UNION ALL ...) GROUP BY dept
```

```sql
-- Negative: different GROUP BY columns
SELECT dept, SUM(sal) FROM emp GROUP BY dept
UNION
SELECT dept, AVG(sal) FROM emp GROUP BY dept;
-- Aggregate functions differ; cannot merge
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/AggregateUnionAggregateRule.java (commit af6367d)
