# Rule: Constant Function Folding

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-function-folding.rra`

## Metadata

- **ID:** `constant-function-folding`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** function, constant, folding, compile-time, deterministic
- **Authors:** "RA Contributors"


# Constant Function Folding

## Description

Evaluates deterministic functions with constant arguments at query
compilation time, replacing the function call with its result. This
eliminates per-row function evaluation overhead.

**When to apply**: Function calls where all arguments are constants or
previously folded results, and the function is deterministic (same input
always produces the same output).

**Why it works**: A deterministic function with constant arguments will
return the same value for every row. Computing it once at plan time and
substituting the result avoids repeated evaluation.

## Relational Algebra

```algebra
f(const1, const2, ..., constN)
  -> evaluate_once(f(const1, const2, ..., constN))
  where f is deterministic (IMMUTABLE)

-- Examples:
LENGTH('hello')        -> 5
UPPER('hello')         -> 'HELLO'
2 + 3                  -> 5
EXTRACT(YEAR FROM DATE '2024-01-15') -> 2024
CAST('42' AS INTEGER)  -> 42
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("fold-constant-function";
    "(call ?func ?args)" =>
    { EvalConstantFunction { func: "?func", args: "?args" } }
    if all_args_constant("?args")
    if is_deterministic("?func")
),

struct EvalConstantFunction { func: Var, args: Var }

impl Applier for EvalConstantFunction {
    fn apply_one(&self, egraph: &mut EGraph, matched: &Subst) -> Vec<Id> {
        let func = &egraph[matched[self.func]];
        let args = extract_constants(&egraph[matched[self.args]]);
        match evaluate_function(func, &args) {
            Ok(result) => vec![egraph.add(Expr::Literal(result))],
            Err(_) => vec![], // Cannot fold if evaluation fails
        }
    }
}
```

**Restrictions:**
- VOLATILE functions (RANDOM, NOW, CLOCK_TIMESTAMP) must not be folded
- STABLE functions (CURRENT_TIMESTAMP) fold within a statement but not across
- Division by zero and overflow must be handled (do not fold if error)
- Locale-dependent functions (collation) need locale context

## Cost Model

```rust
fn estimated_benefit(func_cost: f64, num_rows: u64) -> f64 {
    let original_cost = func_cost * num_rows as f64;
    let folded_cost = func_cost; // evaluate once
    (original_cost - folded_cost) / original_cost
}
```

**Typical benefit**: Proportional to row count; huge for expensive functions

## Test Cases

### Positive: String function with literal arguments

```sql
SELECT * FROM users WHERE name = UPPER('john');
-- Fold to: WHERE name = 'JOHN'
```

### Positive: Arithmetic on constants

```sql
SELECT * FROM products WHERE price > 100 * 1.08;
-- Fold to: WHERE price > 108.0
```

### Positive: Date extraction from literal

```sql
SELECT * FROM events
WHERE EXTRACT(YEAR FROM DATE '2024-06-15') = event_year;
-- Fold to: WHERE 2024 = event_year
```

### Negative: VOLATILE function

```sql
SELECT * FROM users WHERE id = FLOOR(RANDOM() * 100);
-- RANDOM() is volatile: different each call, cannot fold
```

### Negative: Function with column argument

```sql
SELECT UPPER(name) FROM users;
-- name varies per row, cannot fold at compile time
```

## References

**Academic papers:**
- Chaudhuri, "An Overview of Query Optimization in Relational Systems", PODS 1998

**Implementation:**
- PostgreSQL: `eval_const_expressions()` in `clauses.c`
- MySQL: Constant folding in optimizer
- DuckDB: Expression evaluator constant folding pass
