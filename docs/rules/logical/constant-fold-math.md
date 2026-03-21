# Rule: Constant Fold Math Functions

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-fold-math.rra`

## Metadata

- **ID:** `constant-fold-math`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, constant-folding, math, optimization
- **Authors:** "RA Contributors"


# Constant Fold Math Functions

## Description

Evaluates math functions with all-constant arguments at plan time,
replacing the function call with its result literal. Eliminates
per-row computation overhead for expressions like ABS(-5), SQRT(16),
POWER(2, 10).

**When to apply**: A math function call has all constant arguments
and the function is marked as `constant_foldable` in the catalog.

**Why it works**: Deterministic functions with constant inputs always
produce the same result. Computing once at plan time saves per-row
evaluation cost.

## Implementation

```rust
rw!("constant-fold-abs";
    "(abs ?n)" => "(literal (eval-abs ?n))"
    if is_constant("?n")
),

rw!("constant-fold-sqrt";
    "(sqrt ?n)" => "(literal (eval-sqrt ?n))"
    if is_constant("?n")
),

rw!("constant-fold-power";
    "(power ?base ?exp)" => "(literal (eval-power ?base ?exp))"
    if is_constant("?base") if is_constant("?exp")
),

rw!("constant-fold-mod";
    "(mod ?a ?b)" => "(literal (eval-mod ?a ?b))"
    if is_constant("?a") if is_constant("?b")
),
```

## Test Cases

```sql
-- Positive: ABS of constant
SELECT ABS(-42) FROM dual;
-- Folded to: SELECT 42 FROM dual

-- Positive: SQRT of constant
SELECT SQRT(144.0) FROM dual;
-- Folded to: SELECT 12.0 FROM dual

-- Positive: POWER of constants
SELECT POWER(2, 10) FROM dual;
-- Folded to: SELECT 1024 FROM dual

-- Negative: column argument
SELECT ABS(balance) FROM accounts;
-- Cannot fold: balance is a column reference
```

## References

- functions.toml: ABS, SQRT, POWER marked constant_foldable = true
