# Rule: Project Values Reduction

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/project-values-reduction.rra`

## Metadata

- **ID:** `calcite-project-values-reduction`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql, duckdb, sqlite
- **Tags:** logical, calcite, projection, values, constant-folding
- **Authors:** "Apache Calcite"


# Project Values Reduction

## Description

When a Project sits on top of a VALUES clause, eliminates unused value
columns and evaluates constant expressions. This simplifies the plan
and reduces the tuple width of inline data.

**When to apply**: A Project references only a subset of VALUES columns,
or contains evaluable constant expressions over literal values.

## Relational Algebra

```algebra
-- Before
pi[c1, c1+c2 AS sum](VALUES (1, 2, 3), (4, 5, 6))

-- After
VALUES (1, 3), (4, 9)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw\!("project-values-reduction";
    "(project ?exprs (values ?tuples))" =>
    "(values (eval-project-on-values ?exprs ?tuples))"
),
```

## Preconditions

```rust
fn applicable(project: &Project, values: &Values) -> bool {
    project.expressions().iter().all(|e| {
        e.is_column_ref() || e.is_constant_foldable()
    })
}
```

## Cost Model

```rust
fn estimated_benefit(n_tuples: usize, cols_removed: usize) -> f64 {
    n_tuples as f64 * cols_removed as f64 * 8.0
}
```

## Test Cases

```sql
-- Positive: prune unused column
SELECT a FROM (VALUES (1, 2), (3, 4)) AS t(a, b);
-- Becomes VALUES (1), (3)

-- Positive: fold constant expression
SELECT a + b AS sum FROM (VALUES (1, 2), (3, 4)) AS t(a, b);
-- Becomes VALUES (3), (7)

-- Negative: all columns used
SELECT * FROM (VALUES (1, 2), (3, 4)) AS t(a, b);
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ProjectValuesReduceRule.java
Calcite: core/src/main/java/org/apache/calcite/rel/rules/ValuesReduceRule.java
