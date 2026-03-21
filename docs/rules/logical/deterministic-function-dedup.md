# Rule: Deterministic Function Deduplication

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/deterministic-function-dedup.rra`

## Metadata

- **ID:** `deterministic-function-dedup`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql, oracle, duckdb
- **Tags:** function, deterministic, dedup, cse, common-subexpression
- **Authors:** "RA Contributors"


# Deterministic Function Deduplication

## Description

Identifies duplicate deterministic function calls with identical arguments
within the same query block and replaces them with a single evaluation
whose result is reused. This is a specialized form of common
subexpression elimination (CSE) for function calls.

**When to apply**: The same deterministic function with the same arguments
appears more than once in SELECT, WHERE, ORDER BY, or HAVING clauses.

**Why it works**: Deterministic functions produce the same output for the
same input. Evaluating once and reusing the result eliminates redundant
CPU work, which compounds with expensive functions.

## Relational Algebra

```algebra
pi[f(a), g(f(a))](sigma[f(a) > v](R))
  -> let t = f(a) in pi[t, g(t)](sigma[t > v](R))
  where is_deterministic(f)
```

## Implementation

```rust
rw!("dedup-deterministic-fn";
    "(apply-fn ?fn ?args)" =>
    "(ref ?cached_result)"
    if is_deterministic("?fn") &&
       already_computed("?fn", "?args")
),
```

## Test Cases

### Positive: Same function in SELECT and WHERE

```sql
-- Before:
SELECT LOWER(name), LENGTH(LOWER(name)) FROM users WHERE LOWER(name) = 'alice';

-- After: compute LOWER(name) once, reuse in all three positions
```

### Negative: Non-deterministic function

```sql
SELECT RANDOM(), RANDOM() FROM t;
-- Each call must produce a different value
```

## References

- Aho, Sethi, Ullman, "Compilers", Common Subexpression Elimination
- PostgreSQL: Expression deduplication in planner
