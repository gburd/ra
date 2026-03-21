# Rule: GREATEST/LEAST Optimization

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/greatest-least-optimization.rra`

## Metadata

- **ID:** `greatest-least-optimization`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, duckdb
- **Tags:** function, greatest, least, min, max, simplification
- **Authors:** "RA Contributors"


# GREATEST/LEAST Optimization

## Description

Optimizes GREATEST and LEAST function calls by removing redundant arguments,
folding constants, and converting to simpler forms when possible.

**When to apply**: GREATEST/LEAST calls with constant arguments, duplicate
arguments, or single arguments.

## Relational Algebra

```algebra
GREATEST(x)                -> x
GREATEST(x, x)             -> x
GREATEST(x, NULL)           -> x  (NULL-skipping semantics, PostgreSQL)
GREATEST(const1, const2)    -> MAX(const1, const2)  -- constant fold
LEAST(x)                   -> x
GREATEST(GREATEST(a, b), c) -> GREATEST(a, b, c)  -- flatten
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("greatest-single"; "(greatest ?x)" => "?x"),
rw!("least-single"; "(least ?x)" => "?x"),
rw!("greatest-duplicate"; "(greatest ?x ?x)" => "?x"),
rw!("least-duplicate"; "(least ?x ?x)" => "?x"),
rw!("greatest-flatten";
    "(greatest (greatest ?a ?b) ?c)" => "(greatest ?a ?b ?c)"
),
rw!("least-flatten";
    "(least (least ?a ?b) ?c)" => "(least ?a ?b ?c)"
),
rw!("greatest-constants";
    "(greatest (literal ?a) (literal ?b))" =>
    { FoldGreatest { a: "?a", b: "?b" } }
),
```

## Cost Model

```rust
fn estimated_benefit(args_removed: usize) -> f64 {
    args_removed as f64 * 0.05
}
```

## Test Cases

### Positive: Single argument

```sql
SELECT GREATEST(price) FROM products;
-- Rewrite to: SELECT price FROM products;
```

### Positive: Duplicate arguments

```sql
SELECT GREATEST(col, col) FROM t;
-- Rewrite to: SELECT col FROM t;
```

### Positive: Constant folding

```sql
SELECT GREATEST(10, 20, 5) FROM t;
-- Fold to: SELECT 20 FROM t;
```

### Negative: All dynamic arguments

```sql
SELECT GREATEST(a, b, c) FROM t;
-- Cannot simplify without knowing values
```

## References

**Implementation:**
- PostgreSQL: GREATEST/LEAST as variadic functions
- MySQL: GREATEST/LEAST with implicit type conversion
- Oracle: GREATEST/LEAST with NULL handling differences
