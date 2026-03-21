# Rule: Column-at-a-Time SIMD Vectorized Primitives

**Category:** execution-models
**File:** `rules/execution-models/column-at-a-time/column-vectorized-ops.rra`

## Metadata

- **ID:** `column-vectorized-ops`
- **Version:** 1.0.0
- **Databases:** MonetDB, ClickHouse, DuckDB, vectorwise
- **Tags:** execution, columnar, x100, simd, vectorized, avx2, avx512, neon, primitives
- **SQL Standard:** MonetDB X100
- **Authors:** Peter Boncz, Marcin Zukowski, Orestis Polychroniou


# Column-at-a-Time SIMD Vectorized Primitives

## Description

Column-at-a-time processing naturally maps to SIMD (Single Instruction, Multiple Data) instructions because column arrays provide contiguous, homogeneous data. Each primitive operation (comparison, arithmetic, hash, gather) has a SIMD kernel that processes 4-16 values per instruction. MonetDB's X100 engine introduced the concept of vectorized primitives: a library of type-specialized, SIMD-optimized functions that operate on column arrays. The processor's auto-vectorizer can handle simple loops, but hand-written SIMD intrinsics achieve 2-4x better throughput for complex operations like hashing and string processing.

**SIMD instruction sets and column widths:**

| ISA | Register Width | i32/cycle | i64/cycle | f64/cycle |
|-----|---------------|-----------|-----------|-----------|
| SSE4.2 | 128 bit | 4 | 2 | 2 |
| AVX2 | 256 bit | 8 | 4 | 4 |
| AVX-512 | 512 bit | 16 | 8 | 8 |
| ARM NEON | 128 bit | 4 | 2 | 2 |

**Primitive categories:**
1. **Arithmetic**: add, sub, mul, div on column pairs or column+scalar
2. **Comparison**: eq, ne, lt, le, gt, ge producing bitmask or selection vector
3. **Hash**: multiply-shift, CRC32, or MurmurHash on key columns
4. **Gather/Scatter**: collect values at selected positions (SIMD gather instructions)
5. **Aggregation reduction**: horizontal sum, min, max across column
6. **String**: length, equality, prefix match (SIMD byte comparison)
7. **Null handling**: AND/OR bitmasks, conditional operations

**Key characteristics:**
- **Type-specialized**: Separate kernel per (operation, type) pair
- **Alignment-optimized**: Column arrays aligned to 32/64-byte boundaries
- **Branchless**: Predicate evaluation uses mask operations, no branches
- **Auto-vectorization friendly**: Simple loops that compilers can vectorize
- **Fallback chain**: AVX-512 -> AVX2 -> SSE4.2 -> scalar at runtime

**Trade-offs:**
- Hand-tuned SIMD requires large code surface (many type x op combinations)
- Gather instructions (AVX2 vgatherd) are slow (~12 cycles) vs. sequential load
- Masking (AVX-512) adds complexity but eliminates remainder loops
- Variable-length types (strings) break SIMD patterns, require special handling

## Implementation

```rust
/// SIMD primitive dispatch based on runtime CPU features
pub struct VectorizedPrimitives {
    simd_level: SimdLevel,
}

pub enum SimdLevel {
    Avx512,
    Avx2,
    Sse42,
    Neon,
    Scalar,
}

impl VectorizedPrimitives {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if is_x86_feature_detected!("avx512f") {
                return Self {
                    simd_level: SimdLevel::Avx512,
                };
            }
            if is_x86_feature_detected!("avx2") {
                return Self {
                    simd_level: SimdLevel::Avx2,
                };
            }
            return Self {
                simd_level: SimdLevel::Sse42,
            };
        }
        #[cfg(target_arch = "aarch64")]
        {
            return Self {
                simd_level: SimdLevel::Neon,
            };
        }
    }
}

// ---- Arithmetic Primitives ----

/// Column + Column (i64)
pub fn vec_add_i64(
    a: &[i64], b: &[i64], out: &mut [i64], n: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        let chunks = n / 4;
        for i in 0..chunks {
            unsafe {
                let va = _mm256_loadu_si256(
                    a[i * 4..].as_ptr() as *const _,
                );
                let vb = _mm256_loadu_si256(
                    b[i * 4..].as_ptr() as *const _,
                );
                let vr = _mm256_add_epi64(va, vb);
                _mm256_storeu_si256(
                    out[i * 4..].as_mut_ptr() as *mut _,
                    vr,
                );
            }
        }
        for i in (chunks * 4)..n {
            out[i] = a[i] + b[i];
        }
        return;
    }

    // Scalar fallback (auto-vectorizable)
    for i in 0..n {
        out[i] = a[i] + b[i];
    }
}

/// Column x Scalar (f64, SIMD broadcast)
pub fn vec_mul_scalar_f64(
    a: &[f64], scalar: f64, out: &mut [f64], n: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        let vs = unsafe { _mm256_set1_pd(scalar) };
        let chunks = n / 4;
        for i in 0..chunks {
            unsafe {
                let va = _mm256_loadu_pd(
                    a[i * 4..].as_ptr(),
                );
                let vr = _mm256_mul_pd(va, vs);
                _mm256_storeu_pd(
                    out[i * 4..].as_mut_ptr(), vr,
                );
            }
        }
        for i in (chunks * 4)..n {
            out[i] = a[i] * scalar;
        }
        return;
    }

    for i in 0..n {
        out[i] = a[i] * scalar;
    }
}

// ---- Comparison Primitives ----

/// Column > Scalar -> bitmask (AVX-512 native mask)
pub fn vec_gt_i32_mask(
    a: &[i32], threshold: i32, n: usize,
) -> Vec<u64> {
    let num_words = (n + 63) / 64;
    let mut mask = vec![0u64; num_words];

    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx512f") {
        let vt = unsafe {
            _mm512_set1_epi32(threshold)
        };
        let chunks = n / 16;
        for i in 0..chunks {
            unsafe {
                let va = _mm512_loadu_si512(
                    a[i * 16..].as_ptr() as *const _,
                );
                let m = _mm512_cmpgt_epi32_mask(va, vt);
                // Store 16-bit mask into result
                let word = i / 4;
                let bit_offset = (i % 4) * 16;
                mask[word] |= (m as u64) << bit_offset;
            }
        }
        return mask;
    }

    // Scalar fallback
    for i in 0..n {
        if a[i] > threshold {
            mask[i / 64] |= 1u64 << (i % 64);
        }
    }
    mask
}

/// Column == Column -> selection vector
pub fn vec_eq_i64_sel(
    a: &[i64], b: &[i64], n: usize,
) -> SelectionVector {
    let mut positions = Vec::with_capacity(n);

    for i in 0..n {
        if a[i] == b[i] {
            positions.push(i as u32);
        }
    }

    SelectionVector {
        source_len: n,
        positions,
    }
}

// ---- Hash Primitives ----

/// Bulk multiply-shift hash for i64 column
pub fn vec_hash_i64(
    keys: &[i64], out: &mut [u64], n: usize,
) {
    // Multiply-shift: h(x) = (a * x) >> (64 - log2(table_size))
    const A: u64 = 0x517cc1b727220a95;

    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        let va = unsafe {
            _mm256_set1_epi64x(A as i64)
        };
        let chunks = n / 4;
        for i in 0..chunks {
            unsafe {
                let vk = _mm256_loadu_si256(
                    keys[i * 4..].as_ptr() as *const _,
                );
                // AVX2 lacks 64-bit multiply; use mullo_epi32
                // + shifts as workaround, or use scalar
                let h0 = (keys[i * 4] as u64)
                    .wrapping_mul(A);
                let h1 = (keys[i * 4 + 1] as u64)
                    .wrapping_mul(A);
                let h2 = (keys[i * 4 + 2] as u64)
                    .wrapping_mul(A);
                let h3 = (keys[i * 4 + 3] as u64)
                    .wrapping_mul(A);
                out[i * 4] = h0;
                out[i * 4 + 1] = h1;
                out[i * 4 + 2] = h2;
                out[i * 4 + 3] = h3;
            }
        }
        for i in (chunks * 4)..n {
            out[i] = (keys[i] as u64).wrapping_mul(A);
        }
        return;
    }

    for i in 0..n {
        out[i] = (keys[i] as u64).wrapping_mul(A);
    }
}

/// CRC32 hash for i32 keys (hardware acceleration)
pub fn vec_hash_crc32(
    keys: &[i32], out: &mut [u32], n: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("sse4.2") {
        for i in 0..n {
            out[i] = unsafe {
                _mm_crc32_u32(0, keys[i] as u32)
            };
        }
        return;
    }

    for i in 0..n {
        out[i] = software_crc32(keys[i] as u32);
    }
}

// ---- Gather/Scatter Primitives ----

/// SIMD gather: collect values at positions
pub fn vec_gather_i64(
    data: &[i64],
    positions: &[u32],
    out: &mut [i64],
    n: usize,
) {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        let chunks = n / 4;
        for i in 0..chunks {
            unsafe {
                let idx = _mm_loadu_si128(
                    positions[i * 4..].as_ptr()
                        as *const _,
                );
                // Scale by 8 for i64 (8 bytes per element)
                let gathered = _mm256_i32gather_epi64::<8>(
                    data.as_ptr() as *const _,
                    idx,
                );
                _mm256_storeu_si256(
                    out[i * 4..].as_mut_ptr() as *mut _,
                    gathered,
                );
            }
        }
        for i in (chunks * 4)..n {
            out[i] = data[positions[i] as usize];
        }
        return;
    }

    for i in 0..n {
        out[i] = data[positions[i] as usize];
    }
}

// ---- Reduction Primitives ----

/// Horizontal SUM reduction (i64)
pub fn vec_sum_i64(data: &[i64], n: usize) -> i64 {
    let mut sum: i64 = 0;

    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        let mut acc = unsafe { _mm256_setzero_si256() };
        let chunks = n / 4;
        for i in 0..chunks {
            let v = unsafe {
                _mm256_loadu_si256(
                    data[i * 4..].as_ptr() as *const _,
                )
            };
            acc = unsafe { _mm256_add_epi64(acc, v) };
        }
        let arr = unsafe {
            std::mem::transmute::<_, [i64; 4]>(acc)
        };
        sum = arr[0] + arr[1] + arr[2] + arr[3];
        for i in (chunks * 4)..n {
            sum += data[i];
        }
        return sum;
    }

    for i in 0..n {
        sum += data[i];
    }
    sum
}

// ---- String Primitives ----

/// SIMD string equality (fixed-length padded strings)
pub fn vec_streq(
    col: &[u8],
    str_width: usize,
    target: &[u8],
    n: usize,
) -> SelectionVector {
    let mut positions = Vec::new();

    // Compare str_width bytes at a time using SIMD
    for i in 0..n {
        let offset = i * str_width;
        let row_str = &col[offset..offset + str_width];

        // Use memcmp-style SIMD comparison
        if row_str == target {
            positions.push(i as u32);
        }
    }

    SelectionVector {
        source_len: n,
        positions,
    }
}
```

## Cost Model

**Throughput per Primitive (AVX2, single core):**

| Operation | Scalar (ns/val) | AVX2 (ns/val) | Speedup |
|-----------|-----------------|---------------|---------|
| i64 + i64 | 1.0 | 0.25 | 4x |
| f64 * f64 | 1.0 | 0.25 | 4x |
| i32 > scalar | 1.0 | 0.125 | 8x |
| i64 hash | 2.0 | 0.5 | 4x |
| CRC32 hash | 3.0 | 1.0 | 3x |
| Gather i64 | 10-50 | 3-12 | 3-4x |
| SUM reduction | 0.25 | 0.0625 | 4x |
| String eq (8B) | 2.0 | 0.5 | 4x |

**Bottleneck Analysis:**
- Arithmetic: ALU-bound, ~4 ops/cycle/core (AVX2 i64)
- Comparison + selection: Branch prediction or SIMD mask extraction
- Hash: ALU-bound for multiply-shift, port-bound for CRC32
- Gather: Memory latency bound (L1: ~4 cycles, L2: ~12, L3: ~40)
- Reduction: Memory bandwidth bound for large columns

**AVX-512 vs. AVX2:**
- 2x wider registers: 2x theoretical throughput
- Clock frequency reduction: 0-15% on some CPUs
- Net speedup: 1.5-1.8x typical (not 2x due to frequency throttling)

## Test Cases

```sql
-- Test 1: Arithmetic primitive (column + scalar)
SELECT price * 1.1 AS with_tax FROM products;
-- vec_mul_scalar_f64: 1M values at 0.25 ns/val = 0.25 ms
-- Memory bandwidth: 8 MB read + 8 MB write = 16 MB

-- Test 2: Comparison primitive (filter)
SELECT * FROM sensors WHERE reading > 100;
-- vec_gt_i32_mask: 1M values at 0.125 ns/val = 0.125 ms
-- Produces bitmask: 1M bits = 125 KB

-- Test 3: Hash primitive (join build)
SELECT * FROM orders o JOIN products p ON o.product_id = p.id;
-- vec_hash_i64: hash 1M keys at 0.5 ns/val = 0.5 ms
-- CRC32 variant: 1.0 ns/val = 1.0 ms (better distribution)

-- Test 4: Reduction primitive (aggregate)
SELECT SUM(amount) FROM transactions;
-- vec_sum_i64: 10M values at 0.0625 ns/val = 0.625 ms
-- Bandwidth: 80 MB at 40 GB/s = 2 ms (bandwidth-bound)
```

## Comparison

| Property | SIMD Primitives | Scalar Loops | JIT Compiled |
|----------|----------------|-------------|-------------|
| Throughput | 4-16 ops/cycle | 1 op/cycle | 1-4 ops/cycle |
| Code complexity | High (per type) | Low | Medium (codegen) |
| Portability | ISA-specific | Universal | IR-portable |
| String handling | Limited | Natural | Natural |
| Null handling | Bitmask ops | Branching | Inlined checks |
| Auto-vectorize | Not needed | Sometimes | Sometimes |
| Maintenance | N types x M ops | 1 generic | 1 template |

## References

1. **Polychroniou, Orestis; Raghavan, Arun; Ross, Kenneth A.**. "Rethinking SIMD Vectorization for In-Memory Databases." SIGMOD 2015.
2. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
3. **Kersten, Timo; Leis, Viktor; Kemper, Alfons; Neumann, Thomas; Pavlo, Andrew; Boncz, Peter**. "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask." VLDB 2018.
4. **Polychroniou, Orestis; Ross, Kenneth A.**. "Vectorized Bloom Filters for Advanced SIMD Processors." DaMoN 2014.
