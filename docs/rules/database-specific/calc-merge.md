# Rule: Calcite CalcMergeRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/calc-merge.rra`

## Metadata

- **ID:** `calcite-calc-merge`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, calc, merge, fusion
- **Authors:** "RA Contributors"


# Calcite CalcMergeRule

## Description

Merges two adjacent `LogicalCalc` operators into a single Calc.
The resulting Calc combines the programs and conditions by
composing the outer program over the inner program and ANDing
the conditions. This reduces plan tree depth and enables single
pass execution.

**When to apply**: A `LogicalCalc` has another `LogicalCalc`
as its direct input.

**Why it works**: Two passes over data (filter then project,
or project then filter) become one. The merged program evaluates
all expressions and conditions in a single operator.

**Calcite class**: `org.apache.calcite.rel.rules.CalcMergeRule`

## Relational Algebra

```algebra
-- Before: two adjacent calcs
Calc[prog2, cond2](Calc[prog1, cond1](R))

-- After: single merged calc
Calc[prog2(prog1), cond1 AND cond2(prog1)](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-calc-merge";
    "(calc ?prog2 ?cond2 (calc ?prog1 ?cond1 ?input))" =>
    "(calc (compose ?prog2 ?prog1)
          (and ?cond1 (substitute ?cond2 ?prog1))
          ?input)"
),
```

## Preconditions

```rust
fn applicable(
    outer_program: &[Expr],
    inner_program: &[Expr],
) -> bool {
    // Programs must be composable: outer expressions
    // must reference only indices from inner program
    outer_program.iter().all(|e| {
        e.referenced_indices().iter().all(|&i| {
            i < inner_program.len()
        })
    })
}
```

**Restrictions:**
- Programs with non-deterministic expressions may produce
  different results if merged (evaluation count changes)
- Correlated variables must be handled carefully during
  program composition

## Cost Model

```rust
fn estimated_benefit(rows: f64) -> f64 {
    // Save one iteration over all rows
    rows * 0.001
}
```

**Typical benefit**: 5-30% from eliminating an intermediate
materialization point.

## Test Cases

```sql
-- Positive: filter then project fused
SELECT a, b + 1 FROM (
    SELECT * FROM t WHERE x > 10
) sub;
-- Filter->Project becomes single Calc
```

```sql
-- Positive: project then filter fused
SELECT * FROM (
    SELECT a, b + 1 AS c FROM t
) sub WHERE c > 5;
-- Project->Filter becomes single Calc
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/CalcMergeRule.java
