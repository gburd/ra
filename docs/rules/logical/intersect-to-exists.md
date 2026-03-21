# Rule: Calcite IntersectToExistsRule

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/intersect-to-exists.rra`

## Metadata

- **ID:** `calcite-intersect-to-exists`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql
- **Tags:** logical, calcite, intersect, exists, semi-join, rewrite
- **Authors:** "RA Contributors"


# Calcite IntersectToExistsRule

## Description

Translates an INTERSECT DISTINCT into a query using EXISTS
subqueries. The first input is filtered by EXISTS checks against
each subsequent input, then deduplicated.

**When to apply**: An INTERSECT DISTINCT can be more efficiently
executed as filtered scans with EXISTS checks.

**Why it works**: EXISTS translates to semi-joins, which can
leverage indexes on the subquery tables. This avoids building
full hash tables for set intersection.

**Calcite class**: `org.apache.calcite.rel.rules.IntersectToExistsRule`

## Relational Algebra

```algebra
-- Before: INTERSECT
R INTERSECT S

-- After: EXISTS-based filter
gamma[*](sigma[EXISTS(SELECT 1 FROM S WHERE S.* = R.*)](R))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-intersect-to-exists";
    "(intersect ?left ?right)" =>
    "(aggregate (all-cols)
        (filter (exists
            (filter (is-not-distinct-from-all ?left ?right)
                ?right))
            ?left))"
    if is_distinct_intersect("intersect")
),
```

## Preconditions

```rust
fn applicable(intersect: &Intersect) -> bool {
    !intersect.all
}
```

**Restrictions:**
- Only for INTERSECT DISTINCT (not ALL)
- Uses IS NOT DISTINCT FROM for NULL-safe comparison
- Multi-way INTERSECT chains multiple EXISTS

## Cost Model

```rust
fn estimated_benefit(
    left_rows: f64,
    right_rows: f64,
    has_index: bool,
) -> f64 {
    if has_index { 0.5 } else { 0.1 }
}
```

**Typical benefit**: 10-50% when indexes enable efficient semi-join.

## Test Cases

```sql
-- Positive: INTERSECT to EXISTS
SELECT empno FROM emp
INTERSECT
SELECT empno FROM archived_emp;
-- Becomes: SELECT DISTINCT empno FROM emp
--          WHERE EXISTS (SELECT 1 FROM archived_emp WHERE ...)
```

```sql
-- Negative: INTERSECT ALL
SELECT empno FROM emp
INTERSECT ALL
SELECT empno FROM archived_emp;
-- INTERSECT ALL requires count-based logic
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/IntersectToExistsRule.java (commit af6367d)
