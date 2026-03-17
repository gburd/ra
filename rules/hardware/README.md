# Hardware-Specific Optimization Rules

Rules in this directory target hardware-accelerated query execution,
including GPUs, FPGAs, SIMD units, and memory hierarchy optimizations.

## Directory Structure

- **gpu/** - GPU (CUDA/OpenCL) acceleration rules
- **fpga/** - FPGA streaming and near-storage rules
- **accelerator/** - CPU-level hardware optimizations (SIMD, NUMA, cache)
- **data-placement/** - Host-device data transfer optimization

## GPU Rules

| Rule | Description | Databases |
|------|-------------|-----------|
| `gpu-parallel-scan` | Offload scans to GPU for bandwidth-bound workloads | HeavyDB, PG-Strom |
| `gpu-hash-join` | GPU-accelerated hash join with parallel build/probe | HeavyDB, BlazingSQL |
| `gpu-aggregation` | Parallel GROUP BY using shared memory reduction | HeavyDB, SQream |
| `gpu-sort` | GPU radix sort for integer/date keys | HeavyDB, BlazingSQL |
| `gpu-predicate-evaluation` | SIMT predicate evaluation for compound filters | HeavyDB, PG-Strom |
| `gpu-string-operations` | Parallel string matching (LIKE/REGEXP) | HeavyDB, SQream |
| `gpu-window-function` | Parallel prefix for window aggregates | HeavyDB, SQream |
| `gpu-distinct-aggregation` | Two-phase GPU DISTINCT aggregation | HeavyDB, SQream |

## FPGA Rules

| Rule | Description | Databases |
|------|-------------|-----------|
| `fpga-stream-filter` | Line-rate predicate evaluation in hardware | Netezza, Alveo |
| `fpga-compression-scan` | Near-storage decompression and filtering | Netezza, SmartSSD |
| `fpga-hash-join` | Pipelined join with BRAM hash table | Alveo, Intel PAC |
| `fpga-regex-filter` | Hardware NFA for regex pattern matching | Alveo, Intel PAC |

## Accelerator Rules

| Rule | Description | Databases |
|------|-------------|-----------|
| `heterogeneous-operator-placement` | Assign operators to CPU/GPU/FPGA | HeavyDB, PG-Strom |
| `simd-vectorized-scan` | AVX-512/NEON vectorized scan and filter | DuckDB, ClickHouse |
| `numa-aware-partitioning` | NUMA-local data placement on multi-socket | HyPer, SAP HANA |
| `prefetch-aware-join` | Software prefetch for hash join probes | DuckDB, Umbra |
| `cache-conscious-partitioning` | Radix partitioning for cache-local joins | DuckDB, MonetDB |

## Data Placement Rules

| Rule | Description | Databases |
|------|-------------|-----------|
| `host-to-device-transfer` | Minimize PCIe data movement | HeavyDB, PG-Strom |
| `device-memory-caching` | Cache hot data on GPU/FPGA across queries | HeavyDB, SQream |
| `columnar-conversion` | Row-to-columnar conversion for device use | PG-Strom, HeavyDB |
| `unified-memory-management` | Transparent CPU-GPU page migration | HeavyDB, RAPIDS |

## Decision Framework

When should hardware acceleration be used?

```
1. Is the data large enough to amortize transfer overhead?
   NO  -> CPU execution
   YES -> Continue

2. Is the operation data-parallel (same op on many tuples)?
   NO  -> CPU execution (complex logic stays on CPU)
   YES -> Continue

3. Is the data already on the device (cached)?
   YES -> Use device operator (no transfer cost)
   NO  -> Estimate: transfer_time + device_compute < cpu_compute?
          YES -> Use device operator
          NO  -> CPU execution

4. Which device?
   - GPU: High-throughput parallel compute (scans, joins, aggs)
   - FPGA: Streaming filters, near-storage processing, regex
   - CPU SIMD: Vectorized scans and filters (no transfer needed)
```

## Cost Model Comparison

| Metric | CPU | GPU | FPGA |
|--------|-----|-----|------|
| Memory bandwidth | ~50 GB/s | ~900 GB/s (A100) | ~20-80 GB/s |
| Compute throughput | ~100 GFLOPS | ~20 TFLOPS | N/A (custom) |
| Latency per op | ~1-5 ns | ~5-10 ns (amortized) | ~3-5 ns |
| Transfer overhead | None | PCIe: 25 GB/s | PCIe/CXL |
| Crossover point | Baseline | 100K-1M rows | Streaming |
| Power efficiency | ~10-50W/core | ~300W total | ~25-75W |

## Key Systems Referenced

- **HeavyDB (OmniSci)**: Full GPU SQL database
- **PG-Strom**: PostgreSQL GPU offload extension
- **BlazingSQL**: GPU SQL engine on RAPIDS/cuDF
- **SQream**: GPU-native analytical database
- **IBM Netezza**: FPGA-accelerated appliance
- **Xilinx Alveo**: FPGA acceleration cards
- **DuckDB**: SIMD-optimized analytical engine
- **HyPer/Umbra**: NUMA-aware compiled execution

## Contributing

When adding hardware-specific rules:

1. Place the rule in the appropriate subdirectory
2. Include `hardware` field in YAML frontmatter
3. Document the crossover point (when hardware wins vs CPU)
4. Provide cost model with concrete hardware parameters
5. Include positive AND negative test cases
6. Reference real database implementations and papers
