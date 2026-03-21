# Rule: CASE Expression Simplification

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/case-simplification.rra`

## Metadata

- **ID:** `case-simplification`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** function, case, simplification, conditional, elimination
- **Authors:** "RA Contributors"


# CASE Expression Simplification

## Description

Simplifies CASE expressions by evaluating constant conditions, removing
unreachable branches, and converting degenerate cases to simpler forms.

**When to apply**: CASE expressions where:
- A WHEN condition is a constant TRUE (always taken)
- A WHEN condition is a constant FALSE (dead branch)
- All branches return the same value
- Simple CASE can be converted to COALESCE or direct comparison
- Nested CASE expressions can be flattened

**Why it works**: Each CASE branch requires per-row condition evaluation.
Eliminating dead branches and constant-folding conditions reduces this
overhead.

## Relational Algebra

```algebra
CASE WHEN TRUE THEN x ELSE y END -> x
CASE WHEN FALSE THEN x ELSE y END -> y
CASE WHEN cond THEN x ELSE x END -> x
CASE col WHEN NULL THEN x ELSE y END -> y  (NULL never matches in simple CASE)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("case-true-condition";
    "(case (literal true) ?then ?else)" => "?then"
),

rw!("case-false-condition";
    "(case (literal false) ?then ?else)" => "?else"
),

rw!("case-same-branches";
    "(case ?cond ?val ?val)" => "?val"
),

rw!("simple-case-null-when";
    "(simple-case ?expr (literal null) ?then ?rest)" =>
    "(simple-case ?expr ?rest)"
    // NULL never matches in simple CASE (uses = comparison)
),

rw!("case-single-branch-to-if-null";
    "(case (is-null ?col) ?default ?col)" =>
    "(coalesce ?col ?default)"
),
```

## Cost Model

```rust
fn estimated_benefit(branches_removed: usize, total_branches: usize) -> f64 {
    branches_removed as f64 / total_branches as f64
}
```

## Test Cases

### Positive: Constant TRUE condition

```sql
SELECT CASE WHEN 1=1 THEN 'yes' ELSE 'no' END FROM t;
-- Fold to: SELECT 'yes' FROM t;
```

### Positive: All branches return same value

```sql
SELECT CASE status
  WHEN 'active' THEN price * 1.0
  WHEN 'inactive' THEN price * 1.0
  ELSE price * 1.0
END FROM products;
-- All branches identical -> SELECT price * 1.0 FROM products;
```

### Positive: NULL WHEN branch in simple CASE

```sql
SELECT CASE dept_id WHEN NULL THEN 'none' WHEN 1 THEN 'eng' ELSE 'other' END;
-- CASE uses = comparison: NULL = NULL is UNKNOWN, never matches
-- Remove dead branch: CASE dept_id WHEN 1 THEN 'eng' ELSE 'other' END
```

### Negative: All branches have different runtime conditions

```sql
SELECT CASE WHEN age < 18 THEN 'minor'
            WHEN age < 65 THEN 'adult'
            ELSE 'senior' END FROM users;
-- All conditions are runtime-dependent, cannot simplify
```

## References

**Implementation:**
- PostgreSQL: `simplify_boolean_equality()` and CASE optimization
- MySQL: CASE constant folding in optimizer
- DuckDB: CASE simplification in expression binder
