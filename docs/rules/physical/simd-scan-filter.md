# Rule: SIMD-Accelerated Scan and Filter

**Category:** physical/hardware
**File:** `rules/physical/hardware/simd-scan-filter.rra`

## Metadata

- **ID:** `simd-scan-filter`
- **Version:** "1.0.0"
- **Databases:** duckdb, monetdb, hyper, clickhouse, umbra
- **Tags:** physical, hardware, simd, vectorized, scan, filter, avx
- **Authors:** "Polychroniou, Raman, Ross"


# SIMD-Accelerated Scan and Filter

## Description

Uses SIMD (Single Instruction, Multiple Data) instructions to evaluate
filter predicates on multiple values simultaneously. For columnar data,
a comparison like `col > 100` can process 4-16 values per instruction
(depending on data width and SIMD register size). The result is a
selection vector that masks qualifying rows.

**When to apply**: Sequential scan with simple comparison predicates on
fixed-width numeric columns in columnar storage.

## Relational Algebra

```algebra
-- Before: scalar filter
sigma[price > 100.0](ColumnScan(orders.price))

-- After: SIMD-vectorized filter
SIMD_Filter(ColumnScan(orders.price), GT, 100.0, width=AVX2_256)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("simd-filter-int32";
    "(filter (> ?col ?const) (columnscan ?table ?col))" =>
    "(simd-filter-gt ?col ?const (columnscan ?table ?col))"
    if is_fixed_width_numeric("?col")
    if simd_available()
),
```

## Preconditions

```rust
fn applicable(pred: &Predicate, col: &Column) -> bool {
    // Column must be fixed-width (int32, int64, float64)
    col.is_fixed_width()
        // Predicate must be simple comparison
        && pred.is_simple_comparison()
        // SIMD instruction set available
        && system_supports_simd()
}
```

**Restrictions:**
- Variable-length types (VARCHAR) require different approach
- NULL handling needs separate mask
- Predicate must be a simple comparison (not complex expression)

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    simd_width: usize, // values per SIMD register
    predicate_cost_scalar: f64,
) -> f64 {
    let scalar_cost = rows * predicate_cost_scalar;
    let simd_cost = (rows / simd_width as f64)
        * predicate_cost_scalar * 1.2; // slight overhead
    scalar_cost - simd_cost
}
```

**Typical benefit**: 30-80% for scan-heavy queries on columnar data.

## Test Cases

```sql
-- Positive: simple comparison on integer column
SELECT * FROM lineitem WHERE l_quantity > 25;
-- SIMD: 8 int32 comparisons per AVX2 instruction

-- Positive: range predicate
SELECT * FROM orders WHERE o_totalprice BETWEEN 1000 AND 5000;
-- Two SIMD comparisons combined

-- Negative: string predicate (not fixed-width)
SELECT * FROM customer WHERE c_name LIKE '%Smith%';
```

## References

- Polychroniou, O. et al. "Rethinking SIMD Vectorization for In-Memory Databases" (SIGMOD 2015)
- Kersten, T. et al. "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask" (VLDB 2018)
