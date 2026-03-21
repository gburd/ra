# Rule: Constant Fold Comparison Operators

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/constant-fold-comparison.rra`

## Metadata

- **ID:** `constant-fold-comparison`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, constant-folding, comparison, predicate
- **Authors:** "RA Contributors"


# Constant Fold Comparison Operators

## Description

Evaluates comparison operations between constants at plan time.
`5 > 3` becomes TRUE, `'a' = 'b'` becomes FALSE. When used in
filters, constant-true predicates are removed and constant-false
predicates eliminate the entire subtree.

**When to apply**: Both operands of a comparison are constants.

## Implementation

```rust
rw!("constant-fold-eq";
    "(= ?a ?b)" => "(literal (eval-eq ?a ?b))"
    if is_constant("?a") if is_constant("?b")
),
rw!("constant-fold-gt";
    "(> ?a ?b)" => "(literal (eval-gt ?a ?b))"
    if is_constant("?a") if is_constant("?b")
),
rw!("constant-fold-lt";
    "(< ?a ?b)" => "(literal (eval-lt ?a ?b))"
    if is_constant("?a") if is_constant("?b")
),
rw!("filter-always-true";
    "(filter true ?input)" => "?input"
),
rw!("filter-always-false";
    "(filter false ?input)" => "(empty-rel)"
),
```

## Test Cases

```sql
-- Positive: constant comparison in WHERE
SELECT * FROM emp WHERE 1 = 1;
-- Folded to: SELECT * FROM emp (filter removed)

-- Positive: constant false eliminates scan
SELECT * FROM emp WHERE 1 = 2;
-- Folded to: empty result set

-- Positive: mixed constant comparison
SELECT * FROM emp WHERE 5 > 3 AND deptno = 10;
-- 5 > 3 folded to TRUE; simplifies to WHERE deptno = 10

-- Negative: column comparison
SELECT * FROM emp WHERE salary > 50000;
-- Cannot fold: salary is a column
```

## References

- Calcite: ReduceExpressionsRule
