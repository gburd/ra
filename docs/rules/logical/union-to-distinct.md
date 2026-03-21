# Rule: Calcite UnionToDistinctRule

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/union-to-distinct.rra`

## Metadata

- **ID:** `calcite-union-to-distinct`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql, oracle
- **Tags:** logical, calcite, union, distinct, aggregate, normalization
- **Authors:** "RA Contributors"


# Calcite UnionToDistinctRule

## Description

Translates a UNION DISTINCT into an aggregate (for deduplication)
on top of a UNION ALL. This normalizes the plan so that deduplication
is handled by the aggregate operator, enabling further aggregate
optimizations.

**When to apply**: A UNION DISTINCT needs to be decomposed into
UNION ALL + DISTINCT for further optimization.

**Why it works**: UNION DISTINCT = UNION ALL + DISTINCT. By making
the deduplication explicit as an aggregate, rules like
AggregateUnionTransposeRule can push it down.

**Calcite class**: `org.apache.calcite.rel.rules.UnionToDistinctRule`

## Relational Algebra

```algebra
-- Before: UNION DISTINCT
R UNION S

-- After: UNION ALL with aggregate for dedup
gamma[*](R UNION ALL S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-union-to-distinct";
    "(union-distinct ?left ?right)" =>
    "(aggregate (all-cols) empty-aggs
        (union-all ?left ?right))"
),
```

## Preconditions

```rust
fn applicable(union: &Union) -> bool {
    !union.all // Only UNION DISTINCT
}
```

**Restrictions:**
- Only applies to UNION DISTINCT (all=false)
- The resulting aggregate groups by all columns

## Cost Model

```rust
fn estimated_benefit(_: f64) -> f64 {
    // Normalization step; no direct benefit
    0.0
}
```

**Typical benefit**: 0-20% indirectly through enabling other rules.

## Test Cases

```sql
-- Positive: UNION to UNION ALL + DISTINCT
SELECT dept FROM emp
UNION
SELECT dept FROM contractors;
-- Becomes: SELECT DISTINCT dept FROM (...UNION ALL...)
```

```sql
-- Negative: UNION ALL (already normalized)
SELECT dept FROM emp
UNION ALL
SELECT dept FROM contractors;
-- Already UNION ALL; no transformation needed
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/UnionToDistinctRule.java (commit af6367d)
