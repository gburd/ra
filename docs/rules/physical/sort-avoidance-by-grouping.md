# Rule: Sort Avoidance Through Aggregate Grouping

**Category:** physical/sort
**File:** `rules/physical/sort/sort-avoidance-by-grouping.rra`

## Metadata

- **ID:** `sort-avoidance-by-grouping`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, oracle, mssql
- **Tags:** physical, sort, aggregate, grouping, avoidance
- **Authors:** "Simmen, Shekita & O'Keefe"


# Sort Avoidance Through Aggregate Grouping

## Description

When a query requires both GROUP BY and ORDER BY on the same columns,
the sort-based aggregate (which sorts for grouping) already produces
output in the required order. The separate Sort for ORDER BY can be
eliminated.

**When to apply**: ORDER BY columns are a prefix of GROUP BY columns,
and a sort-based aggregate strategy is used.

## Relational Algebra

```algebra
-- Before
Sort[a](GroupBy[a, b; SUM(x)](R))  -- sort-based grouping

-- After (sort removed because GroupBy already sorted)
GroupBy[a, b; SUM(x)](R)  -- output already sorted by a, b
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw\!("sort-avoidance-by-grouping";
    "(sort ?order (sort-aggregate ?group ?aggs ?input))" =>
    "(sort-aggregate ?group ?aggs ?input)"
    if order_prefix_of_group("?order", "?group")
),
```

## Preconditions

```rust
fn applicable(sort: &Sort, agg: &SortAggregate) -> bool {
    sort.order_keys().is_prefix_of(&agg.group_by_keys())
}
```

## Cost Model

```rust
fn estimated_benefit(rows: f64) -> f64 {
    rows * (rows as f64).log2() * 0.001
}
```

## Test Cases

```sql
-- Positive: ORDER BY matches GROUP BY
SELECT department, COUNT(*) FROM employees
GROUP BY department ORDER BY department;
-- Sort-aggregate on department already produces sorted output

-- Negative: ORDER BY on different column
SELECT department, COUNT(*) AS cnt FROM employees
GROUP BY department ORDER BY cnt;
-- Need to sort by cnt after grouping
```

## References

- Simmen, D., Shekita, E. & O'Keefe, T., "Fundamental Techniques for Order Optimization", ACM SIGMOD 1996
