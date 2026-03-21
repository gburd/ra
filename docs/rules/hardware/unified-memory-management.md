# Rule: Unified Memory Management for CPU-GPU

**Category:** hardware/data-placement
**File:** `rules/hardware/data-placement/unified-memory-management.rra`

## Metadata

- **ID:** `unified-memory-management`
- **Version:** "1.0.0"
- **Databases:** heavydb, blazingsql
- **Tags:** unified-memory, cuda-um, migration, page-fault, gpu
- **Authors:** "RA Contributors"


# Unified Memory Management for CPU-GPU

## Description

Uses hardware-managed unified memory (CUDA Unified Memory, AMD HSA)
to automatically migrate pages between CPU and GPU memory on demand.
Instead of explicit data transfers, the optimizer relies on the
hardware page migration engine to move data to whichever processor
accesses it. This simplifies planning for queries that exceed GPU
memory.

**When to apply**: The dataset exceeds GPU memory but the working set
(actively accessed pages) fits. Unified memory handles oversubscription
transparently via page faults and migration. Best for queries with
good data locality where the working set is a fraction of total data.

**Why it works**: NVIDIA's page migration engine (since Pascal/Volta)
can migrate 4KB-2MB pages between CPU and GPU at ~12 GB/s. For
queries with good locality (sequential scans, partitioned access),
most pages are accessed once and migrated on first touch. The
hardware prefetcher can anticipate sequential access patterns,
reducing page fault overhead.

## Relational Algebra

```algebra
gpu_op(explicit_transfer(R))
  -> gpu_op(unified_memory(R))
  where size(R) > gpu_memory
    AND working_set(R) <= gpu_memory
    AND access_pattern is sequential_or_partitioned
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("unified-memory-oversubscription";
    "(gpu_op (chunked_transfer ?input))" =>
    "(gpu_op (unified_memory ?input))"
    if dataset_exceeds_gpu_memory("?input")
    if working_set_fits("?input")
    if access_pattern_is_local("?input")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    let data_bytes =
        stats.row_count as u64 * stats.avg_row_size;

    data_bytes > hw.gpu_memory_bytes
        && hw.unified_memory_supported
        && hw.page_migration_engine_available
}
```

**Restrictions:**
- Page fault overhead: ~20us per fault (amortized over 4KB-2MB pages)
- Random access patterns cause excessive page thrashing
- Pre-Pascal NVIDIA GPUs do not support page migration
- AMD ROCm and Intel oneAPI have different UM implementations
- Concurrent CPU and GPU access to same pages causes ping-pong

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    working_set_fraction: f64,
    hw: &HardwareProfile,
) -> f64 {
    let data_bytes =
        stats.row_count as u64 * stats.avg_row_size;

    // Explicit chunked transfer: multiple round-trips
    let chunks = (data_bytes / hw.gpu_memory_bytes) + 1;
    let explicit_ns = chunks as f64
        * hw.gpu_memory_bytes as f64
        / (hw.pcie_bandwidth_gbps * 1e9)
        * 1e9;

    // Unified memory: migrate working set on demand
    let working_set_bytes =
        (data_bytes as f64 * working_set_fraction) as u64;
    let page_faults =
        working_set_bytes / hw.um_page_size_bytes;
    let fault_overhead_ns =
        page_faults as f64 * hw.um_fault_latency_us * 1e3;
    let migration_ns = working_set_bytes as f64
        / (hw.um_migration_bandwidth_gbps * 1e9)
        * 1e9;
    let um_total = fault_overhead_ns + migration_ns;

    if explicit_ns > um_total {
        (explicit_ns - um_total) / explicit_ns
    } else {
        0.0
    }
}
```

**Typical benefit**: 10-40% for sequential scan workloads that exceed
GPU memory, compared to explicit chunked transfers. Primary benefit
is simplifying the query plan.

## Test Cases

### Positive: Query exceeds GPU memory with good locality

```sql
-- lineitem: 72 GB, GPU: 16 GB
-- Sequential scan + filter: good locality
SELECT l_returnflag, SUM(l_quantity)
FROM lineitem
WHERE l_shipdate > '1998-01-01'
GROUP BY l_returnflag;

-- Expected: Unified memory with automatic page migration
-- Plan: GpuAggregate(GpuFilter(unified_memory(lineitem)))
```

### Negative: Random access pattern

```sql
-- Hash join probe with random access into large hash table
-- Page thrashing makes UM slower than explicit management
SELECT * FROM lineitem l
JOIN large_dim d ON l.l_partkey = d.d_partkey;

-- Expected: Explicit chunked GPU hash join
-- Plan: ChunkedGpuHashJoin(build_chunks=large_dim,
--        probe=lineitem)
```

## References

**Implementation in databases:**
- HeavyDB: CUDA Unified Memory for oversubscription
- NVIDIA RAPIDS: cuDF with managed memory
- BlazingSQL: RMM (RAPIDS Memory Manager) with UM support

**Academic papers:**
- Zheng et al., "GAIA: A System for Interactive Analysis on Distributed Graphs Using a High-Level Language", OSDI 2014 (early UM usage)
- Ganguly et al., "Oversubscription of GPU Memory using CUDA Unified Memory", SC 2019
- Li et al., "Evaluating GPU Unified Memory", HPCA 2023
