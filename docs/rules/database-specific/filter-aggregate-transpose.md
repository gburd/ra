# Rule: Calcite FilterAggregateTransposeRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/filter-aggregate-transpose.rra`

## Metadata

- **ID:** `calcite-filter-aggregate-transpose`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, filter, aggregate, pushdown
- **Authors:** "RA Contributors"


# Calcite FilterAggregateTransposeRule

## Description

Pushes a filter below an aggregate when the filter predicate
references only the grouping columns (not aggregate results).
Since grouping columns pass through the aggregate unchanged,
filtering before aggregation reduces the data that must be
grouped.

**When to apply**: A `LogicalFilter` sits above a
`LogicalAggregate` and the predicate references only columns
in the GROUP BY list.

**Why it works**: Filtering before grouping reduces the number
of input rows to the aggregate, which reduces hash table size
and probe cost.

**Calcite class**: `org.apache.calcite.rel.rules.FilterAggregateTransposeRule`

## Relational Algebra

```algebra
-- Before: filter above aggregate
sigma[g > 10](gamma[g; SUM(x)](R))
  where g is a grouping column

-- After: filter pushed below aggregate
gamma[g; SUM(x)](sigma[g > 10](R))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-filter-aggregate-transpose";
    "(filter ?pred (aggregate ?group_by ?aggs ?input))" =>
    "(aggregate ?group_by ?aggs (filter ?pred ?input))"
    if pred_references_only_group_by("?pred", "?group_by")
),
```

## Preconditions

```rust
fn applicable(
    predicate: &Expr,
    group_by: &[Expr],
) -> bool {
    let pred_cols = predicate.referenced_columns();
    let group_cols: Vec<_> = group_by.iter()
        .filter_map(|e| {
            if let Expr::Column(c) = e { Some(c) }
            else { None }
        })
        .collect();

    pred_cols.iter().all(|c| group_cols.contains(&c))
}
```

**Restrictions:**
- Predicates on aggregate results (HAVING on SUM, COUNT)
  cannot be pushed below the aggregate
- Conjunctions are split: group-by predicates pushed down,
  aggregate-result predicates stay above

## Cost Model

```rust
fn estimated_benefit(
    input_rows: f64,
    selectivity: f64,
) -> f64 {
    let rows_eliminated = input_rows * (1.0 - selectivity);
    rows_eliminated * 0.01
}
```

**Typical benefit**: 20-80% reduction in aggregate input.

## Test Cases

```sql
-- Positive: filter on grouping column
SELECT dept, SUM(sal) FROM emp
GROUP BY dept HAVING dept > 10;
-- HAVING dept > 10 pushed below aggregate as WHERE dept > 10
```

```sql
-- Negative: filter on aggregate result
SELECT dept, SUM(sal) AS total FROM emp
GROUP BY dept HAVING SUM(sal) > 50000;
-- Cannot push SUM(sal) > 50000 below aggregate
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/FilterAggregateTransposeRule.java
