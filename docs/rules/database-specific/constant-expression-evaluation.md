# Rule: Apache Derby Constant Expression Evaluation

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/constant-expression-evaluation.rra`

## Metadata

- **ID:** `derby-constant-expression-evaluation`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, constant, expression, folding, evaluation
- **Authors:** "RA Contributors"


# Apache Derby Constant Expression Evaluation

## Description

Derby evaluates constant expressions at compile time rather than at
execution time.  Expressions like `1 + 1`, `CURRENT_DATE` (evaluated
once at statement start), and casts between compatible types are
folded into their result values during query compilation.

**When to apply**: A WHERE clause or SELECT list contains expressions
composed entirely of constants or deterministic functions of constants.

**Why it works**: Evaluating once at compile time avoids re-evaluating
for each row.  Constant-folded values can also enable index lookup
optimization.

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Before: expression evaluated per row
sigma[price > 100 * 1.1](scan(products))

-- After: constant folded
sigma[price > 110.0](scan(products))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-constant-folding";
    "(filter (> ?col (* ?const1 ?const2)) ?rel)" =>
    "(filter (> ?col ?folded_const) ?rel)"
    if is_database("derby")
    if both_constant("?const1", "?const2")
),
```

## Preconditions

```rust
fn applicable(expr: &Expression) -> bool {
    expr.is_constant()
    || (expr.is_deterministic()
        && expr.inputs().iter().all(|i| i.is_constant()))
}
```

**Restrictions:**
- Non-deterministic functions (RAND) are not folded
- CURRENT_TIMESTAMP is evaluated once per statement (deterministic
  within the statement)
- User-defined functions are not folded unless marked deterministic

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    eval_cost_per_row: f64,
) -> f64 {
    rows * eval_cost_per_row
}
```

**Typical benefit**: Small per-row savings that add up for large
scans.

## Test Cases

```sql
-- Positive: arithmetic constant
SELECT * FROM products WHERE price > 100 * 1.1;
-- Folded to: WHERE price > 110.0
```

```sql
-- Negative: non-deterministic function
SELECT * FROM events WHERE ts > RAND();
-- RAND() cannot be folded
```

## References

Apache Derby: Optimizer documentation
Source: org.apache.derby.impl.sql.compile.ConstantNode
