# Rule: Materialize Operator Fusion

**Category:** database-specific/materialize
**File:** `rules/database-specific/materialize/fusion.rra`

## Metadata

- **ID:** `materialize-fusion`
- **Version:** "1.0.0"
- **Databases:** materialize
- **Tags:** database-specific, materialize, fusion, map, filter, project, merge
- **Authors:** "RA Contributors"


# Materialize Operator Fusion

## Description

Fuses adjacent operators of the same type into single operators.
Materialize's fusion transforms merge consecutive Maps, Filters,
Projects, and Negates to reduce the number of differential dataflow
operators and the overhead of passing collections between them.

**When to apply**: Consecutive operators of the same type appear in
the dataflow graph (e.g., two adjacent Map operators).

**Why it works**: Each differential dataflow operator has per-batch
overhead for progress tracking and message passing.  Fusing adjacent
operators reduces the operator count and eliminates intermediate
collections.

**Database version**: Materialize 0.20+

## Relational Algebra

```algebra
-- Map fusion
map[f2](map[f1](R)) -> map[f1, f2](R)

-- Filter fusion
sigma[p2](sigma[p1](R)) -> sigma[p1 AND p2](R)

-- Project fusion
pi[cols2](pi[cols1](R)) -> pi[compose(cols2, cols1)](R)

-- Negate fusion
negate(negate(R)) -> R
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("materialize-fuse-maps";
    "(map ?exprs2 (map ?exprs1 ?input))" =>
    "(map (concat-exprs ?exprs1 ?exprs2) ?input)"
    if is_database("materialize")
),

rw!("materialize-fuse-filters";
    "(filter ?p2 (filter ?p1 ?input))" =>
    "(filter (and ?p1 ?p2) ?input)"
    if is_database("materialize")
),

rw!("materialize-fuse-projects";
    "(project ?cols2 (project ?cols1 ?input))" =>
    "(project (compose-projects ?cols2 ?cols1) ?input)"
    if is_database("materialize")
),

rw!("materialize-fuse-double-negate";
    "(negate (negate ?input))" =>
    "?input"
    if is_database("materialize")
),
```

## Preconditions

```rust
fn applicable(outer: &MirRelationExpr, inner: &MirRelationExpr) -> bool {
    std::mem::discriminant(outer) == std::mem::discriminant(inner)
}
```

**Restrictions:**
- Map fusion must adjust column references in the outer expressions
  to account for columns added by the inner Map
- Project fusion must compose the column index mappings correctly
- Filter fusion preserves short-circuit evaluation order

## Cost Model

```rust
fn estimated_benefit(
    fused_operators: usize,
    rows: f64,
) -> f64 {
    // Eliminate per-operator overhead
    (fused_operators - 1) as f64 * rows * 0.00001
}
```

**Typical benefit**: Reduces operator count by 10-30%, improving
dataflow compilation time and reducing per-batch overhead.

## Test Cases

```sql
-- Positive: consecutive filters fused
SELECT * FROM events
WHERE status = 'active'
  AND created_at > '2025-01-01';
-- Single Filter(status = 'active' AND created_at > ...)
```

```sql
-- Positive: consecutive projections fused
SELECT name FROM (SELECT id, name, email FROM users);
-- Single Project([name])
```

## References

Materialize: src/transform/src/fusion/filter.rs
Materialize: src/transform/src/fusion/map.rs
Materialize: src/transform/src/fusion/project.rs
Materialize: src/transform/src/fusion/negate.rs
