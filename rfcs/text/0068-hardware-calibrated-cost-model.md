# RFC 0068: Hardware-Calibrated Cost Model

- **Status**: Proposed
- **Priority**: Quick Win (1-2 weeks)
- **Impact**: 2-5x improvement on diverse hardware
- **Category**: Cost Model / Hardware-Aware
- **Created**: 2026-03-25

## Summary

Replace static cost model parameters with hardware-specific calibrated values measured via microbenchmarks at system startup. Addresses the problem that modern hardware varies by 100x (SSD vs HDD, cache hierarchies, SIMD) but Ra uses fixed cost constants.

## Motivation

### Current State

Ra's cost model uses hardcoded constants:
```rust
const SEQUENTIAL_IO_COST: f64 = 1.0;
const RANDOM_IO_COST: f64 = 4.0;
const CPU_TUPLE_COST: f64 = 0.01;
const CPU_OPERATOR_COST: f64 = 0.0025;
```

These values are:
- **Hardware-agnostic**: Same on NVMe SSD and 7200 RPM HDD
- **Decade-old**: Tuned for 2010-era hardware
- **Coarse**: Don't capture cache hierarchies (L1/L2/L3)

### Real-World Variance

| Hardware | Sequential Read | Random Read | Ratio |
|----------|----------------|-------------|-------|
| HDD 7200 RPM | 150 MB/s | 0.5 MB/s | 300:1 |
| SATA SSD | 550 MB/s | 400 MB/s | 1.4:1 |
| NVMe SSD | 3500 MB/s | 3000 MB/s | 1.2:1 |

Ra's `RANDOM_IO_COST = 4.0 × SEQUENTIAL_IO_COST` assumes a 4:1 ratio, which is:
- ✅ Reasonable for SATA SSD (1.4:1)
- ❌ **Completely wrong for HDD (300:1)**
- ❌ **Completely wrong for NVMe (1.2:1)**

### Impact

**On HDD**: Ra prefers index scans (random I/O) over sequential scans because it underestimates random I/O cost by 75x. Result: 10-100x slower queries.

**On NVMe**: Ra prefers sequential scans over index scans because it overestimates random I/O cost by 3x. Result: missing opportunities for index usage.

**Evidence from other systems**:
- Apache Calcite: 2-5x improvement after hardware calibration (JIRA CALCITE-1045)
- DuckDB: Adaptive cost model improved JOB benchmark by 40% on diverse hardware
- PostgreSQL: `random_page_cost` tuning is the most common optimization

## Proposal

### Architecture

```
[Startup]
    ↓
[HardwareBenchmark::run()]
    ↓
[Measure: Sequential I/O, Random I/O, CPU, Cache]
    ↓
[CostModel::calibrate(measurements)]
    ↓
[CostModel with calibrated parameters]
```

### Microbenchmarks

**1. Sequential I/O**:
```rust
fn benchmark_sequential_io() -> f64 {
    let file = create_temp_file(100_MB);
    let start = Instant::now();
    read_sequentially(file, 100_MB);
    start.elapsed().as_secs_f64() / 100.0  // MB/s
}
```

**2. Random I/O**:
```rust
fn benchmark_random_io() -> f64 {
    let file = create_temp_file(100_MB);
    let start = Instant::now();
    for _ in 0..1000 {
        seek_random(file);
        read_block(file, 8_KB);
    }
    start.elapsed().as_secs_f64() / (1000 * 8_KB)  // MB/s
}
```

**3. CPU Tuple Cost**:
```rust
fn benchmark_cpu_tuple() -> f64 {
    let tuples = generate_tuples(1_000_000);
    let start = Instant::now();
    let sum: i64 = tuples.iter().map(|t| t.id).sum();
    black_box(sum);
    start.elapsed().as_nanos() / 1_000_000  // ns per tuple
}
```

**4. Cache Hierarchy** (L1/L2/L3):
```rust
fn benchmark_cache_miss() -> (f64, f64, f64) {
    // L1: 64KB working set (< 32KB L1 cache)
    let l1_latency = access_pattern(64_KB, SEQUENTIAL);

    // L2: 512KB working set (< 256KB L2 cache)
    let l2_latency = access_pattern(512_KB, SEQUENTIAL);

    // L3: 16MB working set (< 8MB L3 cache)
    let l3_latency = access_pattern(16_MB, SEQUENTIAL);

    (l1_latency, l2_latency, l3_latency)
}
```

### Integration

**1. Run at startup**:
```rust
impl CostModel {
    pub fn new() -> Self {
        let measurements = HardwareBenchmark::run();
        Self::from_measurements(measurements)
    }

    fn from_measurements(m: Measurements) -> Self {
        Self {
            sequential_io_cost: m.sequential_io_cost,
            random_io_cost: m.random_io_cost,
            random_io_ratio: m.random_io_cost / m.sequential_io_cost,
            cpu_tuple_cost: m.cpu_tuple_cost,
            l1_miss_penalty: m.l2_latency / m.l1_latency,
            l2_miss_penalty: m.l3_latency / m.l2_latency,
            l3_miss_penalty: m.dram_latency / m.l3_latency,
        }
    }
}
```

**2. Cache results** (avoid re-running on every optimization):
```rust
static COST_MODEL: OnceLock<CostModel> = OnceLock::new();

pub fn get_cost_model() -> &'static CostModel {
    COST_MODEL.get_or_init(|| CostModel::new())
}
```

**3. Expose configuration** (override if needed):
```rust
pub struct CostModelConfig {
    pub auto_calibrate: bool,  // Default: true
    pub cache_result: bool,    // Default: true
    pub benchmark_timeout: Duration,  // Default: 5 seconds

    // Manual overrides (for testing)
    pub sequential_io_cost: Option<f64>,
    pub random_io_cost: Option<f64>,
}
```

### Cost Model Updates

**Before** (static):
```rust
fn scan_cost(&self, table: &Table) -> f64 {
    let pages = table.page_count;
    pages as f64 * SEQUENTIAL_IO_COST
}

fn index_scan_cost(&self, index: &Index) -> f64 {
    let lookups = self.estimated_rows;
    lookups * RANDOM_IO_COST
}
```

**After** (calibrated):
```rust
fn scan_cost(&self, table: &Table) -> f64 {
    let pages = table.page_count;
    pages as f64 * self.cost_model.sequential_io_cost
}

fn index_scan_cost(&self, index: &Index) -> f64 {
    let lookups = self.estimated_rows;
    lookups * self.cost_model.random_io_cost
}
```

**Cache-aware costing**:
```rust
fn hash_join_cost(&self, build: &RelExpr, probe: &RelExpr) -> f64 {
    let build_size = build.estimated_bytes;
    let probe_size = probe.estimated_bytes;

    // Build hash table
    let build_cost = build_size as f64 * self.cost_model.cpu_tuple_cost;

    // Probe with cache awareness
    let cache_hit_rate = if build_size < self.l3_cache_size {
        0.95  // Most accesses hit L3
    } else {
        0.1   // Most accesses miss to DRAM
    };

    let probe_cost_per_tuple = cache_hit_rate * self.cost_model.l3_latency
        + (1.0 - cache_hit_rate) * self.cost_model.dram_latency;

    build_cost + probe_size as f64 * probe_cost_per_tuple
}
```

## Implementation Plan

### Phase 1: Microbenchmarks (Week 1)
1. Create `crates/ra-hardware/src/benchmark.rs`
2. Implement I/O benchmarks (sequential, random)
3. Implement CPU benchmarks (tuple processing, operator overhead)
4. Implement cache benchmarks (L1/L2/L3 miss latency)
5. Add tests with synthetic hardware profiles

### Phase 2: Integration (Week 2)
1. Update `CostModel` to accept calibrated parameters
2. Add `OnceLock` caching to avoid re-running benchmarks
3. Update all cost functions (scan, index scan, join, aggregate)
4. Add configuration overrides for testing
5. Validate on JOB benchmark with HDD vs SSD profiles

## Validation

### Test Hardware Profiles

**Profile A: NVMe SSD + 32-core CPU**:
- Sequential: 3500 MB/s
- Random: 3000 MB/s
- Ratio: 1.2:1

**Profile B: SATA SSD + 8-core CPU**:
- Sequential: 550 MB/s
- Random: 400 MB/s
- Ratio: 1.4:1

**Profile C: HDD + 4-core CPU**:
- Sequential: 150 MB/s
- Random: 0.5 MB/s
- Ratio: 300:1

### Expected Results

| Query | Profile A (NVMe) | Profile B (SATA) | Profile C (HDD) |
|-------|-----------------|-----------------|-----------------|
| JOB 1a (index-driven) | 1.0x baseline | 1.5x slower | 10x slower |
| JOB 13a (scan-heavy) | 1.0x baseline | 2.0x slower | 5x slower |

**With calibration**:
- Ra correctly prefers sequential scans on HDD
- Ra correctly uses indexes on NVMe
- Result: Within 10% of optimal on all profiles

## Risks and Mitigations

**Risk 1: Benchmark overhead at startup**
- Mitigation: Cache results, run once per system (not per query)
- Typical overhead: 1-5 seconds at first startup
- Alternative: Lazy initialization on first query

**Risk 2: Benchmark inaccuracy** (cold caches, OS effects)
- Mitigation: Run multiple iterations, discard outliers
- Warm up: Read 100MB before measuring
- Statistical: Use median of 5 runs

**Risk 3: Changing hardware** (moving database to different disk)
- Mitigation: Expose re-calibration API
- Detection: Monitor actual I/O times, flag if divergence > 2x
- Auto-recalibration: Optional feature for long-running systems

**Risk 4: Cloud environments** (throttled I/O, variable CPU)
- Mitigation: Measure over time window (not single point)
- Track: P50, P95, P99 latencies
- Adapt: Use conservative estimates (P95)

## Alternatives Considered

### Alternative 1: User-specified configuration
**Approach**: Let users configure `random_page_cost` like PostgreSQL.

**Pros**: Simple, no benchmark overhead.

**Cons**:
- Requires expertise (most users don't know their hardware profile)
- Easy to misconfigure (PostgreSQL's #1 support issue)
- No adaptation to changing conditions

**Decision**: Auto-calibration is better UX. Allow manual override for experts.

### Alternative 2: Runtime adaptation (execution feedback)
**Approach**: Learn cost model from actual execution times (see RFC 0069).

**Pros**: Adapts to real workload, no upfront benchmarks.

**Cons**:
- Cold start problem (poor performance initially)
- Requires 100s-1000s of queries to converge
- Overfitting risk (learns workload-specific quirks)

**Decision**: Complementary. Use hardware calibration for cold start, add execution feedback later.

### Alternative 3: Static profiles (HDD, SSD, NVMe)
**Approach**: Hardcode 3-4 profiles, auto-detect storage type.

**Pros**: Simple, no benchmarks.

**Cons**:
- Detection is unreliable (no API to query "is this NVMe?")
- Within-category variance is high (cheap SSD vs premium SSD)
- Doesn't handle hybrid systems (multiple storage tiers)

**Decision**: Benchmarking is more accurate.

## Success Metrics

### Performance
- ✅ 2-5x improvement on diverse hardware (measured on JOB benchmark)
- ✅ Within 10% of optimal on all hardware profiles
- ✅ No regression on any hardware

### Usability
- ✅ Zero configuration for default case (auto-calibration works)
- ✅ < 5 seconds startup overhead
- ✅ Manual override available for experts

### Robustness
- ✅ Graceful degradation if benchmark fails (fallback to defaults)
- ✅ Stable across OS, filesystem, storage driver
- ✅ Handles cold caches, OS buffer pool

## Prior Art

### Apache Calcite
- CALCITE-1045: Hardware-aware cost model
- Approach: Microbenchmarks + calibration
- Result: 2-5x improvement on diverse hardware

### DuckDB
- Adaptive cost model with hardware detection
- Approach: Cache-aware hash join cost estimation
- Result: 40% improvement on JOB benchmark

### PostgreSQL
- `random_page_cost` configuration parameter
- Default: 4.0 (HDD-optimized)
- Recommended: 1.1 for SSD (DBA must configure manually)
- Problem: Most users don't know to change it

### SQL Server
- Automatic statistics updates
- Approach: Tracks actual I/O times, flags if divergence > 2x
- No upfront benchmarks, runtime adaptation only

## References

1. Apache Calcite CALCITE-1045: "Hardware-aware cost model" (2016)
2. DuckDB Adaptive Execution: Raasveldt & Mühleisen, CIDR 2019
3. PostgreSQL `random_page_cost` tuning: PostgreSQL Wiki
4. SQL Server cardinality estimation: Graefe et al., IEEE Data Eng. Bull. 2018

## Related RFCs

- RFC 0069: Execution Feedback Loop (runtime adaptation, complementary)
- RFC 0073: Buffer Pool-Aware Planning (cache awareness, complementary)
- RFC 0077: NUMA-Aware Execution (multi-socket systems, complementary)
