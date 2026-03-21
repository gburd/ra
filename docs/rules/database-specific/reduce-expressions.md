# Rule: Calcite ReduceExpressionsRule

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/reduce-expressions.rra`

## Metadata

- **ID:** `calcite-reduce-expressions`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** database-specific, calcite, expression, simplify, constant-folding
- **Authors:** "RA Contributors"


# Calcite ReduceExpressionsRule

## Description

Simplifies expressions by applying constant folding, constant
propagation, and algebraic simplification. Evaluates
deterministic expressions with constant inputs at plan time,
replaces redundant comparisons, and eliminates tautologies and
contradictions.

**When to apply**: Any relational operator contains expressions
with reducible sub-expressions (constant arithmetic, boolean
simplification, CAST elimination).

**Why it works**: Reducing expressions at plan time avoids
repeated evaluation at runtime and can expose further
optimization opportunities (e.g., a filter that reduces to TRUE
can be removed entirely).

**Calcite class**: `org.apache.calcite.rel.rules.ReduceExpressionsRule`

## Relational Algebra

```algebra
-- Before: constant expression in filter
sigma[1 + 2 > x](R)

-- After: folded constant
sigma[3 > x](R)

-- Before: tautology
sigma[x = x AND p](R)

-- After: simplified
sigma[p](R)

-- Before: contradiction
sigma[FALSE](R)

-- After: empty relation
empty
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Constant folding
rw!("calcite-reduce-const-add";
    "(+ ?a ?b)" => { fold_const_add("?a", "?b") }
    if both_constants("?a", "?b")
),

// Boolean simplification
rw!("calcite-reduce-and-true";
    "(and true ?p)" => "?p"
),

rw!("calcite-reduce-and-false";
    "(and false ?p)" => "false"
),

rw!("calcite-reduce-or-true";
    "(or true ?p)" => "true"
),

rw!("calcite-reduce-or-false";
    "(or false ?p)" => "?p"
),

// Filter with FALSE -> empty
rw!("calcite-reduce-filter-false";
    "(filter false ?input)" => "(empty)"
),

// Filter with TRUE -> remove filter
rw!("calcite-reduce-filter-true";
    "(filter true ?input)" => "?input"
),
```

## Preconditions

```rust
fn applicable(expr: &Expr) -> bool {
    // Expression must contain at least one reducible
    // sub-expression
    has_constant_subexpr(expr)
        || has_tautology(expr)
        || has_contradiction(expr)
}
```

**Restrictions:**
- Non-deterministic functions (RAND, CURRENT_TIMESTAMP) are
  not folded
- NULL handling follows SQL three-valued logic
- CAST reductions must respect type precision

## Cost Model

```rust
fn estimated_benefit(
    num_reduced: usize,
    rows: f64,
) -> f64 {
    // Each reduced expression saves per-row evaluation
    num_reduced as f64 * rows * 0.0001
}
```

**Typical benefit**: 5-40% from eliminating runtime expression
evaluation, more if contradictions remove entire branches.

## Test Cases

```sql
-- Positive: constant folding
SELECT * FROM emp WHERE sal > 1000 + 2000;
-- Folded to: sal > 3000
```

```sql
-- Positive: tautology removal
SELECT * FROM emp WHERE 1 = 1 AND sal > 5000;
-- Simplified to: sal > 5000
```

```sql
-- Positive: contradiction -> empty
SELECT * FROM emp WHERE 1 = 0;
-- Entire scan eliminated
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/ReduceExpressionsRule.java
