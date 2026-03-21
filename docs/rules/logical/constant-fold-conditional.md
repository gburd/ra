# Rule: Constant Fold Conditional Expressions

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-fold-conditional.rra`

## Metadata

- **ID:** `constant-fold-conditional`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, constant-folding, case, conditional
- **Authors:** "RA Contributors"


# Constant Fold Conditional Expressions

## Description

Simplifies CASE expressions and conditional functions when the
condition is a constant. `CASE WHEN TRUE THEN x ELSE y END` reduces
to x. `CASE WHEN FALSE THEN x ELSE y END` reduces to y.

**When to apply**: A CASE/IF/IIF expression has a constant condition.

## Implementation

```rust
rw!("constant-fold-case-true";
    "(case true ?then ?else)" => "?then"
),
rw!("constant-fold-case-false";
    "(case false ?then ?else)" => "?else"
),
rw!("constant-fold-case-null-else";
    "(case false ?then null)" => "null"
),
rw!("constant-fold-if-true";
    "(if true ?then ?else)" => "?then"
),
rw!("constant-fold-if-false";
    "(if false ?then ?else)" => "?else"
),
```

## Test Cases

```sql
-- Positive: constant TRUE condition
SELECT CASE WHEN 1 = 1 THEN 'yes' ELSE 'no' END;
-- Folded to: 'yes'

-- Positive: constant FALSE condition
SELECT CASE WHEN 1 = 2 THEN 'yes' ELSE 'no' END;
-- Folded to: 'no'

-- Positive: nested constant CASE
SELECT CASE WHEN TRUE THEN CASE WHEN FALSE THEN 1 ELSE 2 END END;
-- Folded to: 2

-- Negative: column-dependent condition
SELECT CASE WHEN status = 'A' THEN 1 ELSE 0 END FROM orders;
-- Cannot fold: depends on status column
```

## References

- ReduceExpressionsRule in Calcite
