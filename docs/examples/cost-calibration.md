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

### Step 1: Run Calibration Benchmarks

```bash
# Run comprehensive calibration suite
cargo run --bin ra-cli -- calibrate \
  --workload tpch \
  --scale-factor 10 \
  --iterations 5 \
  --output my-hardware.json

# Output:
# Running calibration benchmarks...
# [x] Sequential scan: 0.8 units/page (baseline: 1.0)
# [x] Random I/O: 1.2 units/page (baseline: 4.0)
# [x] CPU tuple cost: 0.005 units (baseline: 0.01)
# [x] Hash join: 0.02 units/tuple
# [x] Sort: 0.03 units/tuple
# [x] Network transfer: 2.0 units/MB
# Calibration saved to my-hardware.json
```

### Step 2: Workload-Specific Calibration

```bash
# Calibrate using your actual queries
cargo run --bin ra-cli -- calibrate \
  --workload-file production_queries.sql \
  --statistics current_stats.json \
  --runtime-samples query_times.csv \
  --output production-calibrated.json
```

### Step 3: Validate Calibration

```bash
# Compare predicted vs actual costs
cargo run --bin ra-cli -- validate-costs \
  --model my-hardware.json \
  --queries test_queries.sql \
  --actual-times measured_times.csv

# Output:
# Cost Model Accuracy:
# - Mean Absolute Error: 12%
# - Correlation: 0.94
# - Queries within 20% of actual: 87%
```

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

```bash
cargo run --bin ra-cli -- calibrate \
  --hardware gpu \
  --gpu-model "RTX 4090" \
  --gpu-memory 24GB \
  --operations "scan,filter,join,aggregate"

# Produces GPU-specific costs:
{
  "gpu_transfer_cost": 5.0,      // CPU->GPU transfer
  "gpu_scan_cost": 0.001,         // Parallel scan
  "gpu_filter_cost": 0.0005,      // Parallel filter
  "gpu_hash_join_cost": 0.002,    // GPU hash join
  "gpu_sort_cost": 0.003          // GPU radix sort
}
```

### SIMD Calibration

```bash
cargo run --bin ra-cli -- calibrate \
  --hardware cpu \
  --simd avx512 \
  --operations "scan,filter,arithmetic"

# Results in SIMD-aware costs:
{
  "simd_scan_cost": 0.002,        // 8x parallel
  "simd_filter_cost": 0.001,      // Vectorized comparison
  "simd_arithmetic_cost": 0.0005  // Vectorized math
}
```

## Using Calibrated Models

### Apply to Single Query

```bash
cargo run --bin ra-cli -- optimize \
  --cost-model my-hardware.json \
  "SELECT * FROM orders WHERE status = 'pending'"
```

### Set as Default

```toml
# ~/.ra/config.toml
[cost_model]
default = "my-hardware.json"

[cost_model.overrides]
"analytical/*" = "nvme-analytical.json"
"transactional/*" = "memory-oltp.json"
```

### A/B Testing

```bash
# Compare plans with different cost models
cargo run --bin ra-cli -- compare-models \
  --model1 default \
  --model2 my-hardware.json \
  --queries workload.sql

# Output:
# Plan differences: 23/100 queries
# Estimated improvement: 34% average
# Biggest win: query_17 (10x faster)
# Biggest risk: query_42 (1.5x slower)
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

### Automatic Adjustment

```bash
# Enable learning mode
cargo run --bin ra-cli -- monitor \
  --learn-costs \
  --feedback-log query_performance.log \
  --update-interval daily
```

### Drift Detection

```bash
# Detect when calibration is outdated
cargo run --bin ra-cli -- detect-drift \
  --model my-hardware.json \
  --recent-queries last_7_days.log

# Output:
# Cost model drift detected:
# - Random I/O 30% faster than model
# - CPU costs 15% higher than model
# Recommendation: Recalibrate
```

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