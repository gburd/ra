# Rule: Common Subexpression Elimination

**Category:** logical/expression-simplification
**File:** `rules/logical/expression-simplification/common-subexpression-elimination.rra`

## Metadata

- **ID:** `common-subexpression-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, oracle, mssql
- **Tags:** expression, cse, deduplication, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"


# Common Subexpression Elimination

## Description

Identifies identical sub-expressions that appear multiple times in the same
operator and computes them only once, reusing the result. This avoids
redundant evaluation of expensive expressions like function calls,
arithmetic, or CASE expressions.

**When to apply**: The same deterministic sub-expression appears more than
once within a single operator's expression list or predicate.

**Why it works**: A deterministic expression evaluated on the same input
row always produces the same result, so computing it once and referencing
the cached value is semantically equivalent.

## Relational Algebra

```algebra
pi[f(x), g(f(x))](R) -> pi[t1, g(t1)](pi[f(x) as t1, *](R))
  where f(x) appears multiple times
  where f is deterministic
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// CSE is typically implemented as an analysis pass rather than a
// pattern-based rewrite, because it requires detecting duplicates
// across an expression list.

rw!("cse-in-project";
    "(project (list ?e1 (call ?f ?e1) ?rest) ?input)" =>
    "(project (list ?t1 (call ?f ?t1) ?rest)
        (project (list (as (call ?noop ?e1) ?t1) ?input_cols) ?input))"
    if appears_multiple_times("?e1")
    if is_deterministic("?e1")
),
```

## Preconditions

```rust
fn applicable(exprs: &[Expr]) -> bool {
    // Must have at least one sub-expression that appears
    // more than once
    let mut seen = HashSet::new();
    for expr in exprs {
        for sub in expr.sub_expressions() {
            if sub.is_deterministic() && !seen.insert(sub.clone()) {
                return true;
            }
        }
    }
    false
}
```

**Restrictions:**
- Expressions must be deterministic (no `random()`, `now()`, etc.)
- Expressions must be evaluated in the same operator context (same row)
- Side-effecting functions must not be deduplicated
- The temp column introduces a dependency on evaluation order

## Cost Model

```rust
fn estimated_benefit(
    input_card: f64,
    expr_cost: f64,
    duplicate_count: usize,
) -> f64 {
    // Without CSE: expr evaluated duplicate_count times per row
    let cost_before = input_card * expr_cost * duplicate_count as f64;
    // With CSE: expr evaluated once per row, result reused
    let cost_after = input_card * expr_cost;
    (cost_before - cost_after) / cost_before
}
```

**Typical benefit**: Proportional to `(n-1)/n` where `n` is the number of
duplicate occurrences. Expensive expressions (regex, aggregation,
user-defined functions) benefit most.

## Test Cases

```sql
-- Positive: same expression in SELECT and WHERE
-- Before
SELECT UPPER(name), LENGTH(UPPER(name)) FROM employees
WHERE UPPER(name) LIKE 'A%';

-- After (conceptually: compute UPPER(name) once)
SELECT t.upper_name, LENGTH(t.upper_name) FROM (
    SELECT *, UPPER(name) AS upper_name FROM employees
) t WHERE t.upper_name LIKE 'A%';
```

```sql
-- Positive: repeated arithmetic
-- Before
SELECT (a + b) * c, (a + b) * d FROM t;

-- After
SELECT t1.sum_ab * c, t1.sum_ab * d FROM (
    SELECT *, (a + b) AS sum_ab FROM t
) t1;
```

```sql
-- Negative: non-deterministic function
SELECT random(), random() FROM t;
-- Cannot eliminate: each call must produce an independent result
```

```sql
-- Negative: different rows
SELECT (SELECT MAX(x) FROM t) FROM s;
-- Subquery is re-evaluated per row of s (unless decorrelated)
```

## References

PostgreSQL: src/backend/optimizer/util/clauses.c
DuckDB: src/optimizer/common_subexpressions.cpp
MySQL: sql/sql_optimizer.cc - substitute_for_best_equal_field()
Aho, Sethi, Ullman "Compilers: Principles, Techniques, and Tools" Section 8.5
