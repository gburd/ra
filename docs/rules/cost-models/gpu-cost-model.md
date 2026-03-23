# Rule: "GPU Offloading Cost Model"

**Category:** cost-models
**File:** `rules/cost-models/gpu-cost-model.rra`

## Metadata

- **ID:** `gpu-cost-model`
- **Version:** "1.0.0"
- **Databases:** omnisci, blazingsql, sqream, duckdb
- **Tags:** cost, gpu, offloading, pcie, data-transfer, roofline, throughput
- **Authors:** "He et al. 2008 - relational joins on GPU", "Paul et al. 2016 - GPU cost models"


# GPU Offloading Cost Model

## Description

Determines whether a query operator should execute on GPU or CPU by
comparing estimated execution times on both devices. The decision
depends on three factors: (1) PCIe transfer cost to move data to GPU
memory, (2) GPU execution time given massive parallelism, and (3) CPU
execution time with SIMD and prefetching. GPU wins for large,
compute-bound, data-parallel operators; CPU wins for small, irregular,
or branchy workloads.

**When to apply**: Any operator in a hybrid CPU/GPU system. The cost
model assigns each operator to the device that minimizes total time
including data transfer overhead.

**Why it works**: GPU throughput can be 10-100x higher than CPU for
regular operations (scans, hashing, sorting), but PCIe transfer
(32 GB/s for PCIe 5.0) creates a breakeven point. Below this point,
CPU is faster because there is no transfer overhead.

## Relational Algebra

```algebra
-- GPU cost model per operator:
gpu_cost(op) = transfer_to_gpu(op) + gpu_exec(op) + transfer_from_gpu(op)
cpu_cost(op) = cpu_exec(op)

-- Transfer cost:
transfer_to_gpu(op) = DATA_SIZE(op.input) / PCIE_BW + PCIE_LATENCY
transfer_from_gpu(op) = DATA_SIZE(op.output) / PCIE_BW + PCIE_LATENCY

-- GPU execution (roofline model):
gpu_exec(op) = max(
  DATA_SIZE(op.input) / GPU_MEM_BW,   -- memory-bound
  FLOPS(op) / GPU_FLOPS               -- compute-bound
)

-- Decision:
place_on_gpu(op) = gpu_cost(op) < cpu_cost(op)

-- Data already on GPU eliminates transfer:
if input_resident_on_gpu(op):
  transfer_to_gpu(op) = 0
```

## Implementation

```rust
use egg::{rewrite as rw, *};

struct GpuCostModel {
    pcie_bw_gbps: f64,
    pcie_latency_us: f64,
    gpu_mem_bw_gbps: f64,
    gpu_flops_tflops: f64,
    cpu_mem_bw_gbps: f64,
    cpu_flops_gflops: f64,
}

impl GpuCostModel {
    fn transfer_cost(&self, data_bytes: f64) -> f64 {
        let gb = data_bytes / 1e9;
        (gb / self.pcie_bw_gbps) * 1e6 + self.pcie_latency_us
    }

    fn gpu_exec_time(&self, data_bytes: f64, flops: f64) -> f64 {
        let mem_time = (data_bytes / 1e9) / self.gpu_mem_bw_gbps;
        let compute_time = (flops / 1e12) / self.gpu_flops_tflops;
        mem_time.max(compute_time) * 1e6 // microseconds
    }

    fn cpu_exec_time(&self, data_bytes: f64, flops: f64) -> f64 {
        let mem_time = (data_bytes / 1e9) / self.cpu_mem_bw_gbps;
        let compute_time = (flops / 1e9) / self.cpu_flops_gflops;
        mem_time.max(compute_time) * 1e6
    }

    fn should_offload(
        &self,
        input_bytes: f64,
        output_bytes: f64,
        flops: f64,
        input_on_gpu: bool,
    ) -> (bool, f64) {
        let xfer_in = if input_on_gpu {
            0.0
        } else {
            self.transfer_cost(input_bytes)
        };
        let xfer_out = self.transfer_cost(output_bytes);
        let gpu_total = xfer_in
            + self.gpu_exec_time(input_bytes, flops)
            + xfer_out;

        let cpu_total = self.cpu_exec_time(input_bytes, flops);

        let speedup = cpu_total / gpu_total;
        (speedup > 1.0, speedup)
    }

    fn breakeven_data_size(&self, compute_intensity: f64) -> f64 {
        // Minimum data size where GPU wins
        // Solve: transfer + gpu_exec = cpu_exec
        // Approximate: 2 * size/pcie + size/gpu_bw = size/cpu_bw
        let cpu_rate = self.cpu_mem_bw_gbps * 1e9;
        let gpu_rate = self.gpu_mem_bw_gbps * 1e9;
        let pcie_rate = self.pcie_bw_gbps * 1e9;

        // Transfer overhead per byte
        let overhead_per_byte = 2.0 / pcie_rate + 1.0 / gpu_rate;
        let cpu_per_byte = 1.0 / cpu_rate;

        if overhead_per_byte >= cpu_per_byte {
            f64::MAX // GPU never wins for memory-bound ops
        } else {
            // Breakeven when fixed latency is amortized
            self.pcie_latency_us * 1e-6 * pcie_rate
        }
    }
}
```

## Preconditions

```rust
fn applicable(system: &SystemConfig) -> bool {
    system.has_gpu()
        && system.gpu_memory_bytes() > 0
}
```

**Restrictions:**
- GPU memory limits operator data size (8-80 GB typical)
- Multi-GPU adds communication cost between GPUs (NVLink vs PCIe)
- GPU kernels have launch overhead (~5-10us per kernel)
- Irregular access patterns (hash probes) underutilize GPU bandwidth
- String operations are typically slower on GPU than CPU

## Cost Model

```rust
fn operator_gpu_suitability(op: &Operator) -> GpuSuitability {
    match op {
        Operator::SeqScan { .. } => GpuSuitability::Good,
        Operator::Filter { .. } => GpuSuitability::Good,
        Operator::HashJoinBuild { .. } => GpuSuitability::Good,
        Operator::Sort { .. } => GpuSuitability::Moderate,
        Operator::Aggregate { .. } => GpuSuitability::Good,
        Operator::NestedLoop { .. } => GpuSuitability::Poor,
        Operator::StringOp { .. } => GpuSuitability::Poor,
        Operator::IndexScan { .. } => GpuSuitability::Poor,
    }
}
```

**Typical breakeven points (PCIe 4.0, A100):**
- Table scan: >1M rows (GPU 10-50x faster above breakeven)
- Hash join build: >500K rows
- Sort: >10M rows (GPU sort has high fixed overhead)
- Aggregation: >1M groups

## Test Cases

### Positive: Large table scan with filter

```sql
-- lineitem: 600M rows, 100 bytes/row = 60 GB
-- Filter selectivity 1%
SELECT * FROM lineitem WHERE l_quantity > 49;

-- CPU: 60 GB / 50 GB/s = 1.2s
-- GPU: 60 GB / 32 GB/s (PCIe) + 60 GB / 1500 GB/s (HBM) = 1.9s + 0.04s
-- GPU loses due to PCIe bottleneck for simple scan
-- BUT with data resident on GPU: 0.04s (30x faster)
```

### Positive: Hash aggregation (compute-bound)

```sql
-- 100M rows, GROUP BY with 10 aggregates
SELECT category, COUNT(*), SUM(price), AVG(price), ...
FROM transactions GROUP BY category;

-- CPU: ~2s (memory-bound, cache misses on hash table)
-- GPU: 0.1s transfer + 0.05s compute = 0.15s (13x faster)
-- Aggregation is compute-bound: GPU wins
```

### Negative: Small table or irregular access

```sql
-- departments: 50 rows
SELECT * FROM departments WHERE id = 42;

-- CPU: <1us
-- GPU: 5us kernel launch + 10us transfer = 15us
-- GPU overhead exceeds total CPU time
```

## References

**GPU query processing:**
- He et al., "Relational Joins on Graphics Processors", SIGMOD 2008
- Bress et al., "GPU-Accelerated Database Systems: Survey and Open Challenges", DBKDA 2014
- Paul et al., "GPU Join Processing Revisited", DaMoN 2016

**Cost models for heterogeneous systems:**
- Bress et al., "Robust Query Processing in Co-Processor-accelerated Databases", SIGMOD 2016
  - Formal cost model for CPU/GPU placement decisions
- Roofline model: Williams et al., "Roofline: An Insightful Visual Performance Model", CACM 2009
