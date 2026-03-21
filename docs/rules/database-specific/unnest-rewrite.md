# Rule: DataFusion Unnest/Flatten Rewrite

**Category:** database-specific/datafusion
**File:** `rules/database-specific/datafusion/unnest-rewrite.rra`

## Metadata

- **ID:** `datafusion-unnest-rewrite`
- **Version:** "1.0.0"
- **Databases:** datafusion
- **Tags:** database-specific, datafusion, unnest, flatten, array, list
- **Authors:** "RA Contributors"


# DataFusion Unnest/Flatten Rewrite

## Description

Optimizes UNNEST operations on Arrow List/LargeList arrays by pushing
filters and projections below the unnest operator.  DataFusion rewrites
unnest to operate on the underlying Arrow array buffers directly rather
than row-by-row expansion.

**When to apply**: A query uses UNNEST to expand array-typed columns,
and subsequent operations (filters, projections) can be pushed below
or combined with the unnest.

**Why it works**: Arrow List arrays store all elements contiguously
with an offsets buffer.  DataFusion's physical unnest implementation
uses the offsets to expand rows without per-element allocation.
Pushing filters below unnest reduces the number of rows expanded.

**Database version**: DataFusion 32.0+

## Relational Algebra

```algebra
-- Push filter below unnest when it references non-array columns
sigma[p(a)](unnest(arr, R)) -> unnest(arr, sigma[p(a)](R))
  where a not in unnested_columns

-- Push projection below unnest to reduce columns carried
pi[a, unnested](unnest(arr, R))
  -> unnest(arr, pi[a, arr](R))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("datafusion-push-filter-below-unnest";
    "(filter ?pred (unnest ?arr_col ?input))" =>
    "(unnest ?arr_col (filter ?pred ?input))"
    if is_database("datafusion")
    if predicate_references_only_non_unnested("?pred", "?arr_col")
),

rw!("datafusion-push-project-below-unnest";
    "(project ?cols (unnest ?arr_col ?input))" =>
    "(unnest ?arr_col (project (extend-cols ?cols ?arr_col) ?input))"
    if is_database("datafusion")
    if can_push_project_through_unnest("?cols", "?arr_col")
),
```

## Preconditions

```rust
fn applicable(filter_pred: &Expr, unnest_col: &Column) -> bool {
    !filter_pred.column_refs().contains(unnest_col)
}
```

**Restrictions:**
- Filters referencing the unnested column cannot be pushed below
- Unnest changes cardinality, so aggregates above cannot be pushed below
- Nested unnest (unnest of array of arrays) requires special handling

## Cost Model

```rust
fn filter_pushdown_benefit(
    outer_rows: f64,
    avg_array_length: f64,
    selectivity: f64,
) -> f64 {
    // Rows avoided in unnest expansion
    let rows_before = outer_rows * avg_array_length;
    let rows_after = outer_rows * selectivity * avg_array_length;
    rows_before - rows_after
}
```

**Typical benefit**: For arrays averaging 100 elements with 10%
filter selectivity, reduces expanded rows by 90%.

## Test Cases

```sql
-- Positive: filter on non-array column pushed below unnest
SELECT u.id, tag
FROM users u, UNNEST(u.tags) AS tag
WHERE u.active = true;
-- Filter active=true pushed below UNNEST
```

```sql
-- Negative: filter on unnested column stays above
SELECT u.id, tag
FROM users u, UNNEST(u.tags) AS tag
WHERE tag LIKE 'premium%';
-- Cannot push: filter references unnested column
```

## References

DataFusion: datafusion/optimizer/src/optimize_projections/mod.rs
DataFusion: datafusion/physical-plan/src/unnest.rs
DataFusion: datafusion/expr/src/logical_plan/plan.rs (Unnest variant)
