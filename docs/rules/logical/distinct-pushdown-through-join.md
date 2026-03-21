# Rule: Distinct Pushdown Through Join

**Category:** logical/distinct-elimination
**File:** `rules/logical/distinct-elimination/distinct-pushdown-through-join.rra`

## Metadata

- **ID:** `distinct-pushdown-through-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, mssql
- **Tags:** distinct, pushdown, join
- **Authors:** "RA Contributors"


# Distinct Pushdown Through Join

## Description

Push DISTINCT through a join when only columns from one side are projected. The distinct can be applied to the contributing side before the join, reducing the join input size.

**When to apply**: DISTINCT projects columns from only one join input, and the join preserves this uniqueness.

## Relational Algebra

```algebra
Distinct(Project[L.*](Join[cond](L, R)))
  -> Project[L.*](Join[cond](Distinct(L), R))
  where projected_cols subset_of output(L)
    and join_type preserves uniqueness
```

## Implementation

```rust
rw!("distinct-pushdown-through-join";
    "(distinct (project ?cols (join ?type ?cond ?left ?right)))" =>
    "(project ?cols (join ?type ?cond (distinct ?left) ?right))"
    if cols_from_left_only("?cols", "?left")
    if join_preserves_left_uniqueness("?type")
),
```

## Test Cases

### Positive: Distinct on left side only

```sql
SELECT DISTINCT o.customer_id
FROM orders o
JOIN products p ON o.product_id = p.id;

-- Push distinct to orders side
```

## References

- Distinct pushdown in multi-table queries
- Join-aware duplicate elimination
