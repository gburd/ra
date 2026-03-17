# Example: Hardware-Aware Optimization

This example demonstrates how the optimizer places operators across
CPU and GPU using the hardware cost model.

## Scenario

A TPC-H-style analytical query on a server with an NVIDIA A100 GPU:

```sql
SELECT l_returnflag, SUM(l_extendedprice * l_discount) AS revenue
FROM lineitem
WHERE l_shipdate BETWEEN '1995-01-01' AND '1996-12-31'
  AND l_discount BETWEEN 0.05 AND 0.07
  AND l_quantity < 24
GROUP BY l_returnflag;
```

**Table statistics:**
- lineitem: 600M rows, 120 bytes/row, 72 GB total
- l_returnflag: 3 distinct values
- Predicate selectivity: ~2% (compound)

## Step 1: CPU-Only Plan

Without hardware awareness, the optimizer produces a standard plan:

```
HashAggregate [groups=l_returnflag, agg=SUM(price*discount)]
  └─ Filter [shipdate BETWEEN ... AND discount BETWEEN ... AND qty < 24]
      └─ Scan [lineitem, 600M rows]
```

**CPU cost estimate:**
- Scan: 72 GB / 50 GB/s = 1.44 seconds
- Filter: 600M rows * 15 ns/row = 9.0 seconds (compound predicate)
- Aggregate: 12M rows * 80 ns/row = 0.96 seconds
- **Total: ~11.4 seconds**

## Step 2: Hardware Cost Analysis

The hardware cost model evaluates each operator on each device:

### Scan

| Device | Compute | Transfer | Total |
|--------|---------|----------|-------|
| CPU | 1.44s | 0 | 1.44s |
| GPU | 0.035s | 2.88s | 2.92s |

CPU wins for pure scan (PCIe bottleneck). But the scan is combined
with a compound filter, so we evaluate them together.

### Scan + Filter (combined)

| Device | Compute | Transfer | Total |
|--------|---------|----------|-------|
| CPU | 1.44 + 9.0s | 0 | 10.44s |
| GPU | 0.035 + 0.083s | 2.88s | 3.0s |

GPU wins because the compound predicate is compute-intensive. The
GPU evaluates 3 predicates per row in parallel across 108 SMs.

### Aggregate (3 groups, 12M input rows)

| Device | Compute | Transfer | Total |
|--------|---------|----------|-------|
| CPU | 0.96s | 0 | 0.96s |
| GPU | 0.007s | 0 | 0.007s |

GPU wins (data already on GPU from filter stage, no transfer needed).

## Step 3: Optimized Plan

The `heterogeneous-operator-placement` rule assigns operators:

```
TransferToHost
  └─ GpuAggregate [groups=l_returnflag, agg=SUM(price*discount)]
      └─ GpuFilter [shipdate BETWEEN ... AND discount BETWEEN ... AND qty < 24]
          └─ TransferToGpu
              └─ Scan [lineitem, 600M rows]
```

Then `host-to-device-transfer` pushes the column projection before
the GPU transfer to reduce PCIe data volume:

```
TransferToHost                          -- 3 rows * 24 bytes
  └─ GpuAggregate [l_returnflag, SUM]
      └─ GpuFilter [compound predicate]
          └─ TransferToGpu              -- 24 GB (4 columns, not all 16)
              └─ Project [l_shipdate, l_discount, l_quantity, l_extendedprice, l_returnflag]
                  └─ Scan [lineitem]
```

**Hardware-aware cost:**
- CPU scan: 72 GB / 50 GB/s = 1.44s
- Column projection: reduces 72 GB to ~24 GB (5 of 16 columns)
- PCIe transfer: 24 GB / 25 GB/s = 0.96s
- GPU filter: 600M rows * 15 ns / 108 SMs = 0.083s
- GPU aggregate: 12M rows * 80 ns / 108 = 0.007s
- Result transfer: negligible (3 rows)
- **Total: ~2.5 seconds** (4.6x speedup)

## Step 4: With Device Memory Caching

If lineitem is already cached on the GPU from a previous query
(the `device-memory-caching` rule), the transfer cost is eliminated:

```
TransferToHost
  └─ GpuAggregate [l_returnflag, SUM]
      └─ GpuFilter [compound predicate]
          └─ DeviceCached [lineitem]    -- already on GPU, 0 transfer
```

**Cached cost:**
- GPU filter: 0.083s
- GPU aggregate: 0.007s
- **Total: ~0.09 seconds** (127x speedup vs CPU-only)

## Key Takeaways

1. **Transfer cost dominates** for simple scans -- GPU only helps when
   combined with compute-heavy operations.

2. **Column projection before transfer** reduces PCIe data volume,
   often making GPU acceleration worthwhile.

3. **Device memory caching** eliminates the transfer bottleneck
   entirely for repeated queries on the same data.

4. **Compound predicates** are where GPU excels -- the more compute
   per row, the better the GPU payoff.

5. **Keep operator chains on the same device** to avoid round-trips.
   The aggregate stays on GPU because the filtered data is already
   there.
