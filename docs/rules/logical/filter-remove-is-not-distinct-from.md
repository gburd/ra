# Rule: Calcite FilterRemoveIsNotDistinctFromRule

**Category:** logical/expression-simplification
**File:** `rules/logical/expression-simplification/filter-remove-is-not-distinct-from.rra`

## Metadata

- **ID:** `calcite-filter-remove-is-not-distinct-from`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql
- **Tags:** logical, calcite, filter, is-not-distinct-from, simplification
- **Authors:** "RA Contributors"


# Calcite FilterRemoveIsNotDistinctFromRule

## Description

Replaces IS NOT DISTINCT FROM in a filter with logically equivalent
operations using standard comparison operators. This enables the
expression to be used for index lookups and other optimizations
that don't understand IS NOT DISTINCT FROM.

**When to apply**: A filter contains IS NOT DISTINCT FROM predicates
that need to be expanded for downstream processing.

**Why it works**: `a IS NOT DISTINCT FROM b` is equivalent to
`(a = b) OR (a IS NULL AND b IS NULL)`. The expanded form uses
standard operators that indexes and storage engines understand.

**Calcite class**: `org.apache.calcite.rel.rules.FilterRemoveIsNotDistinctFromRule`

## Relational Algebra

```algebra
-- Before: IS NOT DISTINCT FROM
sigma[a IS NOT DISTINCT FROM b](R)

-- After: expanded comparison
sigma[(a = b) OR (a IS NULL AND b IS NULL)](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-filter-remove-is-not-distinct-from";
    "(filter (is-not-distinct-from ?a ?b) ?input)" =>
    "(filter (or (= ?a ?b)
                 (and (is-null ?a) (is-null ?b)))
        ?input)"
),
```

## Preconditions

```rust
fn applicable(filter: &Filter) -> bool {
    filter.condition().contains_is_not_distinct_from()
}
```

**Restrictions:**
- Expanded form is more complex and may be slower without further simplification
- If one operand is known non-null, simplifies to just `a = b`
- Useful as a normalization step before index matching

## Cost Model

```rust
fn estimated_benefit(_: f64) -> f64 {
    // Normalization; no direct benefit
    0.0
}
```

**Typical benefit**: 0-20% through enabling index usage.

## Test Cases

```sql
-- Positive: IS NOT DISTINCT FROM expansion
SELECT * FROM emp e
JOIN dept d ON e.deptno IS NOT DISTINCT FROM d.deptno;
-- Expanded to: (e.deptno = d.deptno) OR (e.deptno IS NULL AND d.deptno IS NULL)
```

```sql
-- Positive: with known non-null column
SELECT * FROM emp WHERE empno IS NOT DISTINCT FROM 7369;
-- empno is NOT NULL; simplifies to empno = 7369
```

```sql
-- Negative: no IS NOT DISTINCT FROM
SELECT * FROM emp WHERE empno = 7369;
-- Standard equality; rule does not apply
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/FilterRemoveIsNotDistinctFromRule.java (commit af6367d)
