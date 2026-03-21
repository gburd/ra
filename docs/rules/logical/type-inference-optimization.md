# Rule: Type Inference Optimization

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/type-inference-optimization.rra`

## Metadata

- **ID:** `type-inference-optimization`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb
- **Tags:** function, type, inference, specialization, monomorphization
- **Authors:** "RA Contributors"


# Type Inference Optimization

## Description

Uses inferred or declared types to select specialized function
implementations, avoid runtime type checks, and enable type-specific
optimizations like SIMD for numeric operations.

**When to apply**: Polymorphic functions where input types are known at
plan time, enabling selection of type-specialized implementations.

## Relational Algebra

```algebra
-- Generic comparison -> specialized
compare(int_col, int_literal) -> compare_int(int_col, int_literal)

-- Polymorphic SUM -> typed SUM
SUM(int_col) -> sum_int64(int_col)  -- no overflow check needed for int32 input

-- Mixed-type comparison -> cast + specialized
int_col = float_literal -> float(int_col) = float_literal
  -- or: int_col = int(float_literal) if lossless
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("specialize-int-comparison";
    "(= ?col ?val)" => "(eq-int ?col ?val)"
    if is_integer_type("?col")
    if is_integer_type("?val")
),

rw!("specialize-float-comparison";
    "(= ?col ?val)" => "(eq-float ?col ?val)"
    if is_float_type("?col")
    if is_float_type("?val")
),

rw!("specialize-string-comparison";
    "(= ?col ?val)" => "(eq-string ?col ?val)"
    if is_string_type("?col")
    if is_string_type("?val")
),

rw!("specialize-sum-int";
    "(aggregate (sum ?col) ?input)" =>
    "(aggregate (sum-int64 ?col) ?input)"
    if is_integer_type("?col")
),
```

## Cost Model

```rust
fn estimated_benefit(num_rows: u64, avoids_type_check: bool) -> f64 {
    if avoids_type_check {
        // Runtime type dispatch per row: ~5ns saved per row
        (num_rows as f64 * 5e-9).min(0.4)
    } else {
        0.1
    }
}
```

## Test Cases

### Positive: Integer-specialized comparison

```sql
SELECT * FROM t WHERE int_col = 42;
-- Use integer comparison (no type dispatch)
-- Enables SIMD vectorized evaluation
```

### Positive: Typed aggregate

```sql
SELECT SUM(quantity) FROM order_items;
-- quantity is INT32: use int64 accumulator (no overflow for INT32)
-- Avoids decimal/arbitrary precision overhead
```

### Positive: Eliminate mixed-type cast

```sql
SELECT * FROM t WHERE int_col = 3.0;
-- 3.0 is losslessly convertible to integer 3
-- Rewrite to: WHERE int_col = 3 (integer comparison, index-eligible)
```

### Negative: Truly polymorphic function

```sql
SELECT * FROM t WHERE json_col->'key' = '"value"';
-- JSON comparison: type not known until runtime
-- Cannot specialize without runtime dispatch
```

## References

**Implementation:**
- DuckDB: Vectorized type-specialized execution
- PostgreSQL: pg_proc function resolution by argument types
- MonetDB: Type-specialized BAT operators
