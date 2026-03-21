# Rule: "Memory and Cache Hierarchy Cost Model"

**Category:** cost-models
**File:** `rules/cost-models/memory-cost-model.rra`

## Metadata

- **ID:** `memory-cost-model`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, singlestore, clickhouse, cockroachdb
- **Tags:** cost, memory, cache, bandwidth, numa, allocation
- **Authors:** "RA Contributors"


# Memory and Cache Hierarchy Cost Model

## Description

Models memory-related costs across the cache hierarchy: L1, L2, L3 caches,
DRAM, and NUMA effects. For in-memory and columnar databases, memory access
patterns dominate execution time. The model captures allocation overhead,
bandwidth saturation, cache miss penalties, and NUMA locality effects.

Modern CPUs spend 50-80% of query execution time waiting for memory. The
cache hierarchy creates a 100x performance gap between L1 hits (1ns) and
DRAM accesses (100ns). Operators that fit in cache run orders of magnitude
faster than those that spill.

**When to apply**: Any operator that materializes intermediate results
(hash join build, sort, aggregation) or processes data exceeding cache size.

**Why it works**: Memory access latency depends on which cache level serves
the request. By modeling working set size against cache capacity, the
optimizer can predict when operators will hit the "memory wall" and prefer
cache-friendly alternatives (e.g., sort-merge join over hash join when
build side exceeds L3).

## Relational Algebra

```algebra
Cost_Memory(Op) = f(working_set, cache_hierarchy, access_pattern)

Cache model:
  If working_set <= L1_size: latency = 1ns
  If working_set <= L2_size: latency = 5ns
  If working_set <= L3_size: latency = 20ns
  If working_set > L3_size:  latency = 100ns (DRAM)

Bandwidth model:
  Throughput = min(compute_rate, bandwidth / bytes_per_access)
```

## Implementation

```rust
use ra_hardware::HardwareProfile;

struct CacheLevel {
    size_bytes: u64,
    latency_ns: f64,
    bandwidth_gb_s: f64,
    line_size: u32,
}

struct MemoryCostModel {
    l1: CacheLevel,
    l2: CacheLevel,
    l3: CacheLevel,
    dram_latency_ns: f64,
    dram_bandwidth_gb_s: f64,
    numa_remote_penalty: f64,
    page_fault_us: f64,
}

impl MemoryCostModel {
    fn for_modern_server() -> Self {
        Self {
            l1: CacheLevel {
                size_bytes: 32 * 1024,
                latency_ns: 1.0,
                bandwidth_gb_s: 500.0,
                line_size: 64,
            },
            l2: CacheLevel {
                size_bytes: 256 * 1024,
                latency_ns: 5.0,
                bandwidth_gb_s: 200.0,
                line_size: 64,
            },
            l3: CacheLevel {
                size_bytes: 32 * 1024 * 1024,
                latency_ns: 20.0,
                bandwidth_gb_s: 100.0,
                line_size: 64,
            },
            dram_latency_ns: 100.0,
            dram_bandwidth_gb_s: 50.0,
            numa_remote_penalty: 1.7,
            page_fault_us: 10.0,
        }
    }

    fn access_latency(&self, working_set_bytes: u64) -> f64 {
        if working_set_bytes <= self.l1.size_bytes {
            self.l1.latency_ns
        } else if working_set_bytes <= self.l2.size_bytes {
            // Mix of L1 hits and L2 hits
            let l1_hit_rate =
                self.l1.size_bytes as f64 / working_set_bytes as f64;
            l1_hit_rate * self.l1.latency_ns
                + (1.0 - l1_hit_rate) * self.l2.latency_ns
        } else if working_set_bytes <= self.l3.size_bytes {
            let l2_hit_rate =
                self.l2.size_bytes as f64 / working_set_bytes as f64;
            l2_hit_rate * self.l2.latency_ns
                + (1.0 - l2_hit_rate) * self.l3.latency_ns
        } else {
            let l3_hit_rate =
                self.l3.size_bytes as f64 / working_set_bytes as f64;
            l3_hit_rate * self.l3.latency_ns
                + (1.0 - l3_hit_rate) * self.dram_latency_ns
        }
    }

    fn effective_bandwidth(
        &self,
        working_set_bytes: u64,
    ) -> f64 {
        if working_set_bytes <= self.l1.size_bytes {
            self.l1.bandwidth_gb_s
        } else if working_set_bytes <= self.l2.size_bytes {
            self.l2.bandwidth_gb_s
        } else if working_set_bytes <= self.l3.size_bytes {
            self.l3.bandwidth_gb_s
        } else {
            self.dram_bandwidth_gb_s
        }
    }

    fn hash_table_cost(
        &self,
        build_rows: f64,
        key_size: u32,
        payload_size: u32,
    ) -> f64 {
        // Hash table: 2x space for load factor 0.5
        let entry_size = (key_size + payload_size + 8) as f64;
        let ht_bytes = (build_rows * entry_size * 2.0) as u64;
        let avg_latency = self.access_latency(ht_bytes);

        // Build: sequential insert
        let build_cost = build_rows * avg_latency;

        // Probe: random access into hash table
        // Random access is worse than sequential by ~2x within
        // the same cache level due to prefetch failure
        let probe_latency = avg_latency * 2.0;

        (build_cost, probe_latency)
    }

    fn sort_memory_cost(
        &self,
        rows: f64,
        row_size: u32,
    ) -> f64 {
        let working_set = (rows * row_size as f64) as u64;
        let avg_latency = self.access_latency(working_set);
        let comparisons = rows * rows.log2().max(1.0);

        // Each comparison accesses two cache lines (random)
        comparisons * avg_latency * 1.5
    }

    fn materialization_cost(
        &self,
        bytes: f64,
        is_numa_remote: bool,
    ) -> f64 {
        let bandwidth = self.effective_bandwidth(bytes as u64);
        let base_time_s = bytes / (bandwidth * 1e9);
        let base_time_ns = base_time_s * 1e9;

        if is_numa_remote {
            base_time_ns * self.numa_remote_penalty
        } else {
            base_time_ns
        }
    }

    fn columnar_scan_cost(
        &self,
        rows: f64,
        column_width: u32,
        num_columns: u32,
    ) -> f64 {
        // Columnar: each column is contiguous, sequential access
        let total_bytes = rows * column_width as f64;
        let per_column_cost = total_bytes
            / (self.effective_bandwidth(total_bytes as u64) * 1e9)
            * 1e9;

        per_column_cost * num_columns as f64
    }
}
```

**Restrictions:**
- Cache behavior is probabilistic; model uses averages
- Assumes single-threaded access (multi-threaded contention not modeled)
- Hardware prefetcher effectiveness varies by access pattern
- TLB misses for large working sets not explicitly modeled
- Compiler optimizations may change access patterns

## Cost Model

```rust
fn estimated_benefit(
    operator: &Operator,
    memory_model: &MemoryCostModel,
    flat_model: &FlatCostModel,
) -> f64 {
    let working_set = operator.estimated_working_set();
    let memory_aware_cost = memory_model.access_latency(working_set)
        * operator.num_accesses();
    let flat_cost = flat_model.fixed_latency
        * operator.num_accesses();

    if flat_cost > memory_aware_cost {
        (flat_cost - memory_aware_cost) / flat_cost
    } else {
        // Memory model may show data exceeds cache
        (memory_aware_cost - flat_cost) / memory_aware_cost
    }
}
```

**Assumptions:**
- Hardware prefetcher active for sequential patterns
- No other processes competing for cache space
- NUMA topology known at optimization time
- Working set size is the dominant predictor of latency

**Typical benefit**: 20-60% improvement for queries where operator choice
depends on whether intermediate data fits in cache. Hash join vs sort-merge
crossover depends heavily on L3 cache size.

## Test Cases

### Test 1: Hash join build fits in L3

```sql
SELECT * FROM small_dim d JOIN large_fact f ON d.id = f.dim_id;
-- small_dim: 10K rows, 200 bytes/row = 2MB working set
-- L3 cache: 32MB

-- Hash table: 2MB * 2 (load factor) = 4MB -> fits in L3
-- Build latency: ~20ns per access (L3 speed)
-- Build cost: 10K * 20ns = 200us
-- Probe latency: ~40ns per random access (L3 + random penalty)
-- Fast path: hash join is optimal
```

### Test 2: Hash join build exceeds L3

```sql
SELECT * FROM big_dim d JOIN large_fact f ON d.id = f.dim_id;
-- big_dim: 5M rows, 200 bytes/row = 1GB working set
-- L3 cache: 32MB -> massive overflow

-- Hash table: 1GB * 2 = 2GB -> DRAM resident
-- Build latency: ~100ns per access (DRAM)
-- Build cost: 5M * 100ns = 500ms
-- Probe latency: ~200ns per random access (DRAM + random)
-- Sort-merge join may be cheaper (sequential access pattern)
```

### Test 3: Columnar vs row scan

```sql
SELECT SUM(price) FROM lineitem;
-- 60M rows, price column: 8 bytes, row: 200 bytes

-- Row scan: 60M * 200 bytes = 12GB / 50GB/s = 240ms
-- Columnar: 60M * 8 bytes = 480MB / 50GB/s = 9.6ms
-- Columnar is 25x faster (touches 25x less memory)
```

### Test 4: NUMA penalty for large aggregation

```sql
SELECT region, SUM(amount) FROM sales GROUP BY region;
-- 100M rows, hash table on remote NUMA node

-- Local NUMA: 100M * 100ns = 10s
-- Remote NUMA: 100M * 170ns = 17s (1.7x penalty)
-- NUMA-aware placement saves 7s
```

### Test 5: Sort in L3 vs spilling to DRAM

```sql
SELECT * FROM orders ORDER BY total;
-- Case A: 100K rows * 100 bytes = 10MB (fits L3)
--   Sort: 100K * log2(100K) * 20ns = 33ms
-- Case B: 10M rows * 100 bytes = 1GB (exceeds L3)
--   Sort: 10M * log2(10M) * 100ns = 23s
--   5x latency penalty cascades through O(n log n) comparisons
```

## References

**Cache hierarchy modeling:**
- Manegold, Boncz, Kersten, "Optimizing Database Architecture for the New Bottleneck: Memory Access", VLDB J. 2000
- Ailamaki et al., "DBMSs on a Modern Processor: Where Does Time Go?", VLDB 1999

**NUMA-aware query processing:**
- Leis et al., "Morsel-Driven Parallelism", SIGMOD 2014
- Li et al., "NUMA-Aware Algorithms: The Case of Data Shuffling", CIDR 2013

**Modern implementations:**
- DuckDB: vectorized execution with cache-conscious batch sizes
- MonetDB: column-at-a-time processing for bandwidth utilization
- PostgreSQL: `effective_cache_size` parameter for plan selection
