# Rule: Projection Merge

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/project-merge.rra`

## Metadata

- **ID:** `project-merge`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, oracle, mssql
- **Tags:** projection, merge, simplification, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"


# Projection Merge

## Description

Merges two adjacent projections into a single projection. When one
projection sits directly atop another, the outer projection's column list
can be composed with the inner one, eliminating an intermediate operator.

**When to apply**: Two projection operators are stacked with no
intervening operator.

**Why it works**: `pi[A](pi[B](R))` produces the same result as
`pi[A](R)` provided `A` is a subset of `B`. The inner projection is
redundant because the outer one already restricts the columns.

## Relational Algebra

```algebra
pi[A](pi[B](R)) -> pi[A](R)
  where A subset B
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("project-merge";
    "(project ?cols_outer (project ?cols_inner ?input))" =>
    "(project ?cols_outer ?input)"
    if columns_subset("?cols_outer", "?cols_inner")
),
```

## Preconditions

```rust
fn applicable(
    outer_cols: &[Column],
    inner_cols: &[Column],
) -> bool {
    // Every column in the outer projection must be available
    // from the inner projection (i.e., a subset of inner cols)
    outer_cols.iter().all(|c| inner_cols.contains(c))
}
```

**Restrictions:**
- Outer columns must be a subset of inner columns
- If outer projection references computed expressions from the inner
  projection, the computation must be preserved
- Does not apply if inner projection has side effects

## Cost Model

```rust
fn estimated_benefit(input_card: f64) -> f64 {
    // Eliminates one projection operator entirely.
    // Savings = cost of materializing the intermediate column set.
    let per_row_saving = PROJECTION_OVERHEAD;
    per_row_saving * input_card
}
```

**Typical benefit**: 0.05-0.2. Modest per-row savings but reduces plan
complexity, which benefits the executor.

## Test Cases

```sql
-- Positive: nested subquery projections merged
-- Before
SELECT name FROM (
    SELECT name, age FROM employees
) t;

-- After
SELECT name FROM employees;
```

```sql
-- Positive: view with extra columns
-- Before (view returns name, age, dept; query only needs name)
SELECT name FROM employee_view;
-- Plan: project[name](project[name,age,dept](scan))
-- After: project[name](scan)
```

```sql
-- Negative: outer references column computed by inner
SELECT total FROM (
    SELECT a + b AS total, c FROM t
) sub;
-- Cannot merge: "total" is computed in the inner projection
-- Inner projection must stay to compute a + b
```

## References

PostgreSQL: src/backend/optimizer/plan/setrefs.c
DuckDB: src/optimizer/remove_unused_columns.cpp
MySQL: sql/sql_resolver.cc - resolve_subquery()
