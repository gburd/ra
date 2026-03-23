# RFC 0016: Hardware-Adaptive Test Expectations

- Start Date: 2026-03-20
- Author: Greg Burd
- Status: Implemented
- Tracking Issue: N/A

## Summary

Replace hard-coded timing, iteration, and resource limits in tests with platform-aware expectations that scale based on hardware capabilities. Tests calibrate against a baseline platform and adjust thresholds automatically for slower/faster hardware.

## Motivation

### Current Problem

Tests currently use hard-coded expectations:
```rust
assert!(duration.as_millis() < 1000, "optimization took too long");
assert!(iterations <= 50, "saturation took too many iterations");
```

These fail on slower platforms (RISC-V OrangePi, SPARC UltraSPARC, ARM Raspberry Pi) not because the code is broken, but because the hardware is slower. This creates:
- **False negatives** - Tests fail on valid implementations
- **CI friction** - Different runners need different thresholds
- **Maintenance burden** - Constant threshold adjustments
- **Poor portability** - Can't test on diverse hardware

### Why This Matters

1. **Multi-architecture support** - RA targets x86_64, ARM, RISC-V, potentially SPARC
2. **CI diversity** - GitHub Actions, self-hosted runners, cloud instances vary widely
3. **Development machines** - Contributors use different hardware (M1 Mac, Intel workstation, ARM server)
4. **Embedded targets** - Future edge deployment (e.g., RA in embedded databases)

### Prior Art

- **Criterion.rs**: Saves per-machine baselines, compares against them
- **RocksDB db_bench**: Calibration tool with environment-specific profiles
- **rustc-perf**: Performance tracking across machines with normalization
- **Go testing**: `testing.Short()` flag for slow systems

## Guide-level explanation

### For Test Authors

Instead of hard-coding limits:
```rust
// OLD: Fails on slow hardware
assert!(duration.as_millis() < 1000);

// NEW: Scales to platform
let profile = TestProfile::current();
let expected = profile.scale_time_ms(1000.0);
assert!(duration.as_millis() < expected as u128);
```

For iteration limits:
```rust
// OLD: Arbitrary limit
with_iter_limit(50)

// NEW: Platform-aware
let profile = TestProfile::current();
with_iter_limit(profile.scale_iterations(50))
```

### For Test Runners

First run (or after hardware change):
```bash
cargo test --calibrate
# Creates .ra-test-profile.toml with measurements

cargo test --workspace
# Tests use calibrated expectations
```

CI integration:
```yaml
- name: Calibrate test environment
  run: cargo test --calibrate

- name: Run tests
  run: cargo test --workspace
```

### For Users

Tests "just work" on any platform. Slower machines get appropriate timeouts automatically. Performance regressions still caught (2x slowdown on same hardware fails tests).

## Reference-level explanation

### Architecture

```
,---------------------------------------------------,
| Test Suite                                      |
|  - integration_optimizer_test.rs                |
|  - proptest_optimization.rs                     |
|  - execution_*_test.rs                          |
`---------------------+--------------------------------'
                   | uses
                   v
,---------------------------------------------------,
| TestProfile::current()                          |
|  - Loads .ra-test-profile.toml                  |
|  - Falls back to baseline if missing            |
|  - Caches in memory for test run               |
`---------------------+--------------------------------'
                   | reads
                   v
,---------------------------------------------------,
| .ra-test-profile.toml                           |
|  [platform]                                     |
|  [calibration]                                  |
|  [scale_factors]                                |
`----------------------------------------------------'
                   ^
                   | written by
,---------------------------------------------------,
| cargo test --calibrate                          |
|  - Runs micro-benchmarks                        |
|  - Measures optimizer performance               |
|  - Compares to baseline                         |
|  - Writes profile                               |
`----------------------------------------------------'
```

### Data Structures

```rust
// crates/ra-test-utils/src/profile.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestProfile {
    pub platform: PlatformInfo,
    pub calibration: CalibrationResults,
    pub scale_factors: ScaleFactors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub os: String,
    pub arch: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub total_memory_gb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationResults {
    /// Time to optimize simple query (ms)
    pub simple_optimization_ms: f64,
    /// Time to optimize complex query (ms)
    pub complex_optimization_ms: f64,
    /// E-graph saturation iterations for depth-2 expr
    pub egraph_saturation_iters: u64,
    /// Integer operations per millisecond
    pub int_ops_per_ms: u64,
    /// Memory bandwidth (MB/s)
    pub memory_bandwidth_mbps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleFactors {
    /// Time scale relative to baseline (1.0 = same speed)
    pub time_scale: f64,
    /// Iteration scale relative to baseline
    pub iteration_scale: f64,
    /// Memory scale relative to baseline
    pub memory_scale: f64,
}

impl TestProfile {
    /// Load from .ra-test-profile.toml or return baseline
    pub fn current() -> &'static Self;

    /// Scale a time expectation (ms)
    pub fn scale_time_ms(&self, baseline_ms: f64) -> f64;

    /// Scale an iteration count
    pub fn scale_iterations(&self, baseline: usize) -> usize;

    /// Scale a memory limit (bytes)
    pub fn scale_memory(&self, baseline_bytes: u64) -> u64;

    /// Baseline profile (AWS c7i.xlarge, 4 vCPU, 8GB RAM)
    pub fn baseline() -> Self;
}
```

### Calibration Benchmarks

```rust
// crates/ra-test-utils/src/calibrate.rs

pub fn calibrate() -> Result<TestProfile> {
    println!("Calibrating test expectations...");

    // 1. Detect hardware
    let hw = ra_hardware::detect_hardware();
    let platform = PlatformInfo::from_hardware(&hw);

    // 2. Run micro-benchmarks (30 seconds total)
    print!("  Integer ops... ");
    let int_ops = benchmark_int_ops(Duration::from_secs(5));
    println!("{} ops/ms", int_ops);

    print!("  Memory bandwidth... ");
    let mem_bw = benchmark_memory_bandwidth(Duration::from_secs(5));
    println!("{} MB/s", mem_bw);

    // 3. Run optimizer benchmarks (60 seconds total)
    print!("  Simple optimization... ");
    let simple_opt = benchmark_simple_optimization(10);
    println!("{:.2}ms", simple_opt);

    print!("  Complex optimization... ");
    let complex_opt = benchmark_complex_optimization(5);
    println!("{:.2}ms", complex_opt);

    print!("  E-graph saturation... ");
    let saturation = benchmark_egraph_saturation(10);
    println!("{} iterations", saturation);

    // 4. Calculate scale factors
    let baseline = TestProfile::baseline();
    let scale_factors = ScaleFactors {
        time_scale: simple_opt / baseline.calibration.simple_optimization_ms,
        iteration_scale: saturation as f64
            / baseline.calibration.egraph_saturation_iters as f64,
        memory_scale: hw.available_memory as f64
            / baseline.platform.total_memory_gb as f64,
    };

    let profile = TestProfile {
        platform,
        calibration: CalibrationResults {
            simple_optimization_ms: simple_opt,
            complex_optimization_ms: complex_opt,
            egraph_saturation_iters: saturation,
            int_ops_per_ms: int_ops,
            memory_bandwidth_mbps: mem_bw,
        },
        scale_factors,
    };

    // 5. Write to .ra-test-profile.toml
    let toml = toml::to_string_pretty(&profile)?;
    std::fs::write(".ra-test-profile.toml", toml)?;

    println!("\nCalibration complete!");
    println!("  Time scale: {:.2}x", scale_factors.time_scale);
    println!("  Iteration scale: {:.2}x", scale_factors.iteration_scale);

    Ok(profile)
}

fn benchmark_simple_optimization(iterations: usize) -> f64 {
    use ra_core::algebra::*;
    use ra_engine::Optimizer;

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(scan("table1")),
        right: Box::new(scan("table2")),
    };

    let optimizer = Optimizer::default();

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = optimizer.optimize(&plan);
    }
    let elapsed = start.elapsed();

    elapsed.as_secs_f64() * 1000.0 / iterations as f64
}

// Similar for benchmark_complex_optimization, benchmark_egraph_saturation...
```

### Test Integration

```rust
// crates/ra-engine/tests/integration_optimizer_test.rs

use ra_test_utils::TestProfile;

#[test]
fn test_optimization_is_fast() {
    let profile = TestProfile::current();
    let expected_ms = profile.scale_time_ms(1000.0);

    let plan = two_table_join("orders", "customers", "customer_id", "id");
    let optimizer = create_test_optimizer();

    let start = Instant::now();
    let _result = optimizer.optimize(&plan).expect("should optimize");
    let duration = start.elapsed();

    assert!(
        duration.as_millis() < expected_ms as u128,
        "optimization took {}ms (expected < {:.0}ms on this platform, scale={:.2}x)",
        duration.as_millis(),
        expected_ms,
        profile.scale_factors.time_scale
    );
}

#[test]
fn test_complex_query_optimizes_reasonably() {
    let profile = TestProfile::current();
    let expected_ms = profile.scale_time_ms(5000.0);

    let j1 = two_table_join("table1", "table2", "id", "id");
    let j2 = two_table_join("table3", "table4", "id", "id");
    let j3 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(j2),
    };

    let optimizer = create_test_optimizer();
    let start = Instant::now();
    let _result = optimizer.optimize(&j3).expect("should optimize");
    let duration = start.elapsed();

    assert!(
        duration.as_millis() < expected_ms as u128,
        "complex optimization took {}ms (expected < {:.0}ms on this platform)",
        duration.as_millis(),
        expected_ms
    );
}
```

```rust
// crates/ra-engine/tests/proptest_optimization.rs

fn saturation_terminates_quickly(expr in arb_rel_expr(2)) {
    let profile = TestProfile::current();
    let max_iters = profile.scale_iterations(50);

    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let runner: Runner<RelLang, RelAnalysis> = Runner::default()
        .with_expr(&rec)
        .with_node_limit(10_000)
        .with_iter_limit((max_iters * 2).min(200))  // Cap at 200
        .run(&all_rules());

    let iteration_count = runner.iterations.len();
    prop_assert!(
        iteration_count <= max_iters,
        "Saturation took {} iterations (expected <= {} on this platform, scale={:.2}x)\n\
         Expression: {:?}",
        iteration_count,
        max_iters,
        profile.scale_factors.iteration_scale,
        expr
    );
}
```

### Baseline Definition

Baseline is **AWS c7i.xlarge** (Intel Xeon Sapphire Rapids, 4 vCPU, 8GB RAM):
- Simple optimization: 2.5ms
- Complex optimization: 6.4ms
- E-graph saturation: 50 iterations (depth-2 expr)
- Integer ops: 1,200,000 ops/ms
- Memory bandwidth: 6400 MB/s

Other platforms scale relative to this. Examples:
- **M1 MacBook Pro**: time_scale = 0.8 (20% faster)
- **Raspberry Pi 4**: time_scale = 4.0 (4x slower)
- **OrangePi RISC-V**: time_scale = 8.0 (8x slower)
- **GitHub Actions (ubuntu-latest)**: time_scale = 1.2 (20% slower)

### File Format (.ra-test-profile.toml)

```toml
[platform]
id = "orangepi-riscv64-allwinner-d1"
timestamp = "2026-03-20T18:00:00Z"
os = "Linux 6.1.0"
arch = "riscv64"
cpu_model = "Allwinner D1 (C906)"
cpu_cores = 1
total_memory_gb = 1

[calibration]
simple_optimization_ms = 20.3
complex_optimization_ms = 51.2
egraph_saturation_iters = 425
int_ops_per_ms = 150_000
memory_bandwidth_mbps = 800

[scale_factors]
time_scale = 8.12
iteration_scale = 8.5
memory_scale = 0.125
```

### Cargo Integration

```rust
// tests/calibrate.rs

#[test]
#[ignore]
fn calibrate_platform() {
    use ra_test_utils::calibrate;

    let profile = calibrate().expect("calibration should succeed");
    println!("\n{:#?}", profile);
}
```

Run with:
```bash
cargo test --test calibrate calibrate_platform -- --ignored --nocapture
# Or shorthand:
cargo test --calibrate  # Via custom test harness
```

### CI Integration

```yaml
# .github/workflows/test.yml

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]

    steps:
      - uses: actions/checkout@v4

      - name: Restore test profile cache
        uses: actions/cache@v4
        with:
          path: .ra-test-profile.toml
          key: test-profile-${{ runner.os }}-${{ runner.arch }}-v1

      - name: Calibrate if needed
        run: |
          if [ ! -f .ra-test-profile.toml ]; then
            cargo test --calibrate
          fi

      - name: Run tests
        run: cargo test --workspace
```

### Backward Compatibility

Tests without `TestProfile::current()` continue to work with hard-coded limits. Migration is gradual:
1. Add `ra-test-utils` dependency
2. Replace hard-coded limits one test at a time
3. Eventually deprecate hard-coded timing tests

### Edge Cases

**No profile file + calibration fails:**
- Fall back to baseline profile
- Warn user: "Using baseline expectations (may fail on slow hardware)"

**Calibration takes too long:**
- Allow `--calibrate-quick` (30s instead of 90s, lower precision)
- Use saved profiles in CI (cache by runner.os + runner.arch)

**Profile too old:**
- Check timestamp, warn if > 90 days old
- User runs `cargo test --recalibrate`

**Hardware changes:**
- Profile mismatch detected (cores changed, memory changed)
- Auto-recalibrate or warn user

## Drawbacks

1. **Complexity** - Adds calibration step, profile management
2. **Calibration time** - 60-90 seconds per platform
3. **CI cache dependency** - Need to cache profiles or recalibrate
4. **False positives possible** - If scale_factor is too generous, real regressions might pass
5. **Profile drift** - Hardware upgrades invalidate profiles

## Rationale and alternatives

### Why this design?

- **Criterion-like** - Proven approach in Rust ecosystem
- **TOML config** - Human-readable, version-controllable
- **Gradual migration** - Old tests keep working
- **Leverage existing work** - Uses `ra-hardware` detection

### Alternative: Environment variables

```rust
let timeout = std::env::var("RA_TEST_TIMEOUT_MS")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(1000);
```

**Rejected because:**
- Requires manual tuning per platform
- Not self-documenting
- Error-prone (forget to set variable)

### Alternative: Skip slow tests

```rust
#[cfg_attr(target_arch = "riscv64", ignore)]
#[test]
fn test_optimization_is_fast() { ... }
```

**Rejected because:**
- Loses test coverage on slow platforms
- Manual arch annotations needed
- Doesn't solve CI runner variance

### Alternative: Increase all limits 10x

**Rejected because:**
- Masks real regressions on fast hardware
- Tests take longer
- Arbitrary multiplier

## Prior art

- **Criterion.rs**: Baseline comparison, per-machine profiles
- **rustc-perf**: Normalizes across machines, tracks perf history
- **RocksDB db_bench**: Calibration tool, environment-specific tuning
- **Go `testing.Short()`**: Boolean flag for slow systems (less sophisticated)
- **Python pytest-benchmark**: Calibration, warmup, statistical analysis

## Unresolved questions

1. **Baseline platform choice** - AWS c7i.xlarge reasonable? Or use median of common platforms?
2. **Calibration frequency** - Auto-recalibrate monthly? On hardware changes only?
3. **Scale factor caps** - Max 10x? Or allow 100x for very slow hardware?
4. **Profile versioning** - What if calibration format changes? Migration path?
5. **Shared profiles** - Commit common profiles to repo (e.g., `profiles/github-ubuntu-latest.toml`)?

## Future possibilities

1. **Adaptive test selection** - Skip expensive tests on slow hardware
2. **Performance tracking** - Store profiles over time, detect hardware degradation
3. **Benchmark dashboard** - Visualize performance across platforms
4. **Automatic baseline updates** - Median of last 100 CI runs becomes new baseline
5. **Cloud cost optimization** - Choose cheapest CI runner that meets test requirements
6. **Integration with ra-hardware cost model** - Feed real hardware measurements into optimizer

## Implementation plan

### Phase 1: MVP (1 week)
1. Create `ra-test-utils` crate
2. Implement `TestProfile` struct with TOML serde
3. Add `calibrate()` with 3 benchmarks (simple opt, complex opt, saturation)
4. Update 3 timing tests to use `TestProfile::current()`
5. Documentation in `docs/testing.md`

### Phase 2: Full Integration (1 week)
1. Update all timing tests (~15 files)
2. Update all iteration limit tests (~5 files)
3. CI integration (GitHub Actions, cache profiles)
4. Add `--calibrate` flag to test harness

### Phase 3: Refinement (ongoing)
1. Collect profiles from diverse hardware
2. Tune scale factor algorithms
3. Add performance tracking
4. Dashboard for multi-platform results
