# Rule: Minimize Host-to-Device Data Transfer

**Category:** hardware/data-placement
**File:** `rules/hardware/data-placement/host-to-device-transfer.rra`

## Metadata

- **ID:** `host-to-device-transfer`
- **Version:** "1.0.0"
- **Databases:** heavydb, blazingsql, pg-strom, sqream
- **Tags:** transfer, pcie, data-placement, memory, optimization
- **Authors:** "RA Contributors"


# Minimize Host-to-Device Data Transfer

## Description

Reorders operators to minimize the volume of data transferred between
host (CPU) memory and device (GPU/FPGA) memory over the PCIe bus.
Pushes selective filters and projections before device transfer to
reduce the data volume that crosses the bus. Keeps chains of
device-executable operators on the device to avoid round-trips.

**When to apply**: A query plan mixes CPU and device operators. The
optimizer should minimize PCIe transfers by pushing reductions
(filters, projections) to before the transfer point, and keeping
consecutive device operators together.

**Why it works**: PCIe bandwidth (16-32 GB/s) is far lower than
device memory bandwidth (GPU HBM: 900+ GB/s). Each host-to-device
and device-to-host transfer adds latency. By reducing data volume
before transfer and batching device operations, the plan avoids the
PCIe bottleneck.

## Relational Algebra

```algebra
gpu_op(transfer_to_gpu(R))
  -> gpu_op(transfer_to_gpu(sigma[p](R)))
  where sigma[p] is CPU-executable
    AND selectivity(p) * size(R) << size(R)

gpu_op1(transfer_to_host(transfer_to_gpu(gpu_op2(R))))
  -> transfer_to_host(gpu_op1(gpu_op2(R)))
  // Eliminate round-trip by keeping both ops on device
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Push CPU filter before GPU transfer
rw!("filter-before-gpu-transfer";
    "(gpu_scan (transfer_to_gpu ?input))" =>
    "(gpu_scan (transfer_to_gpu (filter ?pred ?input)))"
    if has_pushable_filter("?input")
),

// Eliminate unnecessary round-trips
rw!("eliminate-device-roundtrip";
    "(gpu_op1 (transfer_to_host (transfer_to_gpu (gpu_op2 ?input))))" =>
    "(transfer_to_host (gpu_op1 (gpu_op2 ?input)))"
    if both_ops_device_compatible("gpu_op1", "gpu_op2")
),

// Merge adjacent transfers
rw!("merge-adjacent-transfers";
    "(transfer_to_gpu (transfer_to_host ?input))" =>
    "?input"
    if input_already_on_device("?input")
),
```

## Preconditions

```rust
fn applicable(plan: &RelExpr) -> bool {
    let transfer_count = count_transfers(plan);
    let round_trips = count_round_trips(plan);
    // Apply when there are redundant transfers to eliminate
    transfer_count > 1 || round_trips > 0
}
```

**Restrictions:**
- Filter pushdown must not change semantics (same as logical rules)
- Some operators may not be device-compatible (must stay on CPU)
- Pinned memory transfers are faster but limited

## Cost Model

```rust
fn estimated_benefit(
    data_bytes_before: u64,
    data_bytes_after: u64,
    eliminated_round_trips: u32,
    hw: &HardwareProfile,
) -> f64 {
    let pcie_bw = hw.pcie_bandwidth_gbps * 1e9;
    let pcie_latency_ns = 1_000.0; // ~1us per transfer

    let cost_before = data_bytes_before as f64 / pcie_bw
        + pcie_latency_ns
            * (eliminated_round_trips as f64 + 1.0)
            * 2.0;
    let cost_after = data_bytes_after as f64 / pcie_bw
        + pcie_latency_ns;

    if cost_before > cost_after {
        (cost_before - cost_after) / cost_before
    } else {
        0.0
    }
}
```

**Typical benefit**: 2x-10x reduction in PCIe transfer time. Critical
for queries where transfer time dominates computation time.

## Test Cases

### Positive: Push filter before GPU transfer

```sql
-- Before: Transfer all rows, then GPU filters
-- Transfer 600M rows * 120 bytes = 72 GB
SELECT * FROM lineitem WHERE l_shipdate > '1998-01-01';

-- After: CPU index scan first, transfer only matching rows
-- Transfer 60M rows * 120 bytes = 7.2 GB (10x less)
-- Plan: GpuScan(transfer_to_gpu(IndexScan(lineitem,
--        pred=l_shipdate > '1998-01-01')))
```

### Positive: Eliminate round-trip

```sql
-- Before: GPU filter -> transfer to host -> transfer to GPU -> GPU agg
-- After: GPU filter -> GPU agg -> transfer to host (single transfer)
SELECT l_returnflag, SUM(l_quantity)
FROM lineitem
WHERE l_discount > 0.05
GROUP BY l_returnflag;

-- Plan: TransferToHost(GpuAggregate(GpuFilter(lineitem)))
```

### Positive: Project before transfer

```sql
-- Before: Transfer all 15 columns to GPU
-- After: Project to 3 needed columns first, transfer 5x less data
SELECT l_orderkey, SUM(l_extendedprice * l_discount)
FROM lineitem
GROUP BY l_orderkey;

-- Plan: GpuAggregate(transfer_to_gpu(
--        Project([l_orderkey, l_extendedprice, l_discount],
--                lineitem)))
```

## References

**Implementation in databases:**
- HeavyDB: Data manager handles GPU memory placement
- PG-Strom: DMA buffer management for GPU transfers
- NVIDIA RAPIDS: cuDF unified memory management

**Academic papers:**
- Bress et al., "Automatic Selection of Processing Units for Co-Processing in Databases", ADBIS 2012
- Heimel et al., "Hardware-Oblivious Parallelism for In-Memory Column-Stores", VLDB 2013
