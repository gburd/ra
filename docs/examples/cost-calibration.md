# Cost Model Calibration Example

This example shows how to calibrate RA's cost model to match your specific hardware and workload characteristics.

## Why Calibrate?

Default cost models assume generic hardware:
- Sequential I/O: 1.0 cost unit per page
- Random I/O: 4.0 cost units per page
- CPU operation: 0.01 cost units per tuple

Your hardware might be very different:
- **NVMe SSD**: Random I/O nearly as fast as sequential
- **Cloud Storage**: Network latency dominates
- **GPU**: Parallel operations much cheaper
- **Large RAM**: More data stays in cache

## Calibration Process

Cost model calibration involves manually tuning the cost model parameters to match your hardware characteristics. RA uses these parameters during query optimization to estimate execution costs.

### Step 1: Understand Your Hardware

Measure key characteristics:
- Sequential I/O performance
- Random I/O performance
- CPU processing speed
- Memory bandwidth
- Network latency and throughput

### Step 2: Configure Cost Model

Create a custom cost model JSON file with calibrated parameters (see examples below).

### Step 3: Use Custom Cost Model

RA reads cost model configuration from your environment or project configuration files. See the Configuration section below for details.

## Calibration Results

### Example: SSD vs HDD

```json
// hdd-costs.json
{
  "seq_page_cost": 1.0,
  "random_page_cost": 4.0,
  "cpu_tuple_cost": 0.01,
  "cpu_index_tuple_cost": 0.005,
  "cpu_operator_cost": 0.0025
}

// nvme-costs.json
{
  "seq_page_cost": 0.1,
  "random_page_cost": 0.15,  // Nearly sequential!
  "cpu_tuple_cost": 0.01,
  "cpu_index_tuple_cost": 0.005,
  "cpu_operator_cost": 0.0025
}
```

Impact on plan selection:

```sql
-- Query with potential index scan
SELECT * FROM large_table WHERE id IN (1,2,3,...,100);

-- With HDD costs: Sequential scan (avoid random I/O)
-- With NVMe costs: Index scan (random I/O is cheap)
```

### Example: Memory-Resident Data

```json
// large-memory-costs.json
{
  "cache_hit_ratio": 0.95,  // 95% in memory
  "seq_page_cost": 0.05,    // Mostly memory access
  "random_page_cost": 0.05,  // No difference in memory
  "parallel_setup_cost": 10.0,
  "parallel_tuple_cost": 0.001
}
```

Impact on optimization:

```sql
-- Large aggregation query
SELECT category, SUM(amount) FROM sales GROUP BY category;

-- With disk costs: External sort to minimize I/O
-- With memory costs: Hash aggregation (all in memory)
```

## Hardware-Specific Calibration

### GPU Calibration

GPU-specific cost parameters:

```json
{
  "gpu_transfer_cost": 5.0,      // CPU->GPU transfer
  "gpu_scan_cost": 0.001,         // Parallel scan
  "gpu_filter_cost": 0.0005,      // Parallel filter
  "gpu_hash_join_cost": 0.002,    // GPU hash join
  "gpu_sort_cost": 0.003          // GPU radix sort
}
```

### SIMD Calibration

SIMD-aware cost parameters:

```json
{
  "simd_scan_cost": 0.002,        // 8x parallel
  "simd_filter_cost": 0.001,      // Vectorized comparison
  "simd_arithmetic_cost": 0.0005  // Vectorized math
}
```

## Using Calibrated Models

### Configure Hardware Profile

Use hardware profiles that match your system characteristics:

```bash
# Use server profile (optimized for high-end hardware)
ra-cli optimize \
  --hardware-profile server \
  "SELECT * FROM orders WHERE status = 'pending'"

# Use GPU server profile for GPU-accelerated workloads
ra-cli optimize \
  --hardware-profile gpu-server \
  "SELECT * FROM large_dataset GROUP BY category"

# Let RA auto-detect the best profile
ra-cli optimize \
  --hardware-profile auto \
  "SELECT * FROM orders WHERE status = 'pending'"
```

Available profiles: `edge`, `mobile`, `laptop`, `desktop`, `server`, `gpu-server`, `auto`

### Set Default Configuration

```toml
# ~/.ra/config.toml
[cost_model]
seq_page_cost = 0.1
random_page_cost = 0.15
cpu_tuple_cost = 0.01

[hardware]
cpu_cores = 16
memory_gb = 64
gpu_available = true
```

## Calibration Parameters

### Basic Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| seq_page_cost | 1.0 | Sequential I/O cost per 8KB page |
| random_page_cost | 4.0 | Random I/O cost per 8KB page |
| cpu_tuple_cost | 0.01 | CPU cost per tuple processed |
| cpu_index_tuple_cost | 0.005 | CPU cost per index tuple |
| cpu_operator_cost | 0.0025 | CPU cost per operator call |

### Advanced Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| parallel_setup_cost | 1000.0 | Fixed cost to start parallel workers |
| parallel_tuple_cost | 0.1 | Cost per tuple in parallel mode |
| network_transfer_cost | 10.0 | Cost per MB over network |
| cache_hit_ratio | 0.9 | Fraction of pages in cache |
| effective_cache_size | "4GB" | Total cache available |

### Cloud-Specific Parameters

```json
{
  "s3_get_cost": 0.0004,          // Per 1000 requests
  "s3_scan_cost": 0.00001,        // Per GB scanned
  "lambda_invoke_cost": 0.0000002, // Per invocation
  "cross_az_transfer": 0.01       // Per GB between AZs
}
```

## Continuous Calibration

### Monitor Query Performance

Use the monitoring tools to track query performance:

```bash
# Monitor database with tuning advice
ra-cli monitor \
  --postgres 'host=localhost dbname=prod' \
  --tui
```

### Detect Performance Drift

Monitor query execution times and compare with cost model predictions to detect when recalibration is needed. Manually update cost model parameters based on observed performance characteristics.

## Best Practices

1. **Calibrate with representative workload** - Use actual production queries
2. **Recalibrate after hardware changes** - New SSDs, RAM upgrades, etc.
3. **Separate OLAP/OLTP calibration** - Different workloads need different models
4. **Monitor prediction accuracy** - Track model drift over time
5. **Test before deploying** - Validate new models on staging

## Related Topics

- **[Cost Models Guide](../guides/cost-models.md)** - Deep dive into cost modeling
- **[Hardware Optimization](hardware-aware-optimization.md)** - Hardware-specific plans
- **[Performance Tuning](../guides/performance-tuning.md)** - Overall optimization