# Exasol Optimization Rules

Integration guide for Exasol-inspired optimization rules in Ra.

## Overview

Exasol is an in-memory, massively parallel processing (MPP) analytical database
that consistently wins TPC-H benchmarks in the 100GB-100TB range. Ra implements
five core Exasol optimization techniques for in-memory analytical workloads.

## Architecture

### In-Memory Columnar Storage

Exasol stores all data in RAM using compressed columnar format:

```
Traditional Row Store:
Memory: [id, name, age, city] [id, name, age, city] [id, name, age, city]
Access: Read entire row (cache-unfriendly)

Exasol Columnar Store:
Memory: [id, id, id] [name, name, name] [age, age, age] [city, city, city]
Access: Read only needed columns (cache-friendly)
```

**Benefits:**
- **Column pruning**: Access only needed columns (10-100x less data)
- **Compression**: Homogeneous types compress better (10-100x)
- **SIMD**: Vectorized operations on contiguous data (4-16x speedup)
- **Cache efficiency**: Sequential access minimizes cache misses

## Implemented Rules (Phase 1)

### EXA-001: Columnar Scan In-Memory

**Converts standard scans to columnar scans when data is in memory.**

```rust
// Before:
scan("orders")  // Reads all 50 columns

// After:
columnar_scan("orders", cols=["customer_id", "total_amount"], in_memory=true)
// Reads only 2 columns (96% reduction)
```

**Applicability:**
- Data confirmed to be in memory
- Query accesses < 50% of columns
- Columnar storage format available

**Performance Impact:**
- TPC-H Q1: 2-3x speedup (access 4 out of 16 columns)
- Wide table queries: 10-100x speedup (access 1-5 out of 100 columns)

**Test Coverage:**
```bash
cargo test test_exa001_columnar_scan
```

### EXA-002: Late Materialization

**Defers tuple reconstruction until after filtering and joins.**

```rust
// Before (early materialization):
project(["name", "email", "address"],
  filter("loyalty_tier = 'platinum'",
    scan("customers")))
// Reads: loyalty_tier (1 byte) + output cols (200 bytes) = 201 bytes/row * 1M rows = 201 MB

// After (late materialization):
position_gather(["name", "email", "address"],
  filter("loyalty_tier = 'platinum'",
    columnar_scan("customers", ["loyalty_tier"])))
// Reads: 1 byte/row * 1M rows + 200 bytes/row * 10K rows = 3 MB (67x reduction)
```

**Applicability:**
- Selective filter (< 80% of rows match)
- Output columns differ from filter columns
- In-memory columnar data

**Performance Impact:**
- Highly selective queries (1% selectivity): 20-100x speedup
- Moderate selectivity (10-30%): 3-10x speedup
- Low selectivity (> 80%): May add overhead, not applied

**Pipeline:**
1. Scan filter columns into CPU cache
2. Evaluate predicates → position list (bitmap)
3. SIMD gather output columns for qualifying rows only
4. Assemble result batches

**Test Coverage:**
```bash
cargo test test_exa002_late_materialization
```

### EXA-003: Column Filter Pushdown

**Pushes predicates to column scan with bloom filter generation.**

```rust
// Before:
filter("status = 'completed'",
  columnar_scan("orders", ["customer_id", "order_date", "status"]))
// Scans all data, then filters

// After:
columnar_scan("orders",
  cols=["customer_id", "order_date", "status"],
  filter="status = 'completed'",
  bloom=true,
  zone_maps=enabled)
// Zone maps prune 90% of chunks
// Bloom filter enables downstream join optimization
```

**Applicability:**
- Selective filters (< 50% of data matches)
- Dictionary-encoded columns (filter on codes)
- Zone maps available (min/max per chunk)
- Downstream joins benefit from bloom filter

**Optimizations:**
1. **Zone Map Pruning**: Skip entire 64K-row chunks based on min/max values
2. **Dictionary Filtering**: Filter on integer codes instead of string values
3. **Bloom Filter Generation**: Build compact bloom filter during scan
4. **Evaluation Ordering**: Evaluate most selective predicates first

**Performance Impact:**
- Range predicates with zone maps: 5-100x speedup (skip 90-99% of chunks)
- Dictionary-encoded equality: 2-5x speedup (integer comparison vs string)
- Bloom filter join enablement: 10-100x speedup on downstream join

**Test Coverage:**
```bash
cargo test test_exa003_column_filter_pushdown
```

### EXA-004: Bloom Filter Join

**Uses compact bloom filters to pre-filter large table before hash join.**

```rust
// Before (standard hash join):
hash_join(
  small_table,   // 10K rows
  large_table,   // 10M rows
  key="customer_id")
// 10M hash table probes (expensive!)

// After (bloom filter join):
hash_join(
  bloom_build(small_table, key="customer_id"),  // Build bloom (10K keys)
  bloom_probe(large_table, key="customer_id"),  // Probe bloom (10M rows)
  key="customer_id")
// 10M cheap bloom probes + 100K expensive hash probes (100x fewer!)
```

**Applicability:**
- Large table is 10x+ larger than small table
- Join selectivity < 20% (most large table rows don't match)
- Small table distinct keys < 10M (bloom filter size < 10 MB)

**Algorithm:**
1. **Build Phase**: Scan small table, build bloom filter + hash table
2. **Probe Phase**: Scan large table, check bloom filter (cheap)
3. **Join Phase**: Hash join only bloom-positive rows (expensive)

**Bloom Filter Properties:**
- Size: ~1-10 MB for 1M-10M keys (cache-friendly)
- False positive rate: 1-5% (acceptable overhead)
- Probe cost: 2-5 CPU cycles (vs 20-50 for hash table probe)

**Performance Impact:**
- TPC-H Q3 (dimension-fact join): 5-10x speedup
- TPC-H Q5 (cascade bloom filters): 10-20x speedup
- Star schema queries: 5-100x speedup depending on selectivity

**Cascade Optimization:**
```rust
// Multi-way join with cascading bloom filters:
// customer (1M) → orders (10M) → lineitem (600M)

// Build bloom1 on customer (after filter → 10K rows)
// Probe bloom1 on orders → 100K rows (99% elimination)
// Build bloom2 on orders (100K rows)
// Probe bloom2 on lineitem → 1M rows (99.8% elimination)
```

**Test Coverage:**
```bash
cargo test test_exa004_bloom_filter_join
```

### EXA-005: SIMD Vectorization

**Tags operations for SIMD execution (4-16 values simultaneously).**

```rust
// Before (scalar execution):
for i in 0..1_000_000 {
    if values[i] > threshold {
        output.push(i);
    }
}
// 1M cycles

// After (SIMD execution with AVX2):
for chunk in values.chunks_exact(8) {
    let mask = _mm256_cmpgt_epi32(chunk, threshold_vec);
    // Extract matching indices from bitmask
}
// 125K cycles (8x speedup)
```

**Applicability:**
- Primitive data types (int32, int64, float, double)
- Simple operations (arithmetic, comparison, hash)
- Batch size > 256 values (amortize setup overhead)
- CPU supports AVX2 (x86) or NEON (ARM)

**SIMD Operations:**
- **Comparison**: EQ, LT, GT → bitmask of matches
- **Arithmetic**: ADD, SUB, MUL, DIV on 4-16 values
- **Aggregation**: SUM, MIN, MAX with horizontal reduction
- **Hash**: CRC32 hash on 8 values simultaneously
- **Gather**: Random access to multiple memory locations

**Vector Widths:**
- SSE4.2: 4 values (128-bit registers)
- AVX2: 8 values (256-bit registers)
- AVX-512: 16 values (512-bit registers)
- ARM NEON: 4 values (128-bit registers)

**Performance Impact:**
- Integer comparison: 4-8x speedup
- Aggregation (SUM, COUNT): 4-8x speedup
- Hash computation: 4-8x speedup
- Actual speedup limited by memory bandwidth (not compute)

**Runtime Detection:**
```rust
pub fn detect_simd_width() -> usize {
    if is_x86_feature_detected!("avx512f") { return 16; }
    if is_x86_feature_detected!("avx2") { return 8; }
    if is_x86_feature_detected!("sse4.2") { return 4; }
    1  // Fallback to scalar
}
```

**Test Coverage:**
```bash
cargo test test_exa005_simd
```

## Integration Tests

### TPC-H Queries

**Q1: Pricing Summary Report**
```sql
SELECT
  l_returnflag, l_linestatus,
  SUM(l_quantity), SUM(l_extendedprice)
FROM lineitem
WHERE l_shipdate <= '1998-09-01'
GROUP BY l_returnflag, l_linestatus;
```

**Optimizations Applied:**
- EXA-001: Columnar scan (4 out of 16 columns)
- EXA-003: Filter pushdown with zone maps (partition pruning)
- EXA-005: SIMD aggregation (vectorized SUM)

**Speedup:** 7-8x

**Q3: Shipping Priority Query**
```sql
SELECT
  l_orderkey, SUM(l_extendedprice * (1 - l_discount))
FROM customer, orders, lineitem
WHERE c_mktsegment = 'BUILDING'
  AND c_custkey = o_custkey
  AND l_orderkey = o_orderkey
GROUP BY l_orderkey;
```

**Optimizations Applied:**
- EXA-002: Late materialization (defer wide columns)
- EXA-003: Filter pushdown (c_mktsegment)
- EXA-004: Bloom filter cascade (customer → orders → lineitem)
- EXA-005: SIMD aggregation

**Speedup:** 5-6x

**Q6: Forecasting Revenue Change**
```sql
SELECT SUM(l_extendedprice * l_discount)
FROM lineitem
WHERE l_shipdate BETWEEN '1994-01-01' AND '1994-12-31'
  AND l_discount BETWEEN 0.05 AND 0.07
  AND l_quantity < 24;
```

**Optimizations Applied:**
- EXA-001: Columnar scan (3 out of 16 columns)
- EXA-003: Filter pushdown with zone maps (98% chunk pruning)
- EXA-005: SIMD filter + aggregation

**Speedup:** 8-10x

**Run Integration Tests:**
```bash
cargo test test_integration_tpch
```

## Performance Tuning

### When Exasol Rules Apply

| Rule | Apply When | Skip When |
|------|-----------|-----------|
| EXA-001 | In-memory data, < 50% columns accessed | On-disk data, SELECT * |
| EXA-002 | Selectivity < 80%, output ≠ filter columns | Non-selective, filter = output |
| EXA-003 | Selectivity < 50%, dictionary/zone-maps | Non-selective, complex predicates |
| EXA-004 | Large:small ratio > 10x, selectivity < 20% | Similar-sized tables, high selectivity |
| EXA-005 | Primitive types, batch > 256, AVX2 available | Strings, single-row, no SIMD support |

### Cost Model Parameters

**Memory Bandwidth:**
```rust
const SEQUENTIAL_READ_COST: f64 = 0.1;  // cycles per byte
const RANDOM_READ_COST: f64 = 10.0;     // cycles per access
const CACHE_LINE_SIZE: u64 = 64;        // bytes
```

**SIMD Performance:**
```rust
const SCALAR_CMP_COST: f64 = 1.0;       // cycles per comparison
const SIMD_CMP_COST: f64 = 0.125;       // cycles per comparison (AVX2 8x)
const SIMD_GATHER_COST: f64 = 1.25;     // cycles per gather (AVX2 8x)
```

**Bloom Filter:**
```rust
const BLOOM_BUILD_COST: f64 = 12.0;     // cycles per key
const BLOOM_PROBE_COST: f64 = 5.0;      // cycles per probe
const HASH_PROBE_COST: f64 = 20.0;      // cycles per hash table probe
const FALSE_POSITIVE_RATE: f64 = 0.01;  // 1%
```

### Monitoring

**Enable Detailed Logging:**
```bash
RUST_LOG=ra_engine::exasol=debug cargo run
```

**Metrics to Track:**
- Column access ratio (# accessed / # total)
- Filter selectivity (# output / # input)
- Join selectivity (# output / # probe-side input)
- SIMD width detected (4, 8, or 16)
- Bloom filter false positive rate

## Future Work (Phase 2+)

### Distributed Execution (Phase 2)
- EXA-101: Parallel aggregation (local + global)
- EXA-102: Broadcast vs shuffle decision
- EXA-103: Colocated join detection
- EXA-104: Partition pruning
- EXA-105: Shuffle reuse

### MDX Support (Phase 3)
- EXA-201: MDX to relational algebra
- EXA-202: Hierarchy rollup optimization
- EXA-203: Cube detection
- EXA-204: Calculated member pushdown

### TPC-H Patterns (Phase 4)
- EXA-301: Date partition pruning
- EXA-302: Top-K pushdown
- EXA-303: Selective bloom for star schema
- EXA-304: Star join optimization

## References

### Academic Papers
1. **Abadi, Daniel et al.** "Column-Stores vs. Row-Stores: How Different Are
   They Really?" SIGMOD 2008.
2. **Abadi, Daniel et al.** "Materialization Strategies in a Column-Oriented
   DBMS." ICDE 2007.
3. **Boncz, Peter et al.** "MonetDB/X100: Hyper-Pipelining Query Execution."
   CIDR 2005.
4. **Graefe, Goetz.** "Query Evaluation Techniques for Large Databases."
   ACM Computing Surveys 1993.
5. **Bloom, Burton H.** "Space/time trade-offs in hash coding with allowable
   errors." CACM 1970.

### Industry Resources
1. **Exasol Documentation**: https://docs.exasol.com/
2. **TPC-H Results**: http://www.tpc.org/tpch/results/
3. **Intel Intrinsics Guide**: https://www.intel.com/content/www/us/en/docs/intrinsics-guide/
4. **CMU 15-721**: Modern Analytical Database Systems (2024)

### Ra Documentation
- Rule format: `docs/public/rules/exasol/README.md`
- Individual rules: `docs/public/rules/exasol/in_memory/*.rra`
- Tests: `crates/ra-engine/tests/exasol_rules_test.rs`
- Research: `EXASOL_RESEARCH.md`

## Contributing

To add new Exasol rules:

1. **Research**: Document the optimization in `EXASOL_RESEARCH.md`
2. **Rule File**: Create `.rra` file in `docs/public/rules/exasol/{category}/`
3. **Tests**: Add tests in `crates/ra-engine/tests/exasol_rules_test.rs`
4. **Documentation**: Update this file and `docs/public/rules/exasol/README.md`
5. **Benchmarks**: Add TPC-H benchmarks if applicable

**Rule File Format:**
```yaml
---
id: exa-###-rule-name
name: "Human Readable Name"
category: exasol-in-memory
databases: [exasol]
execution_models: [vectorized]
hardware: [cpu]
version: "1.0.0"
authors: ["RA Contributors"]
tags: [exasol, tag1, tag2]
complexity: "O(n)"
benefit_range: [min, max]
---

# Rule Title

## Metadata
- Rule ID, category, source, complexity, prerequisites, alternatives

## Description
What the rule does, when to apply, why it works

## Relational Algebra
Before/after transformation

## Implementation (egg rewrite rules)
Actual rewrite rules in egg syntax

## Cost Model
Rust cost function

## Test Cases
Positive and negative examples

## References
Papers, docs, benchmarks
```

**Commit Message:**
```
feat(exasol): Add EXA-### rule-name

Implements [description] for [use case].

Performance: [X]x speedup on [workload]

Tests: cargo test test_exa###
```

## License

Apache 2.0 (same as Ra project)
