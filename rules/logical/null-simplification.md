# Null Constant Simplification

## Overview

Null constant simplification rules handle NULL propagation through various operators according to SQL's three-valued logic semantics.

## Rules

### AND with NULL
- `AND(NULL, x) -> NULL`
- `AND(x, NULL) -> NULL`
- NULL propagates through AND operations

### OR with NULL
- `OR(NULL, TRUE) -> TRUE`
- `OR(TRUE, NULL) -> TRUE`
- `OR(NULL, FALSE) -> NULL`
- `OR(FALSE, NULL) -> NULL`
- `OR(NULL, NULL) -> NULL`

### Comparisons with NULL
- All comparison operators (`=`, `!=`, `<`, `<=`, `>`, `>=`) with NULL operands yield NULL
- `EQ(NULL, x) -> NULL`
- `NE(NULL, x) -> NULL`
- `LT(NULL, x) -> NULL`
- etc.

### IS NULL / IS NOT NULL
- `IS_NULL(NULL) -> TRUE`
- `IS_NOT_NULL(NULL) -> FALSE`

### Arithmetic with NULL
- All arithmetic operations (`+`, `-`, `*`, `/`, `%`) with NULL operands yield NULL
- `ADD(NULL, x) -> NULL`
- `SUB(NULL, x) -> NULL`
- etc.

### Filter with NULL predicate
- `FILTER(NULL, input) -> FILTER(FALSE, input)` (no rows pass)

## Benefits

1. **Correctness**: Ensures proper NULL handling according to SQL standard
2. **Early termination**: Simplifies expressions with NULL constants early
3. **Proptest fix**: Resolves saturation issues with NULL predicates in property tests
4. **Performance**: Reduces expression complexity during optimization

## Implementation

Located in: `crates/ra-engine/src/null_simplification.rs`

Priority: **HIGH** - Applied first in the rule set to prevent proptest saturation issues.

## Testing

- Unit tests for each NULL propagation pattern
- Property tests to ensure no saturation with NULL values
- Integration tests with complex queries containing NULLs