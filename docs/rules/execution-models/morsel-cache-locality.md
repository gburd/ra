# Rule: Morsel-Driven Cache Locality Optimization

**Category:** execution-models/morsel-driven
**File:** `rules/execution-models/morsel-driven/morsel-cache-locality.rra`

## Metadata

- **ID:** `morsel-cache-locality`
- **Version:** "1.0.0"
- **Databases:** hyper, umbra, duckdb
- **Tags:** execution, parallel, morsel, cache, locality, L1, L2, prefetch
- **Authors:** "Viktor Leis", "Thomas Neumann"


# Morsel-Driven Cache Locality Optimization

## Description

Sizes morsels to fit in CPU cache (L2 or L3) and structures operator execution
to maximize temporal and spatial locality. The morsel size is tuned so that the
working set of a pipeline (morsel data + operator state + intermediate results)
fits in the per-core L2 cache, minimizing cache misses and memory stalls.

**Cache-conscious design principles:**
- **Morsel size**: Tuned to L2 cache size (typically 256KB-1MB)
- **Pipeline fusion**: Multiple operators process the same morsel before
  moving to the next, keeping data hot in L1/L2
- **Prefetching**: Software prefetch instructions issued for hash table
  probes and random access patterns
- **Data layout**: Column-major for scan-heavy, row-major for join-heavy

**Cache hierarchy interaction:**
- L1 (32-64 KB): Current morsel's active rows + operator state
- L2 (256 KB-1 MB): Full morsel data + hash table partition
- L3 (shared): Hash table overflow, multiple concurrent morsels

## Relational Algebra

```
Cache-optimal morsel sizing:
  morsel_rows = L2_size / (row_width * pipeline_width_factor)

  For L2 = 256 KB, row_width = 100 bytes, factor = 2:
    morsel_rows = 256000 / (100 * 2) = 1280 rows

Pipeline fusion for locality:
  // Bad: process all morsels through op1, then all through op2
  for morsel in all_morsels:
    op1.process(morsel)        -- morsel evicted from cache
  for morsel in all_morsels:
    op2.process(morsel)        -- morsel must be re-read

  // Good: fuse operators, process each morsel completely
  for morsel in all_morsels:
    result = op1.process(morsel)  -- morsel in L2
    op2.process(result)           -- result still in L1/L2
```

## Implementation

```rust
/// Morsel size calculator based on cache hierarchy
pub fn optimal_morsel_size(
    row_width_bytes: usize,
    l2_cache_bytes: usize,
    num_pipeline_operators: usize,
) -> usize {
    // Reserve space for operator state and intermediate results
    let pipeline_overhead_factor = 1 + num_pipeline_operators;
    let effective_cache = l2_cache_bytes / pipeline_overhead_factor;

    // Each morsel should fit in effective cache
    let rows = effective_cache / row_width_bytes;

    // Clamp to reasonable bounds
    rows.max(64).min(65536)
}

/// Software prefetch for hash table probing
pub fn prefetch_hash_probe(
    morsel: &Morsel,
    hash_table: &HashTable,
    prefetch_distance: usize,
) -> Vec<ProbeResult> {
    let mut results = Vec::with_capacity(morsel.len());
    let keys: Vec<u64> = morsel.rows()
        .map(|r| r.hash_key())
        .collect();

    // Issue prefetch instructions ahead of actual probes
    for i in 0..morsel.len() {
        // Prefetch future hash table bucket
        if i + prefetch_distance < morsel.len() {
            let future_bucket = hash_table.bucket_addr(
                keys[i + prefetch_distance],
            );
            unsafe {
                std::arch::x86_64::_mm_prefetch(
                    future_bucket as *const i8,
                    std::arch::x86_64::_MM_HINT_T0,
                );
            }
        }

        // Probe current key (data should be in cache from earlier prefetch)
        let matches = hash_table.probe(keys[i]);
        results.extend(matches);
    }

    results
}

/// Fused pipeline execution for cache locality
pub struct FusedPipeline {
    operators: Vec<Box<dyn Operator>>,
    morsel_size: usize,
}

impl FusedPipeline {
    pub fn execute_morsel(&self, morsel: Morsel) -> Vec<Row> {
        let mut current = morsel.into_rows();

        // Process through all operators while data is cache-hot
        for op in &self.operators {
            current = op.process_batch(current);
            if current.is_empty() {
                break; // Early termination if morsel fully filtered
            }
        }

        current
    }
}

/// Cost model incorporating cache effects
pub fn cache_aware_cost(
    total_rows: f64,
    row_bytes: f64,
    morsel_size: usize,
    l2_size: usize,
    memory_latency_ns: f64,
    l2_latency_ns: f64,
) -> f64 {
    let morsel_bytes = morsel_size as f64 * row_bytes;
    let fits_in_l2 = morsel_bytes <= l2_size as f64;

    let per_row_cost = if fits_in_l2 {
        l2_latency_ns  // ~5 ns per access from L2
    } else {
        memory_latency_ns  // ~100 ns per access from DRAM
    };

    total_rows * per_row_cost * 0.000001
}
```

## Cost Model

**Cache hit rates by morsel size:**
- Morsel fits in L1 (32KB): ~95% L1 hit rate, ~2ns per access
- Morsel fits in L2 (256KB): ~90% L2 hit rate, ~5ns per access
- Morsel exceeds L2: ~60% L3 hit rate, ~15ns per access
- Morsel exceeds L3: DRAM accesses, ~100ns per access

**Prefetch benefits:**
- Hash table probe without prefetch: ~100ns (cache miss per probe)
- With software prefetch: ~15ns (overlap miss with computation)
- Optimal prefetch distance: 8-16 entries ahead

**Pipeline fusion benefit:**
- Unfused: data loaded from DRAM once per operator
- Fused: data loaded once, stays in cache through all operators
- For 5-operator pipeline: ~5x reduction in memory bandwidth

## Test Cases

```sql
-- Test 1: Cache-friendly morsel scan
SELECT * FROM lineitem WHERE l_quantity < 25;
-- Row width: ~150 bytes, L2: 256KB
-- Optimal morsel: ~1700 rows (fits in L2 with overhead)
-- Each morsel scanned without cache misses

-- Test 2: Hash join with prefetching
SELECT * FROM orders o JOIN customers c ON o.cid = c.id;
-- Probe side processes morsel of orders
-- Software prefetch issued 8 probes ahead
-- Reduces hash table cache misses by ~80%

-- Test 3: Multi-operator fusion
SELECT region, SUM(amount) FROM orders
WHERE date > '2024-01-01' GROUP BY region;
-- Fused pipeline: Scan -> Filter -> Aggregate
-- Each morsel processed through all 3 operators
-- Data stays in L2 throughout pipeline
```

## References

1. **Leis, Viktor et al**. "Morsel-Driven Parallelism." SIGMOD 2014.
   - Morsel sizing and cache-conscious execution

2. **Boncz, Peter et al**. "MonetDB/X100: Hyper-Pipelining Query Execution."
   CIDR 2005.
   - Cache-conscious vectorized execution

3. **Chen, Shimin et al**. "Improving Hash Join Performance through
   Prefetching." ACM TODS 2007.
   - Software prefetching for hash joins
