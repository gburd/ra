# Rule: Calcite FilterCalcMergeRule

**Category:** logical/semantic-rewriting
**File:** `rules/logical/semantic-rewriting/filter-calc-merge.rra`

## Metadata

- **ID:** `calcite-filter-calc-merge`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** logical, calcite, filter, calc, merge, fusion
- **Authors:** "RA Contributors"


# Calcite FilterCalcMergeRule

## Description

Merges a Filter into a Calc below it. The filter's condition is
ANDed with the Calc's existing filter condition, producing a single
Calc with the combined filter.

**When to apply**: A Filter sits directly above a Calc.

**Why it works**: Combining the filter into the Calc reduces the
number of operators in the plan. The Calc's RexProgram can
efficiently evaluate the combined filter and projection together.

**Calcite class**: `org.apache.calcite.rel.rules.FilterCalcMergeRule`

## Relational Algebra

```algebra
-- Before: filter above calc
sigma[p2](Calc[proj, filter=p1](R))

-- After: merged into single calc
Calc[proj, filter=(p1 AND p2)](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-filter-calc-merge";
    "(filter ?pred (calc ?proj ?filter ?input))" =>
    "(calc ?proj (and ?filter ?pred) ?input)"
),
```

## Preconditions

```rust
fn applicable(
    filter: &Filter,
    calc: &Calc,
) -> bool {
    // Always applicable when filter is above calc
    true
}
```

**Restrictions:**
- The filter predicate must be remapped to reference Calc's inputs
- Similar to FilterMergeRule but for Calc operators

## Cost Model

```rust
fn estimated_benefit(input_rows: f64) -> f64 {
    input_rows * 0.0001
}
```

**Typical benefit**: 5-20% by reducing operator count.

## Test Cases

```sql
-- Positive: filter merged into calc
-- Internal: Filter(x > 5, Calc(proj=[a,b], filter=[c < 10], R))
-- Becomes: Calc(proj=[a,b], filter=[c < 10 AND x > 5], R)
```

```sql
-- Negative: no calc below filter
-- Standard Filter(pred, Scan) is not this rule's pattern
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/FilterCalcMergeRule.java (commit af6367d)
