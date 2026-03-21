# Rule: Constant Fold Logical Operators

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-fold-logical.rra`

## Metadata

- **ID:** `constant-fold-logical`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, constant-folding, boolean, and, or, not
- **Authors:** "RA Contributors"


# Constant Fold Logical Operators

## Description

Simplifies boolean expressions with constant operands. TRUE AND x
simplifies to x, FALSE OR x simplifies to x, NOT TRUE simplifies
to FALSE, and so on.

**When to apply**: A logical operator (AND, OR, NOT) has one or
more constant boolean operands.

## Implementation

```rust
rw!("fold-and-true";  "(and true ?x)"  => "?x"),
rw!("fold-and-false"; "(and false ?x)" => "false"),
rw!("fold-or-true";   "(or true ?x)"   => "true"),
rw!("fold-or-false";  "(or false ?x)"  => "?x"),
rw!("fold-not-true";  "(not true)"     => "false"),
rw!("fold-not-false"; "(not false)"    => "true"),
rw!("fold-not-not";   "(not (not ?x))" => "?x"),
```

## Test Cases

```sql
-- Positive: AND with TRUE
SELECT * FROM emp WHERE TRUE AND deptno = 10;
-- Simplified to: WHERE deptno = 10

-- Positive: OR with FALSE
SELECT * FROM emp WHERE FALSE OR salary > 50000;
-- Simplified to: WHERE salary > 50000

-- Positive: double negation
SELECT * FROM emp WHERE NOT NOT (salary > 50000);
-- Simplified to: WHERE salary > 50000

-- Positive: AND with FALSE eliminates branch
SELECT * FROM emp WHERE FALSE AND salary > 50000;
-- Simplified to: WHERE FALSE (empty result)
```

## References

- Calcite: ReduceExpressionsRule
- Boolean algebra identity laws
