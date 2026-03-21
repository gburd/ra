# Rule: Calcite ProjectToCalcRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/project-to-calc.rra`

## Metadata

- **ID:** `calcite-project-to-calc`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, project, calc, conversion
- **Authors:** "RA Contributors"


# Calcite ProjectToCalcRule

## Description

Converts a `LogicalProject` into a `LogicalCalc`. A Calc is
Calcite's unified representation that combines filter and project
into a single operator using a program (list of expressions with
a condition). This conversion enables subsequent merging of
adjacent Calc nodes.

**When to apply**: A `LogicalProject` exists in the plan and
the optimizer is using the Calc-based program model.

**Why it works**: Converting projects to Calcs enables the
`CalcMergeRule` to fuse adjacent filter-project sequences into
a single operator, reducing plan tree depth and enabling more
efficient code generation.

**Calcite class**: `org.apache.calcite.rel.rules.ProjectToCalcRule`

## Relational Algebra

```algebra
-- Before: project
pi[a, b + 1 AS c](R)

-- After: calc with project program
Calc[program: {a, b + 1 AS c}](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-project-to-calc";
    "(project ?exprs ?input)" =>
    "(calc ?exprs true ?input)"
),
```

## Preconditions

```rust
fn applicable() -> bool {
    // Always applicable: any project can become a calc
    true
}
```

**Restrictions:**
- This is a representation change, not a semantic optimization
- Only useful if the optimizer has other Calc-based rules
  (CalcMergeRule, FilterToCalcRule)

## Cost Model

```rust
fn estimated_benefit(_rows: f64) -> f64 {
    // No direct benefit; enables further optimizations
    0.0
}
```

**Typical benefit**: 1-10% indirectly, by enabling Calc merging.

## Test Cases

```sql
-- Positive: simple projection
SELECT a, b + 1 AS c FROM t;
-- Project(a, b+1) -> Calc(program=[a, b+1], cond=true)
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ProjectToCalcRule.java
