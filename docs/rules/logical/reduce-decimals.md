# Rule: Calcite ReduceDecimalsRule

**Category:** logical/expression-simplification
**File:** `rules/logical/expression-simplification/reduce-decimals.rra`

## Metadata

- **ID:** `calcite-reduce-decimals`
- **Version:** "1.0.0"
- **Databases:** calcite, mysql
- **Tags:** logical, calcite, decimal, reduction, type-lowering
- **Authors:** "RA Contributors"


# Calcite ReduceDecimalsRule

## Description

Reduces decimal operations (casts, arithmetic) into operations using
more primitive types (longs, doubles). This allows implementations
that lack native decimal support to handle decimal arithmetic
consistently using scaled integer operations.

**When to apply**: A Calc or Project contains decimal arithmetic
that needs to be lowered to primitive type operations.

**Why it works**: Decimal(p,s) can be represented as an integer
multiplied by 10^(-s). All decimal operations (add, subtract,
multiply, divide, cast) can be expressed as integer operations
with appropriate scaling.

**Calcite class**: `org.apache.calcite.rel.rules.ReduceDecimalsRule`

## Relational Algebra

```algebra
-- Before: decimal arithmetic
pi[price * quantity AS total](R)
  where price is DECIMAL(10,2) and quantity is INT

-- After: integer arithmetic with scaling
pi[cast((price_long * quantity) / 100 AS DECIMAL(10,2))](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-reduce-decimal-multiply";
    "(* (decimal ?p1 ?s1 ?v1) (decimal ?p2 ?s2 ?v2))" =>
    "(rescale (+ ?s1 ?s2) (* ?v1 ?v2))"
),

rw!("calcite-reduce-decimal-add";
    "(+ (decimal ?p1 ?s ?v1) (decimal ?p2 ?s ?v2))" =>
    "(decimal (max ?p1 ?p2) ?s (+ ?v1 ?v2))"
    if same_scale("?s")
),
```

## Preconditions

```rust
fn applicable(calc: &Calc) -> bool {
    calc.program().requires_decimal_expansion()
}
```

**Restrictions:**
- Optionally applied; some backends have native decimal support
- Overflow handling must be preserved
- Division requires careful rounding mode handling

## Cost Model

```rust
fn estimated_benefit(
    num_decimal_ops: usize,
) -> f64 {
    // Integer ops are faster than decimal emulation
    num_decimal_ops as f64 * 0.01
}
```

**Typical benefit**: 10-50% for decimal-heavy queries.

## Test Cases

```sql
-- Positive: decimal multiplication
SELECT price * quantity AS total FROM orders;
-- Lowered to integer multiplication with rescaling
```

```sql
-- Positive: decimal aggregation
SELECT SUM(price * discount) FROM orders;
-- Decimal arithmetic lowered before aggregation
```

```sql
-- Negative: database with native decimal support
-- Rule is optionally disabled for such databases
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ReduceDecimalsRule.java (commit af6367d)
