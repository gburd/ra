# Rule: Calcite ProjectCorrelateTransposeRule

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/project-correlate-transpose.rra`

## Metadata

- **ID:** `calcite-project-correlate-transpose`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql, oracle
- **Tags:** logical, calcite, projection, correlate, lateral, pushdown
- **Authors:** "Apache Calcite"


# Project Correlate Transpose

## Description

Pushes a projection through a correlated join (LATERAL / APPLY), pruning
columns from both the outer and inner side that are not used downstream.
Correlated joins are expensive because the inner side executes once per
outer row; reducing width on both sides yields multiplicative savings.

**When to apply**: A Project above a Correlate where not all columns from
the outer or inner relation are referenced.

**Calcite class**: `org.apache.calcite.rel.rules.ProjectCorrelateTransposeRule`

## Relational Algebra

```algebra
-- Before
pi[o.a, i.x](Correlate(Outer(a, b, c), Inner(x, y)))

-- After
pi[o.a, i.x](Correlate(pi[a](Outer), pi_inner[x](Inner)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("project-correlate-transpose";
    "(project ?cols (correlate ?outer ?inner ?corr_vars))" =>
    "(project ?cols (correlate
        (project (outer-needed ?cols ?corr_vars) ?outer)
        (project (inner-needed ?cols ?corr_vars) ?inner)
        ?corr_vars))"
),
```

## Preconditions

```rust
fn applicable(project: &Project, correlate: &Correlate) -> bool {
    let needed = project.referenced_columns();
    let corr_needed = correlate.correlation_variables();
    let outer_used = needed.intersect(&correlate.outer_schema())
        .union(&corr_needed);
    outer_used.len() < correlate.outer_schema().len()
        || needed.intersect(&correlate.inner_schema()).len()
            < correlate.inner_schema().len()
}
```

**Restrictions:**
- Correlation variables must be preserved on the outer side
- Inner side must retain columns referenced by correlation

## Cost Model

```rust
fn estimated_benefit(
    outer_rows: f64,
    inner_avg_rows: f64,
    cols_removed: usize,
) -> f64 {
    outer_rows * inner_avg_rows * cols_removed as f64 * 8.0
}
```

**Typical benefit**: 5-30%, multiplicative with outer cardinality.

## Test Cases

```sql
-- Positive: prune unused lateral columns
SELECT e.name, d.dept_name
FROM employees e,
     LATERAL (SELECT dept_name, budget FROM departments d
              WHERE d.id = e.dept_id) d;
-- budget not used: push pi[dept_name] into lateral subquery
```

```sql
-- Negative: all columns used
SELECT e.*, d.*
FROM employees e, LATERAL (SELECT * FROM departments d WHERE d.id = e.dept_id) d;
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ProjectCorrelateTransposeRule.java
