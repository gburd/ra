# Rule: Calcite CalcRemoveRule

**Category:** logical/semantic-rewriting
**File:** `rules/logical/semantic-rewriting/calc-remove.rra`

## Metadata

- **ID:** `calcite-calc-remove`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** logical, calcite, calc, remove, trivial, identity
- **Authors:** "RA Contributors"


# Calcite CalcRemoveRule

## Description

Removes a trivial Calc operator that projects its input fields
in their original order and has no filter. A trivial Calc is an
identity operation and can be safely removed.

**When to apply**: A Calc's program is trivial (identity projection,
no filter).

**Why it works**: An identity Calc does nothing; its input already
produces the desired output. Removing it simplifies the plan tree.

**Calcite class**: `org.apache.calcite.rel.rules.CalcRemoveRule`

## Relational Algebra

```algebra
-- Before: trivial Calc (identity)
Calc[proj={$0, $1, ..., $n}, filter=true](R)

-- After: remove Calc
R
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-calc-remove";
    "(calc identity true ?input)" =>
    "?input"
),
```

## Preconditions

```rust
fn applicable(calc: &Calc) -> bool {
    calc.program().is_trivial()
}
```

**Restrictions:**
- Only fires when the Calc is truly trivial (identity projection)
- Analogous to ProjectRemoveRule for Project operators

## Cost Model

```rust
fn estimated_benefit(input_rows: f64) -> f64 {
    // Minimal overhead removal
    input_rows * 0.0001
}
```

**Typical benefit**: 5-10% by removing unnecessary plan node.

## Test Cases

```sql
-- Positive: identity Calc after other transformations
-- Internal: Calc(proj=identity, filter=true, Scan(emp))
-- Becomes: Scan(emp)
```

```sql
-- Negative: non-trivial Calc
-- Internal: Calc(proj=[a, b+1], filter=true, Scan(emp))
-- Not trivial; cannot remove
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/CalcRemoveRule.java (commit af6367d)
