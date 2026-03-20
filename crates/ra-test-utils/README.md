# ra-test-utils

Hardware-adaptive test calibration utilities for the RA optimizer test suite.

## Overview

This crate provides test profiles that automatically scale timing expectations, iteration limits, and resource constraints based on the underlying hardware capabilities. Tests use calibrated profiles to avoid false failures on slower hardware while still catching performance regressions.

## Features

- **Automatic calibration** - Benchmark the platform and create a profile
- **Scaled expectations** - Adjust timeouts and limits based on hardware speed
- **Baseline comparison** - All platforms scale relative to AWS c7i.xlarge
- **Profile caching** - Calibrate once, use for entire test suite

## Quick Start

### 1. Calibrate your platform

Run calibration to create `.ra-test-profile.toml`:

```bash
cargo test --test calibrate calibrate_platform -- --ignored --nocapture
```

This takes about 30 seconds and benchmarks:
- Integer operations per millisecond
- Memory bandwidth
- Simple query optimization (2-table join)
- Complex query optimization (4-table join)
- E-graph saturation iterations

### 2. Use in tests

```rust
use ra_test_utils::TestProfile;
use std::time::Instant;

#[test]
fn test_optimization_is_fast() {
    let profile = TestProfile::current();
    let expected_ms = profile.scale_time_ms(1000.0);

    let start = Instant::now();
    do_expensive_optimization();
    let duration = start.elapsed();

    assert!(
        duration.as_millis() < expected_ms as u128,
        "optimization took {}ms (expected < {:.0}ms on this platform)",
        duration.as_millis(),
        expected_ms
    );
}
```

## API

### `TestProfile::current()`

Load the current platform's test profile from `.ra-test-profile.toml`. Falls back to baseline profile if file doesn't exist.

### `TestProfile::scale_time_ms(baseline_ms)`

Scale a time expectation in milliseconds. For example, if baseline expects 1000ms and the current platform is 2x slower, returns 2000.0.

### `TestProfile::scale_iterations(baseline)`

Scale an iteration count. If baseline expects 50 iterations and current platform is 1.5x slower, returns 75.

### `TestProfile::scale_memory(baseline_bytes)`

Scale a memory limit in bytes based on available system memory.

## Profile Format

The `.ra-test-profile.toml` file contains:

```toml
[platform]
id = "Linux-x86_64-Intel-Xeon"
timestamp = "2026-03-20T18:00:00Z"
os = "Linux 6.1.0"
arch = "x86_64"
cpu_model = "Intel Xeon Sapphire Rapids"
cpu_cores = 4
total_memory_gb = 8

[calibration]
simple_optimization_ms = 2.5
complex_optimization_ms = 6.4
egraph_saturation_iters = 50
int_ops_per_ms = 1200000
memory_bandwidth_mbps = 6400

[scale_factors]
time_scale = 1.0        # 1.0 = baseline speed
iteration_scale = 1.0   # 1.0 = baseline iterations
memory_scale = 1.0      # 1.0 = baseline memory
```

## Baseline Platform

The baseline platform is AWS c7i.xlarge (4 vCPU, 8GB RAM, Intel Xeon Sapphire Rapids). All other platforms scale relative to this baseline.

Example scale factors for common platforms:
- **M1 MacBook Pro**: time_scale = 0.8 (20% faster)
- **GitHub Actions**: time_scale = 1.2 (20% slower)
- **Raspberry Pi 4**: time_scale = 4.0 (4x slower)
- **RISC-V OrangePi**: time_scale = 8.0 (8x slower)

## CI Integration

Add calibration to your GitHub Actions workflow:

```yaml
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

## Migration Guide

To migrate existing tests:

1. Add dependency: `ra-test-utils = { workspace = true }`
2. Import: `use ra_test_utils::TestProfile;`
3. Replace hard-coded limits:

```rust
// Before
assert!(duration.as_millis() < 1000);

// After
let profile = TestProfile::current();
assert!(duration.as_millis() < profile.scale_time_ms(1000.0) as u128);
```

## Troubleshooting

### Profile not found

If `.ra-test-profile.toml` doesn't exist, tests use the baseline profile. This may cause failures on slower hardware. Run calibration to create a profile.

### Calibration fails

If calibration fails, check:
- ra-engine and ra-core are built
- At least 1GB free memory
- No other CPU-intensive processes running

### Tests still failing

If tests fail even after calibration:
- Check if profile is stale (> 90 days old)
- Verify hardware hasn't changed (CPU upgrade, virtualization)
- Consider increasing scale factor caps in extreme cases

## License

Same as the RA project: MIT OR Apache-2.0