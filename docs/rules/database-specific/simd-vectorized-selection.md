# Rule: MonetDB SIMD Vectorized Selection

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/simd-vectorized-selection.rra`

## Metadata

- **ID:** `monetdb-simd-vectorized-selection`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, simd, avx2, avx512, vectorized, selection
- **Authors:** "Polychroniou et al. 2015", "RA Contributors"


# MonetDB SIMD Vectorized Selection

## Description

Exploits CPU SIMD (Single Instruction Multiple Data) instructions
(SSE, AVX2, AVX-512) to evaluate selection predicates on multiple
column values simultaneously. A single AVX-512 instruction can compare
16 32-bit integers in one cycle, producing a bitmask of qualifying
rows. The resulting selection vector is used by subsequent operators
to skip non-qualifying tuples.

**When to apply**: Selection predicates on fixed-width numeric columns
where the column data is stored in a SIMD-friendly layout (contiguous,
aligned). Effective for both point and range predicates.

**Why it works**: SIMD registers are 128-512 bits wide. A 512-bit
AVX-512 register holds 16 32-bit integers. The VPCMPD instruction
compares all 16 against a broadcast predicate value in 1 cycle,
producing a 16-bit mask. Selection throughput increases 4-16x over
scalar code.

**Database version**: MonetDB/X100 (Vectorwise), MonetDB with vectorized
execution on AVX2+ CPUs

## Relational Algebra

```algebra
-- Scalar selection (1 value per cycle):
sigma[x > 100](R) -> for each row: if row.x > 100 then emit(row)

-- SIMD selection (16 values per cycle with AVX-512):
sigma[x > 100](R) -> for each 16-wide vector:
  mask = vpcmpd(vec, broadcast(100), GT)
  compress(vec, mask) -> selection_vector
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-simd-select";
    "(filter (compare ?op ?col ?val) (scan ?table))" =>
    "(simd-vectorized-select ?op ?col ?val ?table
       (simd_width (detect_simd_width)))"
    if is_database("monetdb")
    if is_numeric_fixed_width("?col")
    if column_is_aligned("?col")
    if cpu_supports_simd()
),
```

## Preconditions

```rust
fn applicable(
    column: &Column,
    hw: &HardwareProfile,
) -> bool {
    // Column must be fixed-width numeric
    if !matches!(column.data_type(),
        DataType::Int8 | DataType::Int16
        | DataType::Int32 | DataType::Int64
        | DataType::Float32 | DataType::Float64)
    {
        return false;
    }

    // CPU must support SIMD
    if !hw.supports_sse2() {
        return false;
    }

    // Data must be aligned for SIMD loads
    column.is_aligned(hw.simd_alignment())
}

fn simd_width(hw: &HardwareProfile) -> usize {
    if hw.supports_avx512() { 512 }
    else if hw.supports_avx2() { 256 }
    else if hw.supports_sse2() { 128 }
    else { 64 }
}
```

**Restrictions:**
- Column data must be contiguous and aligned (no gaps, no compression)
- Nullable columns need separate null bitmap check
- String/variable-length columns cannot use SIMD comparison directly
- Requires compile-time or JIT SIMD code generation
- Performance varies by CPU microarchitecture (port contention)

## Cost Model

```rust
fn estimated_benefit(
    total_rows: f64,
    value_width_bytes: usize,
    simd_width_bits: usize,
) -> f64 {
    let values_per_register = simd_width_bits / (value_width_bytes * 8);
    let scalar_cost = total_rows * 2.0; // compare + branch per value
    let simd_cost = (total_rows / values_per_register as f64) * 3.0;
    // load + compare + compress per register

    if scalar_cost > simd_cost {
        (scalar_cost - simd_cost) / scalar_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 8x-16x with AVX-512 on 32-bit columns.
4x-8x with AVX2. 2x-4x with SSE2.

## Test Cases

```sql
-- Positive: range predicate on 32-bit integer column
SELECT * FROM sensor_readings WHERE temperature > 30;
-- AVX-512: 16 comparisons per cycle, 16x throughput

-- Positive: equality predicate on 64-bit column
SELECT * FROM events WHERE event_type = 42;
-- AVX-512: 8 comparisons per cycle (64-bit values)
```

```sql
-- Negative: predicate on variable-length string
SELECT * FROM logs WHERE message LIKE '%error%';
-- Cannot use SIMD for string matching (use SIMD-based string search instead)
```

## References

Polychroniou, Raghavan, Ross, "Rethinking SIMD Vectorization for In-Memory
Databases", SIGMOD 2015
Boncz et al., "MonetDB/X100: Hyper-Pipelining Query Execution", CIDR 2005
Kersten et al., "Everything You Always Wanted to Know About Compiled and
Vectorized Queries But Were Afraid to Ask", VLDB 2018
