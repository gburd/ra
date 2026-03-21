# Rule: Software Prefetch-Aware Hash Join

**Category:** hardware/accelerator
**File:** `rules/hardware/accelerator/prefetch-aware-join.rra`

## Metadata

- **ID:** `prefetch-aware-join`
- **Version:** "1.0.0"
- **Databases:** duckdb, umbra, hyper, clickhouse
- **Tags:** prefetch, hash-join, cache, memory-hierarchy, cpu
- **Authors:** "RA Contributors"


# Software Prefetch-Aware Hash Join

## Description

Restructures the hash join probe phase to issue software prefetch
instructions ahead of hash table lookups. By computing hash values
for a batch of probe tuples, issuing prefetches for all of them,
then performing the lookups, the CPU overlaps memory access latency
with computation. This hides L3 and DRAM latency for hash tables
that exceed the L2 cache.

**When to apply**: The hash table exceeds the L2 cache and the probe
side is large. The hash table must use open addressing (not chaining)
for prefetch effectiveness.

**Why it works**: A standard hash probe has a data-dependent cache
miss: compute hash, access bucket, stall on miss. By batching,
multiple prefetches are in flight simultaneously, utilizing the
CPU's memory-level parallelism (typically 10-20 outstanding loads).
This converts sequential cache misses into parallel ones.

## Relational Algebra

```algebra
R hash_join[c] S -> prefetch_hash_join[c](R, S)
  where size(hash_table(build_side)) > L2_cache
    AND |probe_side| > 1000
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("prefetch-aware-join";
    "(hash_join ?cond ?build ?probe)" =>
    "(prefetch_hash_join ?cond ?build ?probe)"
    if hash_table_exceeds_l2("?build")
    if probe_side_large("?probe")
),
```

## Preconditions

```rust
fn applicable(
    build_stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    let hash_table_bytes =
        build_stats.row_count as u64
            * build_stats.avg_row_size
            * 2;
    hash_table_bytes > hw.l2_cache_bytes
}
```

**Restrictions:**
- Open-addressing hash tables benefit most (predictable memory access)
- Chained hash tables have pointer-chasing that limits prefetch benefit
- Batch size should match memory-level parallelism (10-20 typically)
- Only effective when hash table exceeds L2 cache

## Cost Model

```rust
fn estimated_benefit(
    build_stats: &Statistics,
    probe_stats: &Statistics,
    hw: &HardwareProfile,
) -> f64 {
    let hash_table_bytes = build_stats.row_count as u64
        * build_stats.avg_row_size
        * 2;

    if hash_table_bytes <= hw.l2_cache_bytes {
        return 0.0;
    }

    let miss_latency_ns = if hash_table_bytes > hw.l3_cache_bytes
    {
        hw.dram_latency_ns // ~80-100ns
    } else {
        hw.l3_latency_ns // ~30-40ns
    };

    let probe_rows = probe_stats.row_count;
    // Without prefetch: sequential misses
    let no_prefetch_ns =
        probe_rows * (miss_latency_ns + 10.0);
    // With prefetch: overlap misses in batches of MLP
    let mlp = hw.memory_level_parallelism as f64;
    let prefetch_ns =
        probe_rows * (miss_latency_ns / mlp + 15.0);

    if no_prefetch_ns > prefetch_ns {
        (no_prefetch_ns - prefetch_ns) / no_prefetch_ns
    } else {
        0.0
    }
}
```

**Typical benefit**: 20-50% for hash tables in L3, 30-55% for hash
tables in DRAM. Widely used in modern analytical databases.

## Test Cases

### Positive: Large hash table in DRAM

```sql
-- customer: 15M rows, hash table ~1.8 GB (exceeds L3)
-- lineitem: 600M probe rows
SELECT l.*, c.c_name
FROM lineitem l
JOIN customer c ON l.l_custkey = c.c_custkey;

-- Expected: Prefetch-aware hash join
-- Plan: PrefetchHashJoin(build=customer, probe=lineitem,
--        batch_size=16)
```

### Negative: Small hash table fits in L2

```sql
-- region: 5 rows, hash table ~200 bytes
SELECT * FROM nation n
JOIN region r ON n.n_regionkey = r.r_regionkey;

-- Expected: Standard hash join (no prefetch needed)
-- Plan: HashJoin(build=region, probe=nation)
```

## References

**Implementation in databases:**
- DuckDB: Prefetch-based hash join in `src/execution/join_hashtable.cpp`
- Umbra/HyPer: Software-pipelined hash joins
- ClickHouse: Prefetch hints in hash table probing

**Academic papers:**
- Chen et al., "Improving Hash Join Performance through Prefetching", ICDE 2004
- Kocberber et al., "Meet the Walkers: Accelerating Index Traversals for In-Memory Databases", MICRO 2013
- Schuh et al., "An Experimental Comparison of Thirteen Relational Equi-Joins in Main Memory", SIGMOD 2016
