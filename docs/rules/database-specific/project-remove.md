# Rule: Calcite ProjectRemoveRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/project-remove.rra`

## Metadata

- **ID:** `calcite-project-remove`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, project, remove, identity
- **Authors:** "RA Contributors"


# Calcite ProjectRemoveRule

## Description

Removes a `LogicalProject` that is an identity projection --
one that outputs exactly the same columns in the same order
as its input, with no renaming or expression computation.

**When to apply**: A project node outputs the same row type
as its input.

**Why it works**: An identity projection adds a plan node
with no semantic effect, increasing overhead for no benefit.

**Calcite class**: `org.apache.calcite.rel.rules.ProjectRemoveRule`

## Relational Algebra

```algebra
-- Before: identity projection
pi[a, b, c](R)  where R has columns {a, b, c}

-- After: no projection
R
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-project-remove";
    "(project ?exprs ?input)" =>
    "?input"
    if is_identity_project("?exprs", "?input")
),
```

## Preconditions

```rust
fn applicable(
    project_exprs: &[ProjectionColumn],
    input_columns: &[Column],
) -> bool {
    if project_exprs.len() != input_columns.len() {
        return false;
    }
    project_exprs.iter().zip(input_columns).all(|(pc, ic)| {
        pc.alias.is_none()
            && matches!(&pc.expr, Expr::Column(c) if c == ic)
    })
}
```

**Restrictions:**
- Renames (aliases) prevent removal
- Column reordering prevents removal
- Expression computation prevents removal

## Cost Model

```rust
fn estimated_benefit(rows: f64) -> f64 {
    rows * 0.001
}
```

**Typical benefit**: 1-10% plan overhead reduction.

## Test Cases

```sql
-- Positive: trivial SELECT *
SELECT * FROM emp;
-- Identity projection removed
```

```sql
-- Negative: column reordering
SELECT b, a FROM emp;
-- Not identity, cannot remove
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ProjectRemoveRule.java
