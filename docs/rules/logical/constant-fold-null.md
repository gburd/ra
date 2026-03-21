# Rule: Constant Fold NULL Expressions

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-fold-null.rra`

## Metadata

- **ID:** `constant-fold-null`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, constant-folding, null, coalesce
- **Authors:** "RA Contributors"


# Constant Fold NULL Expressions

## Description

Simplifies expressions involving NULL constants. NULL + 5 evaluates
to NULL, COALESCE(NULL, 10) evaluates to 10, IS NULL on a non-null
constant evaluates to FALSE.

**When to apply**: An expression involves NULL literals combined
with other constants.

## Implementation

```rust
rw!("fold-null-arithmetic";
    "(+ null ?x)" => "null"
),
rw!("fold-null-comparison";
    "(= null ?x)" => "null"  // NULL = anything is NULL, not TRUE/FALSE
),
rw!("fold-coalesce-null-first";
    "(coalesce null ?x)" => "?x"
),
rw!("fold-coalesce-non-null";
    "(coalesce ?x ?y)" => "?x"
    if is_non_null_constant("?x")
),
rw!("fold-is-null-constant";
    "(is-null ?x)" => "false"
    if is_non_null_constant("?x")
),
rw!("fold-is-null-null";
    "(is-null null)" => "true"
),
rw!("fold-is-not-null-constant";
    "(is-not-null ?x)" => "true"
    if is_non_null_constant("?x")
),
```

## Test Cases

```sql
-- Positive: NULL arithmetic
SELECT NULL + 5;
-- Folded to: NULL

-- Positive: COALESCE with NULL first
SELECT COALESCE(NULL, 10);
-- Folded to: 10

-- Positive: COALESCE with non-null first
SELECT COALESCE(42, NULL, 10);
-- Folded to: 42

-- Positive: IS NULL on constant
SELECT * FROM emp WHERE 5 IS NULL;
-- Folded to: WHERE FALSE

-- Negative: IS NULL on column
SELECT * FROM emp WHERE commission IS NULL;
-- Cannot fold: commission may be NULL
```

## References

- SQL three-valued logic (TRUE, FALSE, NULL)
- Calcite: ReduceExpressionsRule NULL handling
