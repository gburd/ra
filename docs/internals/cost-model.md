# Cost Model

This document describes RA's cost model: how the optimizer estimates the cost
of query plans, integrates statistics, accounts for hardware characteristics,
and adapts to runtime feedback. The cost model spans three crates:

- `ra-hardware` -- hardware profiles and device-aware cost estimation
- `ra-stats` -- statistics types, staleness tracking, and streaming pipeline
- `ra-engine` -- the integrated cost model that combines both

## Architecture

```
                    +---------------------+
                    |  IntegratedCostFn   |  (egg CostFunction impl)
                    +----------+----------+
                               |
                    +----------v----------+
                    | IntegratedCostModel |
                    +--+---------------+--+
                       |               |
          +------------v---+    +------v-----------+
          | StatisticsAdapter|  | HardwareProfile   |
          +---+--------+---+    +------+-----------+
              |        |               |
    +---------v-+  +---v--------+  +---v-----------+
    |TableStats |  |ColumnStats |  |HardwareCostModel|
    |Staleness  |  |IndexStats  |  |CostCalibration  |
    |Confidence |  |Histograms  |  |CostPruner       |
    +-----------+  +------------+  +-----------------+
```

The `IntegratedCostModel` (`crates/ra-engine/src/cost.rs:54`) is the central
struct. It holds a `StatisticsAdapter` for table metadata and a
`HardwareProfile` for machine characteristics. For each operator, it:

1. Looks up table statistics (falling back to a default of 1000 rows)
2. Adjusts row counts based on staleness
3. Applies hardware-specific cost factors
4. Discounts by confidence level

The `IntegratedCostFn` (`crates/ra-engine/src/cost.rs:892`) wraps this into
an `egg::CostFunction` implementation for the equality saturation optimizer.

---

## Statistics Integration

### Table Statistics

`TableStats` (`crates/ra-stats/src/types.rs:17`) captures physical table
metadata:

```rust
pub struct TableStats {
    pub row_count: u64,
    pub page_count: u64,
    pub average_row_size: f64,
    pub table_size_bytes: u64,
    pub live_tuples: Option<u64>,
    pub dead_tuples: Option<u64>,
    pub last_analyzed: Option<i64>,
}
```

Key derived metrics:

- **Dead tuple ratio**: `dead / (live + dead)` -- triggers vacuum decisions
  (`crates/ra-stats/src/types.rs:275`)
- **Fill factor**: `table_size_bytes / page_count` -- indicates page
  utilization (`crates/ra-stats/src/types.rs:290`)

### Column Statistics

`ColumnStats` (`crates/ra-stats/src/types.rs:36`) drives selectivity
estimation:

```
equality_selectivity = (1.0 - null_fraction) / ndv
range_selectivity    = (1.0 - null_fraction) * fraction
```

Where `ndv` is the number of distinct values and `fraction` is the portion
of the value domain covered by the range predicate
(`crates/ra-stats/src/types.rs:300`).

A column is classified as "high cardinality" when `ndv / row_count > 0.9`,
which influences index selection decisions.

### Index Statistics

`IndexStats` (`crates/ra-stats/src/types.rs:96`) includes:

- **Clustering factor**: how well index order matches heap order (1.0 =
  perfect, `row_count` = random)
- **Range scan page estimate**:

```
pages = min_pages + (max_pages - min_pages) * factor
where factor = clustering_factor / distinct_keys
      min_pages = total_pages * selectivity
```

(`crates/ra-stats/src/types.rs:333`)

A well-clustered index has `clustering_factor / row_count < 0.1`, making
sequential I/O patterns likely during range scans.

### Histogram Types

RA supports four histogram representations (`crates/ra-stats/src/types.rs:64`):

| Type | Use Case |
|------|----------|
| `EquiWidth` | Uniform distributions, simple range queries |
| `EquiDepth` | Skewed distributions, balanced bucket counts |
| `EndBiased` | PostgreSQL-style with MCV separation |
| `TDigest` | Streaming data, approximate percentile queries |

### Sketch Types

For approximate statistics on large datasets (`crates/ra-stats/src/types.rs:167`):

| Sketch | Purpose |
|--------|---------|
| `HyperLogLog` | NDV estimation (cardinality) |
| `CountMinSketch` | Frequency estimation |
| `BloomFilter` | Membership testing |

---

## Staleness Model

Statistics become stale as data changes. The staleness model
(`crates/ra-stats/src/accuracy.rs:48`) classifies staleness based on the
ratio of modifications since the last ANALYZE to the row count at gathering
time:

```
change_rate = modifications_since / rows_at_gathering

Staleness classification:
  change_rate < 0.01  =>  Fresh
  change_rate < 0.05  =>  SlightlyStale
  change_rate < 0.20  =>  ModeratelyStale
  change_rate >= 0.20 =>  VeryStale
  (zero rows)         =>  Unknown
```

### Staleness Inflation

When statistics are stale, the cost model inflates row count estimates to
bias the optimizer toward plans robust to cardinality mis-estimation. The
inflation factors (`crates/ra-engine/src/cost.rs:23`):

```
Staleness           Factor    Effect
---------           ------    ------
Fresh               1.0       No adjustment
SlightlyStale       1.05      +5% inflation
ModeratelyStale     1.2       +20% inflation
VeryStale           1.5       +50% inflation
Unknown             2.0       +100% inflation (doubled)
```

The practical effect: stale statistics push the optimizer toward hash joins
over nested loops, and toward plans that tolerate cardinality errors.

### Confidence Discount

Statistics from different sources carry different confidence levels
(`crates/ra-stats/src/accuracy.rs:62`):

```
Source          Initial Confidence
------          ------------------
ExactCount      1.0
Sampled(n%)     n/100
Histogram       0.8
MlModel         0.7
Derived         0.6
Default         0.3
```

The confidence discount function (`crates/ra-engine/src/cost.rs:41`):

```
discount = 2.0 - confidence.clamp(0.0, 1.0)
```

This produces a multiplier in `[1.0, 2.0]`:
- confidence = 1.0 -> discount = 1.0 (no penalty)
- confidence = 0.5 -> discount = 1.5 (50% cost increase)
- confidence = 0.0 -> discount = 2.0 (doubled cost)

Low-confidence statistics inflate cost estimates, making the optimizer
conservative when it has poor information.

### Confidence Decay

Confidence decays exponentially with time (`crates/ra-stats/src/accuracy.rs:144`):

```
confidence *= exp(-decay_rate * age_days)
```

Where `age_days` = seconds since gathering / 86400. This models the
intuition that older statistics are less trustworthy.

### Quality Metrics

The `QualityMetrics` struct (`crates/ra-stats/src/accuracy.rs:172`)
combines three dimensions into an overall quality score:

```
quality_score = (freshness + confidence + coverage) / 3.0
```

Where:
- `freshness` maps Staleness to [0.0, 1.0]
- `confidence` is the source confidence
- `coverage` maps the StatisticsSource to [0.1, 1.0]

---

## Hardware Profile Integration

### HardwareProfile

`HardwareProfile` (`crates/ra-hardware/src/profile.rs:13`) describes the
complete system hardware:

```
CPU:      cores, L2/L3 cache, SIMD width, NUMA nodes,
          memory bandwidth, memory-level parallelism
GPU:      SM count, memory, bandwidth, unified memory support
FPGA:     clock MHz, BRAM, pipeline depth, regex engines
Storage:  bandwidth (GB/s)
Network:  PCIe bandwidth (GB/s)
```

Predefined profiles include GPU servers (A100 80GB), FPGA appliances
(Alveo U280), and CPU-only analytics servers.

### Cost Calibration

`CostCalibration` (`crates/ra-engine/src/cost.rs:811`) normalizes costs to
a reference machine and produces per-operator scaling factors:

```
Reference machine: 8 cores, 16 MB L3, 256-bit SIMD, 3.5 GB/s storage

scan_factor      = ref_storage_bw / actual_storage_bw
filter_factor    = ref_simd_bits  / actual_simd_bits
join_factor      = ref_cache_mb   / actual_cache_mb
sort_factor      = max(ref_cores / actual_cores, 0.5)
aggregate_factor = ref_cache_mb   / actual_cache_mb
```

(`crates/ra-engine/src/cost.rs:835`)

Values < 1.0 indicate the actual hardware is faster than the reference;
values > 1.0 indicate slower hardware. This ensures cost comparisons
are meaningful across different deployment targets.

---

## Cost Estimation Formulas

All cost functions live in `IntegratedCostModel`
(`crates/ra-engine/src/cost.rs:54`). Each applies the same pattern:

```
cost = base_formula * hardware_factor * confidence_discount
```

### Scan Cost

```
storage_factor = 100.0 / storage_bandwidth_gbps
base = row_count * avg_row_size / (1024 * 1024)
cost = base * storage_factor * confidence_discount
```

(`crates/ra-engine/src/cost.rs:136`)

Faster storage (higher bandwidth) reduces scan cost linearly.

### Filter Cost

```
simd_factor = 256.0 / simd_width_bits
cost = row_count * 0.001 * simd_factor * confidence_discount
```

(`crates/ra-engine/src/cost.rs:151`)

Wider SIMD registers (AVX-512 = 512 bits) reduce filter cost by processing
more values per instruction.

### Join Cost (Hash Join)

```
cache_mb = l3_cache_bytes / (1024 * 1024)
cache_factor = 16.0 / max(cache_mb, 1.0)

build_rows = min(left_rows, right_rows)
probe_rows = max(left_rows, right_rows)

cost = (build_rows * 100e-6 + probe_rows * 50e-6)
     * cache_factor
     * max(confidence_left, confidence_right)
```

(`crates/ra-engine/src/cost.rs:164`)

The build side costs 100 ns/row (hash table insertion) and the probe side
50 ns/row (hash lookup). Larger L3 caches reduce the cache_factor, making
hash joins cheaper when the hash table fits in cache.

### Join Cost with Runtime Filter

When a bloom/min-max filter is built during the hash join build phase and
pushed to the probe-side scan (`crates/ra-engine/src/cost.rs:196`):

```
filter_build_cost = build_rows * 10e-9
filter_apply_cost = probe_rows * 20e-9
effective_probe   = probe_rows * selectivity

join_cost = (build_rows * 100e-6 + effective_probe * 50e-6)
          * cache_factor

total = join_cost + filter_build_cost + filter_apply_cost
```

The runtime filter reduces the effective probe row count, which can
substantially reduce join cost when the filter is selective.

### Sort Cost

```
n_log_n = n * log2(n)    (for n > 1)
par_factor = max(8.0 / cpu_cores, 0.5)
cost = n_log_n * 200e-9 * par_factor * confidence_discount
```

(`crates/ra-engine/src/cost.rs:232`)

More CPU cores reduce sort cost through the parallelism factor (capped at
50% reduction).

### Incremental Sort Cost

When input is already sorted by prefix columns
(`crates/ra-engine/src/cost.rs:254`):

```
groups = min(prefix_ndv, n)
avg_group_size = n / groups
group_sort = avg_group_size * log2(avg_group_size)
total = groups * group_sort

cost = total * 200e-9 * par_factor * confidence_discount
```

Incremental sort is cheaper than full sort when prefix_ndv is large relative
to the total row count, because each group is small.

### Aggregate Cost

```
cache_factor = 16.0 / max(cache_mb, 1.0)

cost = (row_count * 80e-9 + group_count * 64.0 * cache_factor * 1e-9)
     * cache_factor
     * confidence_discount
```

(`crates/ra-engine/src/cost.rs:279`)

The per-row cost (80 ns) covers hash computation and group lookup. The
per-group cost accounts for hash table memory, scaled by cache efficiency.

### Covering Index Scan Cost

```
cost = scan_cost(table) * 0.3
```

(`crates/ra-engine/src/cost.rs:302`)

Index-only scans avoid heap fetches, reducing cost to ~30% of a regular scan.

### Bitmap Index Scan Cost

```
index_pages = max(row_count * selectivity / 100, 1.0)
index_cost  = index_pages * storage_factor * 0.3
bitmap_cost = row_count / 64 * 1e-9

total = (index_cost + bitmap_cost) * confidence_discount
```

(`crates/ra-engine/src/cost.rs:313`)

### Bitmap Combine Cost (AND/OR)

```
bitmap_words = max(row_count / 64, 1.0)
cost = bitmap_words * 1e-10 * num_bitmaps
```

(`crates/ra-engine/src/cost.rs:337`)

Bitwise operations run at memory bandwidth speed.

### Bitmap Heap Scan Cost

```
pages_accessed = max(row_count * selectivity / 100, 1.0)
heap_cost    = pages_accessed * storage_factor * 0.25
recheck_cost = row_count * selectivity * 5e-9

total = (heap_cost + recheck_cost) * confidence_discount
```

(`crates/ra-engine/src/cost.rs:356`)

Sequential access after bitmap is ~4x faster than random.

### Full Bitmap Scan Cost

Combines individual index scan costs + bitmap combine cost + heap scan
cost for multiple predicates (`crates/ra-engine/src/cost.rs:384`):

```
index_costs    = sum(bitmap_index_scan_cost(table, sel_i))
combine_cost   = bitmap_combine_cost(table, num_predicates)
combined_sel   = product(selectivities)
heap_cost      = bitmap_heap_scan_cost(table, combined_sel)

total = index_costs + combine_cost + heap_cost
```

### Parquet Scan Cost

```
sel = pruning_selectivity.clamp(0.0, 1.0)
metadata_overhead = scan_cost(table) * 0.01
cost = scan_cost(table) * sel + metadata_overhead
```

(`crates/ra-engine/src/cost.rs:417`)

Row group pruning via predicate pushdown reduces the data scanned.

### LIMIT-Adjusted Cost

```
fraction = (limit_rows / estimated_total_rows).clamp(0.0, 1.0)
startup_floor = 0.1
effective_fraction = max(fraction, startup_floor)
cost = total_cost * effective_fraction
```

(`crates/ra-engine/src/cost.rs:443`)

The 10% startup floor accounts for hash table build, sort initialization,
and other blocking work that must complete before the first row is emitted.

---

## Parallel Execution Cost Model

For parallel query execution, the cost model uses Amdahl's law with
additional coordination and contention factors.

### Parallel Efficiency

```
serial_fraction   = 0.05       (5% of work is inherently serial)
parallel_fraction = 0.95

amdahl_speedup = 1.0 / (serial_fraction + parallel_fraction / workers)

coordination_factor = 0.95^(workers - 1)    (5% loss per worker)
contention_factor   = max(1.0 - 0.1 * min(workers - 1, 5), 0.5)

efficiency = amdahl_speedup * coordination_factor * contention_factor
```

(`crates/ra-engine/src/cost.rs:574`)

### Parallel Coordination Cost

```
startup_cost = 1000.0 * workers           (us)
sync_cost    = 100.0 * workers * log2(workers)  (us)
gather_cost  = 50.0 * workers             (us)

total = (startup + sync + gather) * 1e-6
```

(`crates/ra-engine/src/cost.rs:605`)

### Parallel Scan Cost

```
cost = scan_cost(table) / parallel_efficiency(workers)
     + parallel_coordination_cost(workers)
```

(`crates/ra-engine/src/cost.rs:555`)

### Parallel Hash Join Cost

```
build_cost         = build_rows * 100e-6         (sequential)
probe_cost         = probe_rows * 50e-6 / efficiency
coordination       = parallel_coordination_cost(workers)

total = (build_cost + probe_cost + coordination) * confidence_discount
```

(`crates/ra-engine/src/cost.rs:630`)

The build phase is sequential (shared hash table); the probe phase is
parallelized across workers.

### Parallel Aggregation Cost (Two-Phase)

```
Phase 1 (parallelized):
  rows_per_worker    = input_rows / workers
  partial_cost       = rows_per_worker * 80e-9 / efficiency

Phase 2 (sequential combine):
  combine_rows       = groups_per_worker * workers
  combine_cost       = combine_rows * 100e-9

total = partial_cost + combine_cost + coordination_cost
```

(`crates/ra-engine/src/cost.rs:666`)

### Optimal Worker Count

```
if rows < 10,000:
    workers = 1   (no parallelism for small inputs)
else:
    workers = min(ceil(rows / 100,000), cpu_cores, max_workers)
```

(`crates/ra-engine/src/cost.rs:704`)

---

## Device-Aware Cost Model

`HardwareCostModel` (`crates/ra-hardware/src/cost.rs:21`) estimates cost on
CPU, GPU, and FPGA independently, choosing the cheapest device:

### CPU Scan
```
time = data_bytes / (cpu_memory_bandwidth * 1e9)
```

### GPU Scan
```
transfer = data_bytes / (pcie_bandwidth * 1e9)
compute  = data_bytes / (gpu_memory_bandwidth * 1e9)
time     = transfer + compute
```

### FPGA Scan
```
clock_period = 1.0 / (clock_mhz * 1e6)
time = row_count * clock_period
```

### GPU Hash Join
```
transfer = total_bytes / (pcie_bandwidth * 1e9)
build    = build_rows * 100e-9 / sm_count
probe    = probe_rows * 50e-9  / sm_count
time     = build + probe + transfer
```

GPU is faster for large hash joins because the SM count divides the
per-row cost. For scan-only workloads, CPU typically wins because PCIe
bandwidth is lower than CPU memory bandwidth.

### GPU Sort
```
time = n_log_n * 200e-9 / sm_count + transfer
```

FPGA sort is unsupported (returns infinite cost).

### Device Selection

`best_scan_device` (`crates/ra-hardware/src/cost.rs:230`) evaluates all
available devices and picks the one with lowest total cost. Small data
always runs on CPU to avoid transfer overhead.

---

## Cost-Based Pruning

`CostPruner` (`crates/ra-engine/src/cost_pruning.rs:14`) implements
branch-and-bound search space reduction inspired by Apache Calcite's
Volcano planner:

```
should_prune = cost > global_best * threshold
```

With the default threshold of 1.5x (prune plans >50% worse than best):

```rust
let pruner = CostPruner::new(1.5);
pruner.record_cost(eclass_id, 100.0);     // best = 100
pruner.should_prune_class(id2, 140.0);    // false (1.4x)
pruner.should_prune_class(id3, 160.0);    // true  (1.6x, pruned)
```

Pruning reduces extraction time by 30-50% on complex queries while
maintaining plan quality within acceptable bounds.

`PruningStats` tracks effectiveness (`crates/ra-engine/src/cost_pruning.rs:140`):

```
pruning_rate = classes_pruned / classes_evaluated * 100
```

---

## Q-Error Calibration

The feedback system uses **q-error** as the standard metric for estimation
accuracy (`crates/ra-stats/src/feedback.rs:103`):

```
q_error = max(actual / estimated, estimated / actual)
```

Both values are clamped to a minimum of 1.0 to avoid division by zero.
A q-error of 1.0 means perfect estimation. The metric is symmetric: a
10x overestimate and 10x underestimate both yield q-error = 10.

### Severity Classification

```
q_error < 2.0   =>  Low       (acceptable estimation)
q_error < 10.0  =>  Medium    (correlation or skew issues)
q_error >= 10.0 =>  High      (likely stale statistics)
```

(`crates/ra-stats/src/feedback.rs:110`)

### Error Tracking

`CardinalityErrorTracker` (`crates/ra-stats/src/feedback.rs:126`) collects
per-operator observations and computes aggregate statistics:

```rust
let mut tracker = CardinalityErrorTracker::new();
tracker.record("orders", OperatorKind::Scan, 100.0, 500.0, None);
// q-error = 5.0, severity = Medium

tracker.mean_q_error();    // average across all observations
tracker.median_q_error();  // resistant to outliers
tracker.max_q_error();     // worst case

tracker.worst_tables(5);   // tables ranked by avg q-error
tracker.worst_operators(3); // operator types ranked by avg q-error
```

### Recommendation Engine

`RecommendationEngine` (`crates/ra-stats/src/feedback.rs:361`) maps error
patterns to actionable fixes:

| Error Pattern | Threshold | Recommendation |
|---------------|-----------|----------------|
| High scan q-errors (avg >= 10) | `analyze_threshold` | ANALYZE table |
| High join q-errors (avg >= 5) | `extended_stats_threshold` | Extended statistics for correlated columns |
| High filter q-errors (avg >= 5) | `index_threshold` | Create missing index |
| Moderate scan q-errors (3-10) | `histogram_threshold` | Histogram for skewed columns |

### Execution Feedback Loop

`IntegratedCostModel::apply_execution_feedback`
(`crates/ra-engine/src/cost.rs:503`) closes the loop by adjusting
confidence based on observed q-errors:

```
q_error <= 1.5   =>  no change
q_error <= 3.0   =>  -10% confidence
q_error <= 10.0  =>  -25% confidence
q_error > 10.0   =>  -50% confidence
```

Reduced confidence increases the confidence discount on future cost
estimates, making the optimizer more conservative for tables with poor
estimation history. This creates a self-correcting feedback mechanism:
tables with consistently bad estimates get increasingly penalized until
their statistics are refreshed.

---

## Adaptive Cost Model (Streaming Statistics)

The streaming statistics pipeline (`crates/ra-stats/src/streaming.rs:22`)
provides continuous cost model adaptation based on runtime resource metrics.

### Pipeline Architecture

```
Monitoring Sources -> Adapter -> Ring Buffer -> Percentile Tracker
                                                     |
                                                 EWMA Smoother
                                                     |
                                                 Cost Model Update
```

### Components

**Ring Buffer** (`crates/ra-stats/src/ring_buffer.rs:16`): Fixed-capacity
(default 4096) lock-free ring buffer storing `f64` samples. O(1) push with
no allocations in the hot path.

**EWMA Smoother** (`crates/ra-stats/src/smoother.rs:20`): Exponentially
weighted moving average that filters noise from streaming metrics:

```
smoothed = alpha * new_value + (1 - alpha) * smoothed
```

Default alpha = 0.1. The `from_half_life(n)` constructor computes alpha
so an old observation decays to 50% weight after n samples:

```
alpha = 1 - 0.5^(1/half_life)
```

**Percentile Tracker** (`crates/ra-stats/src/percentiles.rs:26`): T-digest
algorithm for streaming approximate quantile estimation. Compression
parameter = 100 provides ~1% accuracy at distribution tails (p1, p99).

### Metric Channels

The pipeline manages four standard channels plus custom channels
(`crates/ra-stats/src/streaming.rs:62`):

| Channel | MetricKind | Threshold | Description |
|---------|-----------|-----------|-------------|
| CPU | `Cpu` | 10% change | CPU utilization percentage |
| Memory | `Memory` | 15% change | Memory usage |
| I/O | `Io` | 20% change | I/O operations or latency |
| Latency | `Latency` | -- | Query latency (ms) |

### Change Detection

`ChangeThresholds` (`crates/ra-stats/src/streaming.rs:41`) define fractional
change thresholds. The pipeline produces a `CostModelUpdate` only when a
metric's smoothed value shifts by more than its threshold:

```
exceeds_threshold(old, new, threshold) =
    if |old| < epsilon: |new| > epsilon
    else: |(new - old) / old| > threshold
```

(`crates/ra-stats/src/streaming.rs:378`)

A minimum update interval of 100ms prevents excessive recomputation
(`crates/ra-stats/src/streaming.rs:37`).

### CostModelUpdate

When thresholds are exceeded, the pipeline emits a snapshot
(`crates/ra-stats/src/streaming.rs:125`):

```rust
pub struct CostModelUpdate {
    pub cpu: f64,      // smoothed CPU metric
    pub memory: f64,   // smoothed memory metric
    pub io: f64,       // smoothed I/O metric
    pub latency: f64,  // smoothed query latency
    pub timestamp: Instant,
}
```

This snapshot can be used to dynamically adjust cost model parameters --
for example, increasing I/O cost weights when storage latency spikes, or
reducing parallel efficiency estimates during high CPU contention.

### Usage Example

```rust
use ra_stats::streaming::{StreamingPipeline, MetricKind, ChangeThresholds};

let mut pipeline = StreamingPipeline::new()
    .with_thresholds(ChangeThresholds {
        cpu: 0.05,    // 5% CPU change triggers update
        memory: 0.10, // 10% memory change
        io: 0.15,     // 15% I/O change
    });

// Ingest metrics from monitoring system
pipeline.ingest(MetricKind::Cpu, 45.0);
pipeline.ingest(MetricKind::Memory, 8192.0);
pipeline.ingest(MetricKind::Io, 150.0);
pipeline.ingest(MetricKind::Latency, 2.5);

// Check for cost model update
if let Some(update) = pipeline.maybe_update() {
    // Adjust cost model parameters based on update.cpu, update.io, etc.
}

// Force an update regardless of thresholds
let snapshot = pipeline.force_update();
```

### Monitoring Adapters

The pipeline supports pluggable monitoring adapters
(`crates/ra-stats/src/adapters/mod.rs`) that export metrics to external
systems:

- `OtelAdapter` -- OpenTelemetry export
- `PrometheusAdapter` -- Prometheus metrics
- `StatsdAdapter` -- StatsD protocol

---

## Statistics Configuration Profiles

`StatisticsProfile` (`crates/ra-stats/src/profiles.rs:12`) provides
pre-configured profiles for different workload patterns:

| Profile | Method | Refresh Trigger | Confidence | Sketches |
|---------|--------|-----------------|------------|----------|
| RealTime | Incremental | 1K mods OR 5min | 0.95 | No |
| Standard | BlockSample(10%) | 100K mods OR 1hr | 0.80 | Yes |
| Lazy | BlockSample(5%) | 1M mods OR 24hr | 0.60 | Yes |
| Stale | Sketch | 7 days | 0.30 | Yes |
| Analytical | FullScan | 10M mods OR 12hr | 0.90 | No |
| Streaming | Sketch | 10K mods | 0.70 | Yes |

### Profile Selection

`ProfileSelector` (`crates/ra-stats/src/profiles.rs:196`) recommends
profiles based on workload characteristics:

```
write_ratio > 0.5 AND latency_sensitivity > 0.8  =>  RealTime
write_ratio > 0.3                                  =>  Standard
table_size > 100M rows                             =>  Analytical
write_ratio < 0.01                                 =>  Lazy
else                                               =>  Standard
```

### Refresh Thresholds

`RefreshThreshold` (`crates/ra-stats/src/accuracy.rs:152`) supports
composable conditions:

```rust
// Refresh when either condition is met
RefreshThreshold::Any(vec![
    RefreshThreshold::Modifications(100_000),
    RefreshThreshold::Age(3600),
])

// Refresh only when both conditions are met
RefreshThreshold::All(vec![
    RefreshThreshold::Confidence(0.5),
    RefreshThreshold::Staleness(Staleness::ModeratelyStale),
])
```

---

## Statistics Gathering Cost Model

Before gathering statistics, the system estimates the resource cost
(`crates/ra-stats/src/gathering_cost.rs:84`):

| Method | CPU Cost | I/O | Memory | Interference |
|--------|----------|-----|--------|--------------|
| FullScan | rows * cpu_per_row | all pages | 10 pages | 0.8 (high) |
| BlockSample(n%) | rows*n% * cpu_per_row | n% pages | 10 pages | 0.3 (low) |
| RowSample(n%) | rows*n% * cpu_per_row | all pages | 10 pages | 0.6 (medium) |
| IndexScan | rows * 0.5 * cpu_per_row | rows/rpp pages | 5 pages | 0.4 |
| Incremental | rows/100 * cpu_per_row | modified pages | 2 pages | 0.1 (minimal) |
| Sketch | rows * 0.5us | 0 | 1 MB | 0.05 (negligible) |

Default estimator parameters (`crates/ra-stats/src/gathering_cost.rs:99`):

```
cpu_cost_per_row  = 1.0 us
io_cost_per_page  = 10.0 ms
page_size         = 8192 bytes
rows_per_page     = 100
buffer_hit_ratio  = 0.9 (90% of pages served from buffer pool)
```

Effective I/O cost accounts for buffer pool hits:

```
effective_io = pages * (1 - buffer_hit_ratio)
io_time      = effective_io * io_cost_per_page
```

---

## Egg Integration (IntegratedCostFn)

`IntegratedCostFn` (`crates/ra-engine/src/cost.rs:892`) implements
`egg::CostFunction<RelLang>` to drive the equality saturation optimizer's
plan extraction. For each e-node, it computes a scalar cost incorporating
hardware factors:

```
Operator          Base Cost    Hardware Factor
--------          ---------    ---------------
Scan              100.0        * (100 / storage_bw)
ScanAlias         100.0        * (100 / storage_bw)
Filter/Project    1.0          * (256 / simd_width)
Join              500.0        * (16 / cache_mb)
Aggregate         200.0        * (16 / cache_mb)
Sort              150.0        * max(8 / cores, 0.5)
IncrementalSort   60.0         * max(8 / cores, 0.5)
Limit             0.5 + n + child * 0.3   (startup opt.)
RecursiveCTE      1000.0       * (16 / cache_mb)
BitmapIndexScan   10.0         (fixed)
BitmapAnd/Or      0.1          (fixed)
BitmapHeapScan    5.0          * (100 / storage_bw)
MetadataLookup    1.0          (fixed, O(1))
IndexOnlyScan     30.0         * (100 / storage_bw)
```

The total cost of a plan is the sum of each operator's cost plus the cost
of its children, computed bottom-up through the e-graph.

### LIMIT Optimization

When a `Limit` node is present, the cost function applies a startup
fraction of 0.3 (`crates/ra-engine/src/cost.rs:1021`), meaning the
optimizer pays only ~30% of the child plan's cost. This biases extraction
toward plans with low startup cost (index scans, streaming operators) over
plans with high startup cost (full sorts, hash join builds).

---

## Calibration Methodology (RFC 0026)

The adaptive cost calibration system operates at three tiers:

### Tier 1: Static Hardware Benchmarks

On first run, micro-benchmarks measure actual I/O, CPU, and memory costs.
Results are stored as a `CostCalibration` derived from `HardwareProfile`.

### Tier 2: Dynamic Execution Feedback

After each query, estimated costs are compared to actual execution metrics.
The `CardinalityErrorTracker` accumulates q-error observations across
queries, identifying systematic patterns per table and operator type.

### Tier 3: Adaptive Correction Factors

When systematic bias is detected (e.g., hash join cost consistently
underestimated), correction factors are applied automatically via the
confidence mechanism. High q-errors reduce table confidence, inflating
future cost estimates for that table until statistics are refreshed.

### Cost Model Extensions (from RFC 0026)

- **Correlation-aware index cost**:
  `cost = random_cost * (1 - corr^2) + seq_cost * corr^2`
- **Cache-aware random I/O**: Reduce effective `random_page_cost` based on
  working set size vs available cache
- **Memory spill threshold**: When hash table exceeds memory budget, add 2x
  I/O cost for spill-to-disk

---

## Validation and Tuning

### Monitoring Estimation Accuracy

Use the `CardinalityErrorTracker` to continuously monitor estimation quality:

```rust
use ra_stats::feedback::{CardinalityErrorTracker, RecommendationEngine};

let mut tracker = CardinalityErrorTracker::new();

// After query execution, record estimated vs actual
tracker.record("orders", OperatorKind::Scan, 1000.0, 5000.0, None);
tracker.record("users", OperatorKind::Join, 500.0, 450.0, None);

// Aggregate metrics
println!("Mean q-error: {:.2}", tracker.mean_q_error());
println!("Median q-error: {:.2}", tracker.median_q_error());
println!("Max q-error: {:.2}", tracker.max_q_error());

// Per-table analysis
for (table, avg_q) in tracker.worst_tables(5) {
    println!("  {}: avg q-error = {:.1}", table, avg_q);
}

// Actionable recommendations
let engine = RecommendationEngine::new();
for rec in engine.recommend(&tracker) {
    println!("[{}] {}: {}", rec.severity, rec.kind, rec.message);
    println!("  Suggested: {}", rec.suggestion);
}
```

### Tuning Cost Model Parameters

1. **Adjust staleness inflation**: If the optimizer is too conservative
   with stale statistics, reduce the inflation factors in `staleness_factor`.

2. **Tune hardware calibration**: Compare `CostCalibration::from_hardware`
   factors against actual benchmark results. Adjust reference machine
   parameters if the baseline doesn't match your deployment.

3. **Modify pruning threshold**: The default 1.5x prunes plans more than
   50% worse than the best. Lower values (e.g., 1.2) prune more
   aggressively for faster optimization at the risk of missing good plans.
   Higher values (e.g., 2.0) explore more but take longer.

4. **Configure streaming thresholds**: Adjust `ChangeThresholds` based on
   workload volatility. OLTP workloads benefit from lower thresholds (more
   frequent updates); OLAP workloads can tolerate higher thresholds.

5. **Select statistics profile**: Use `ProfileSelector` to match the
   statistics gathering strategy to the workload pattern, or manually
   configure a `StatisticsProfile` with custom refresh thresholds and
   gathering methods.

### Regression Detection Workflow

1. Collect q-error observations during normal operation
2. Track per-table mean and median q-errors over time
3. Alert when mean q-error exceeds 5.0 for any table
4. Use `RecommendationEngine` to determine corrective action
5. Apply `IntegratedCostModel::apply_execution_feedback` to reduce
   confidence for affected tables
6. Re-ANALYZE tables with persistently high q-errors

---

## File Reference

| File | Description |
|------|-------------|
| `crates/ra-engine/src/cost.rs` | IntegratedCostModel, IntegratedCostFn, CostCalibration |
| `crates/ra-engine/src/cost_pruning.rs` | CostPruner for branch-and-bound |
| `crates/ra-hardware/src/profile.rs` | HardwareProfile struct |
| `crates/ra-hardware/src/cost.rs` | HardwareCostModel (CPU/GPU/FPGA) |
| `crates/ra-stats/src/types.rs` | TableStats, ColumnStats, IndexStats, Histograms |
| `crates/ra-stats/src/accuracy.rs` | Staleness, StatisticsState, QualityMetrics |
| `crates/ra-stats/src/integration.rs` | ManagedTableStats, StatisticsAdapter |
| `crates/ra-stats/src/feedback.rs` | Q-error, CardinalityErrorTracker, RecommendationEngine |
| `crates/ra-stats/src/streaming.rs` | StreamingPipeline, CostModelUpdate |
| `crates/ra-stats/src/smoother.rs` | EWMA smoother |
| `crates/ra-stats/src/ring_buffer.rs` | Lock-free ring buffer |
| `crates/ra-stats/src/percentiles.rs` | T-digest percentile tracker |
| `crates/ra-stats/src/profiles.rs` | StatisticsProfile, ProfileSelector |
| `crates/ra-stats/src/gathering_cost.rs` | CostEstimator, GatheringCost |
| `rfcs/0005-hardware-aware-optimization.md` | Hardware-aware optimization RFC |
| `rfcs/0026-adaptive-cost-calibration.md` | Adaptive cost calibration RFC |
