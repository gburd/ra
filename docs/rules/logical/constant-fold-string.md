# Rule: Constant Fold String Functions

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-fold-string.rra`

## Metadata

- **ID:** `constant-fold-string`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, constant-folding, string, optimization
- **Authors:** "RA Contributors"


# Constant Fold String Functions

## Description

Evaluates string functions with all-constant arguments at plan time.
Functions like UPPER('hello'), LENGTH('test'), CONCAT('a', 'b') are
replaced with their literal results.

**When to apply**: A string function has all constant arguments.

## Implementation

```rust
rw!("constant-fold-upper";
    "(upper ?s)" => "(literal (eval-upper ?s))"
    if is_constant("?s")
),
rw!("constant-fold-lower";
    "(lower ?s)" => "(literal (eval-lower ?s))"
    if is_constant("?s")
),
rw!("constant-fold-length";
    "(length ?s)" => "(literal (eval-length ?s))"
    if is_constant("?s")
),
rw!("constant-fold-concat";
    "(concat ?a ?b)" => "(literal (eval-concat ?a ?b))"
    if is_constant("?a") if is_constant("?b")
),
rw!("constant-fold-substring";
    "(substring ?s ?from ?len)" =>
    "(literal (eval-substring ?s ?from ?len))"
    if is_constant("?s") if is_constant("?from") if is_constant("?len")
),
```

## Test Cases

```sql
-- Positive: UPPER of constant
SELECT UPPER('hello') FROM dual;
-- Folded to: SELECT 'HELLO'

-- Positive: LENGTH of constant
SELECT LENGTH('database');
-- Folded to: SELECT 8

-- Positive: CONCAT of constants
SELECT CONCAT('hello', ' ', 'world');
-- Folded to: SELECT 'hello world'

-- Negative: column argument
SELECT UPPER(name) FROM users;
-- Cannot fold: name is a column
```

## References

- functions.toml: UPPER, LOWER, LENGTH, CONCAT marked constant_foldable = true
