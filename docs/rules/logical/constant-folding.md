# Rule: Constant Folding

**Category:** logical/expression-simplification
**File:** `rules/logical/expression-simplification/constant-folding.rra`

## Metadata

- **ID:** `constant-folding`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, oracle, mssql
- **Tags:** expression, constant, folding, simplification, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"


# Constant Folding

## Description

Evaluates expressions composed entirely of constants at compile time,
replacing them with a single literal value. This eliminates redundant
computation that would otherwise be repeated for every row.

**When to apply**: An expression or sub-expression contains only literal
values and deterministic functions with no column references.

**Why it works**: If every operand is known at plan time, the result is
also known and can be materialized once instead of evaluated per row.

## Relational Algebra

```algebra
sigma[f(constants)](R) -> sigma[result_literal](R)
pi[f(constants), ...](R) -> pi[result_literal, ...](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("fold-add";
    "(+ ?a ?b)" => { FoldConst("?a", "?b", Op::Add) }
    if both_const("?a", "?b")
),

rw!("fold-mul";
    "(* ?a ?b)" => { FoldConst("?a", "?b", Op::Mul) }
    if both_const("?a", "?b")
),

rw!("fold-comparison";
    "(= ?a ?b)" => { FoldConst("?a", "?b", Op::Eq) }
    if both_const("?a", "?b")
),

rw!("fold-not";
    "(not ?a)" => { FoldConst1("?a", Op::Not) }
    if is_const("?a")
),
```

## Preconditions

```rust
fn applicable(expr: &Expr) -> bool {
    // All leaf nodes must be literals (no column references)
    expr.children().all(|c| c.is_literal())
    // Function must be deterministic
    && expr.is_deterministic()
}
```

**Restrictions:**
- Functions must be deterministic (`random()`, `now()`, `nextval()` are excluded)
- Must handle overflow and division-by-zero at fold time
- NULL propagation must follow SQL three-valued logic

## Cost Model

```rust
fn estimated_benefit(
    input_card: f64,
    num_foldable_exprs: usize,
) -> f64 {
    // Each folded expression saves one evaluation per row.
    let eval_cost_per_row = num_foldable_exprs as f64 * EXPR_EVAL_COST;
    let total_saved = input_card * eval_cost_per_row;
    total_saved / (input_card * (eval_cost_per_row + BASE_ROW_COST))
}
```

**Typical benefit**: Small per-expression but compounds across the plan.
Critical for enabling other simplifications (e.g., `WHERE 1=1` becomes `WHERE TRUE`).

## Test Cases

```sql
-- Positive: arithmetic folding
-- Before
SELECT x, 2 + 3 AS five FROM t;
-- After
SELECT x, 5 AS five FROM t;
```

```sql
-- Positive: comparison folding in WHERE
-- Before
SELECT * FROM t WHERE 10 > 5;
-- After
SELECT * FROM t WHERE TRUE;
-- (which can then be eliminated entirely)
```

```sql
-- Positive: string concatenation
-- Before
SELECT 'hello' || ' ' || 'world' AS greeting FROM t;
-- After
SELECT 'hello world' AS greeting FROM t;
```

```sql
-- Negative: non-deterministic function
SELECT random() + 1 FROM t;
-- Cannot fold: random() returns different values per call
```

```sql
-- Negative: column reference present
SELECT x + 1 FROM t;
-- Cannot fold: x is a column, not a constant
```

## References

PostgreSQL: src/backend/optimizer/util/clauses.c - eval_const_expressions()
MySQL: sql/sql_optimizer.cc - fold_condition()
DuckDB: src/optimizer/expression_rewriter.cpp
Aho, Sethi, Ullman "Compilers: Principles, Techniques, and Tools" Section 8.5
