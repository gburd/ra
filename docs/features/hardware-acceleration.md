# Hardware-Accelerated Query Optimization

This document describes the hardware-specific optimization rules and
cost models in the RA system, covering GPU, FPGA, SIMD, and NUMA
acceleration strategies.

## Overview

Modern database systems can accelerate query execution using
specialized hardware beyond the CPU. The `ra-hardware` crate and
`rules/hardware/` directory provide:

- **21 optimization rules** targeting GPU, FPGA, CPU SIMD, and data
  placement strategies
- **Hardware cost model** (`HardwareCostModel`) that estimates
  execution cost across CPU, GPU, and FPGA including data transfer
  overhead
- **Hardware profiles** (`HardwareProfile`) describing system
  capabilities for cost-based operator placement

## Architecture

```
                    Query Plan
                        |
                        v
            ,-------------------------,
            |   Hardware Cost Model  |
            |  (ra-hardware crate)   |
            `----------+---------+---------'
                    |       |
          ,-----------'       `------------,
          v                           v
   ,---------------,           ,----------------,
   |  CPU Cost    |           | Device Cost   |
   |  (baseline)  |           | + Transfer    |
   `---------+--------'           `----------+--------'
          |                          |
          v                          v
   ,-------------------------------------------,
   |         Operator Placement Decision      |
   |  (choose cheapest: CPU / GPU / FPGA)     |
   `--------------------------------------------'
```

## When Hardware Acceleration Helps

Hardware acceleration is not always beneficial. The optimizer must
balance compute speedup against data transfer overhead.

### Decision Framework

1. **Data volume**: Is the data large enough to amortize transfer
   overhead? GPU acceleration typically needs 100K+ rows.

2. **Operation type**: Is the operation data-parallel? Scans, filters,
   aggregations, and joins are good candidates. Complex sequential
   logic is not.

3. **Data locality**: Is the data already on the device? Cached GPU
   data eliminates transfer cost entirely.

4. **Transfer vs compute**: Does `transfer_time + device_compute <
   cpu_compute`? For bandwidth-bound scans, PCIe (25 GB/s) is slower
   than CPU memory (50 GB/s), so GPU only helps for compute-heavy
   operations.

### Hardware Comparison

| Metric | CPU | GPU (A100) | FPGA (Alveo) |
|--------|-----|------------|--------------|
| Memory BW | ~50 GB/s | ~2 TB/s | ~20-80 GB/s |
| Parallelism | 64 cores | 108 SMs (6912 cores) | Custom |
| Transfer | None | PCIe 25 GB/s | PCIe 15 GB/s |
| Latency | ~1-5 ns | ~5-10 ns (amortized) | ~3-5 ns |
| Best for | Complex logic | Data-parallel compute | Streaming |
| Power | ~10-50W/core | ~300W total | ~25-75W |

## GPU Acceleration

GPU acceleration targets operations where massive parallelism
overcomes PCIe transfer overhead.

### GPU Rules

**gpu-parallel-scan**: Offloads table scans when GPU memory bandwidth
(900+ GB/s) dramatically outpaces CPU memory bandwidth. Only
beneficial when data is already GPU-resident or when combined with
filtering that reduces result volume.

**gpu-hash-join**: Builds hash tables in parallel using atomic
insertions, then probes with thousands of concurrent threads. The
build side must fit in GPU memory. Achieves 3-10x speedup for large
joins with asymmetric input sizes.

**gpu-aggregation**: Two-phase GROUP BY: block-local reduction in
shared memory, then global merge. Excels at both low-cardinality
aggregations (parallel reduction) and moderate-cardinality (parallel
hash aggregation).

**gpu-sort**: Uses parallel radix sort for integer/date keys (4
passes of 8 bits each) or parallel merge sort for comparisons.
Achieves 2-10x throughput for >1M rows with integer keys.

**gpu-predicate-evaluation**: Evaluates compound WHERE clauses
(AND/OR/BETWEEN/IN/LIKE) using SIMT parallelism. Each thread
evaluates the predicate for one tuple. Benefits scale with predicate
complexity.

**gpu-string-operations**: Parallel LIKE/REGEXP matching where each
thread processes one string. Dictionary-encoded columns can match
against the dictionary only, achieving 50x+ speedup.

**gpu-window-function**: Parallel prefix sum for running aggregates
(SUM, AVG) and GPU sort + position assignment for ranking functions
(ROW_NUMBER, RANK). Works within each partition independently.

**gpu-distinct-aggregation**: Two-phase approach: GPU hash set for
deduplication within each group, then aggregate on deduplicated
values. Handles COUNT(DISTINCT), SUM(DISTINCT).

### GPU Systems Referenced

- **HeavyDB (OmniSci)**: Full GPU SQL database with LLVM-based query
  compilation to GPU kernels
- **PG-Strom**: PostgreSQL extension that offloads operations to GPU
  via custom scan/join/aggregate operators
- **BlazingSQL**: GPU SQL engine built on NVIDIA RAPIDS/cuDF
- **SQream**: GPU-native analytical database for large-scale data

## FPGA Acceleration

FPGA acceleration excels at streaming operations with deterministic
throughput and near-storage processing.

### FPGA Rules

**fpga-stream-filter**: Synthesizes predicates into hardware logic
that filters at one tuple per clock cycle (200-300 MHz). Ideal for
simple comparisons, range checks, and bitmask operations on streaming
data.

**fpga-compression-scan**: Places the FPGA between storage and CPU to
decompress (LZ4, RLE, delta, dictionary) and filter at the storage
interface. Only qualifying rows cross the bus, reducing data volume
by compression_ratio * selectivity.

**fpga-hash-join**: Loads small build-side hash table into FPGA BRAM
(~40 MB on Alveo U280), then streams probe tuples through at one per
clock cycle. No cache misses since BRAM has single-cycle latency.

**fpga-regex-filter**: Compiles regex patterns into hardware NFAs
where each NFA state is a flip-flop. Processes one character per
clock cycle per NFA instance, with multiple instances in parallel.

### FPGA Systems Referenced

- **IBM Netezza**: FPGA-based zone map filtering and data pruning
- **Xilinx Alveo**: Database acceleration cards with reference designs
- **Samsung SmartSSD**: Near-storage FPGA processing
- **Intel PAC**: Programmable Acceleration Cards

## CPU-Level Optimizations

These rules exploit CPU hardware features without requiring external
accelerators.

### CPU Accelerator Rules

**simd-vectorized-scan**: Replaces scalar comparisons with SIMD
vector operations (AVX-512: 16 int32s per instruction, AVX2: 8).
Requires columnar storage and SIMD-friendly data types.

**numa-aware-partitioning**: Hash-partitions data across NUMA nodes
so each core accesses local memory. On dual-socket systems, avoids
the 1.5-3x remote memory access penalty.

**prefetch-aware-join**: Batches hash table probes and issues software
prefetch instructions to overlap memory latency with computation.
Achieves 20-55% improvement when hash table exceeds L2 cache.

**cache-conscious-partitioning**: Radix-partitions both sides of a
hash join so each partition's hash table fits in L2 cache. Eliminates
cache misses during the probe phase.

## Data Placement Optimization

These rules minimize data movement between host and device memory.

### Data Placement Rules

**host-to-device-transfer**: Pushes filters and projections before
device transfer to reduce PCIe data volume. Eliminates unnecessary
round-trips between host and device.

**device-memory-caching**: Keeps frequently accessed tables resident
in GPU memory across queries. For N queries on the same table,
eliminates (N-1)/N of the transfer cost.

**columnar-conversion**: Converts row-oriented data to columnar
layout before GPU transfer. Columnar layout enables coalesced memory
access on GPU (all threads in a warp access consecutive addresses).

**unified-memory-management**: Uses hardware-managed page migration
(CUDA Unified Memory) for datasets exceeding GPU memory. The hardware
prefetcher handles sequential access patterns transparently.

## Using the Hardware Cost Model

### Hardware Profiles

The `HardwareProfile` struct describes available hardware:

```rust
use ra_hardware::HardwareProfile;

// Pre-defined profiles
let gpu_server = HardwareProfile::gpu_server();    // A100 80GB
let fpga = HardwareProfile::fpga_appliance();      // Alveo U280
let cpu = HardwareProfile::cpu_only();             // 2x Xeon
```

### Cost Estimation

The `HardwareCostModel` estimates cost on each device:

```rust
use ra_hardware::{HardwareCostModel, Device};

let model = HardwareCostModel::new(HardwareProfile::gpu_server());

// Compare scan cost on CPU vs GPU
let cpu_cost = model.scan_cost(100_000_000.0, 100, Device::Cpu);
let gpu_cost = model.scan_cost(100_000_000.0, 100, Device::Gpu);

// For pure scans, CPU wins (PCIe < DDR bandwidth)
// GPU wins for compute-heavy operations:
let cpu_join = model.hash_join_cost(
    1_000_000.0, 100_000_000.0, 100, Device::Cpu,
);
let gpu_join = model.hash_join_cost(
    1_000_000.0, 100_000_000.0, 100, Device::Gpu,
);
assert!(gpu_join.total() < cpu_join.total());
```

### Implementing the CostModel Trait

`HardwareCostModel` implements `ra_core::CostModel`:

```rust
use ra_core::{CostModel, RelExpr, StatisticsProvider};

let model = HardwareCostModel::new(HardwareProfile::gpu_server());
let cost = model.estimate(&expr, &stats_provider);
```

## Adding New Hardware Rules

Use the template at `rules/templates/template-hardware.rra`:

1. Place the rule in the appropriate subdirectory (`gpu/`, `fpga/`,
   `accelerator/`, or `data-placement/`)
2. Include `hardware` field in YAML frontmatter
3. Document the crossover point where hardware wins vs CPU
4. Provide a cost model with concrete hardware parameters
5. Include both positive (hardware wins) and negative (CPU wins) test
   cases
6. Reference real database implementations and academic papers
7. Add the rule ID to `rules/index.toml`

## References

### Academic Papers

- He et al., "Relational Joins on Graphics Processors", SIGMOD 2008
- Bress et al., "GPU-Accelerated Database Systems: Survey and Open
  Challenges", TODS 2014
- Mueller et al., "Streams on Wires - A Query Compiler for FPGAs",
  VLDB 2009
- Leis et al., "Morsel-Driven Parallelism: A NUMA-Aware Query
  Evaluation Framework for the Many-Core Age", SIGMOD 2014
- Polychroniou et al., "Rethinking SIMD Vectorization for In-Memory
  Databases", SIGMOD 2015
- Karnagel et al., "Adaptive Work Placement for Query Processing on
  Heterogeneous Computing Resources", VLDB 2017

### Database Implementations

- HeavyDB source: `QueryEngine/` directory
- PG-Strom source: `src/gpu*.c` files
- DuckDB SIMD: `src/common/vector_operations/`
- ClickHouse SIMD: `src/Functions/FunctionsComparison.h`
