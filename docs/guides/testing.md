# Testing Guide

This guide covers the testing infrastructure and best practices for the RA optimizer project.

## Table of Contents

- [Running Tests](#running-tests)
- [Test Organization](#test-organization)
- [Hardware-Adaptive Testing](#hardware-adaptive-testing)
- [Property-Based Testing](#property-based-testing)
- [Integration Testing](#integration-testing)
- [Benchmarking](#benchmarking)

## Running Tests

### Basic Test Commands

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p ra-engine

# Run a specific test
cargo test test_optimization_is_fast

# Run tests with output
cargo test -- --nocapture

# Run ignored tests
cargo test -- --ignored
```

### Test Categories

Tests are organized into several categories:

- **Unit tests**: In `src/` alongside the code
- **Integration tests**: In `tests/` directory
- **Property tests**: Using proptest for exhaustive testing
- **Benchmarks**: In `benches/` using criterion

## Test Organization

### Unit Tests

Unit tests live in the same file as the code they test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature() {
        // test implementation
    }
}
```

### Integration Tests

Integration tests live in the `tests/` directory and test the public API:

```rust
// tests/integration_test.rs
use ra_engine::Optimizer;

#[test]
fn test_end_to_end() {
    let optimizer = Optimizer::new();
    // test cross-component behavior
}
```

## Hardware-Adaptive Testing

The RA test suite uses hardware-adaptive test calibration to ensure tests pass reliably across different platforms while still catching performance regressions.

### Overview

Tests that verify performance characteristics (timing, iteration counts, memory usage) can fail on different hardware not because the code is broken, but because the hardware is slower or faster than expected. Hardware-adaptive testing solves this by:

1. **Calibrating** the test environment to measure its performance
2. **Scaling** test expectations based on the calibration results
3. **Comparing** against a baseline platform (AWS c7i.xlarge)

### Quick Start

#### 1. Calibrate Your Platform

Before running performance-sensitive tests, calibrate your platform:

```bash
cargo test --test calibrate calibrate_platform -- --ignored --nocapture
```

This creates `.ra-test-profile.toml` with your platform's performance profile. The calibration takes about 30 seconds and measures:

- Integer operations per millisecond
- Memory bandwidth
- Query optimization performance
- E-graph saturation characteristics

#### 2. Use Scaled Expectations in Tests

Instead of hard-coding performance expectations:

```rust
// ❌ Bad: Fails on slower hardware
assert!(duration.as_millis() < 1000);

// ✅ Good: Scales to platform performance
use ra_test_utils::TestProfile;

let profile = TestProfile::current();
let expected_ms = profile.scale_time_ms(1000.0);
assert!(duration.as_millis() < expected_ms as u128);
```

### API Reference

#### `TestProfile::current()`

Loads the current platform's profile from `.ra-test-profile.toml`. Falls back to baseline if not found.

```rust
let profile = TestProfile::current();
```

#### `scale_time_ms(baseline_ms)`

Scales a time expectation in milliseconds:

```rust
// Baseline expects 1000ms, but current platform is 2x slower
let expected = profile.scale_time_ms(1000.0); // Returns 2000.0
```

#### `scale_iterations(baseline)`

Scales an iteration count:

```rust
// Baseline expects 50 iterations, current platform needs 75
let max_iters = profile.scale_iterations(50); // Returns 75
```

#### `scale_memory(baseline_bytes)`

Scales memory limits based on available system memory:

```rust
// Scale 1GB baseline to platform's available memory
let memory_limit = profile.scale_memory(1_000_000_000);
```

### Profile Format

The `.ra-test-profile.toml` file contains platform information and scale factors:

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

### CI Integration

Add calibration to your CI workflow:

```yaml
# .github/workflows/test.yml
- name: Restore test profile cache
  uses: actions/cache@v4
  with:
    path: .ra-test-profile.toml
    key: test-profile-${{ runner.os }}-${{ runner.arch }}-v1

- name: Calibrate if needed
  run: |
    if [ ! -f .ra-test-profile.toml ]; then
      cargo test --test calibrate calibrate_platform -- --ignored --nocapture
    fi

- name: Run tests
  run: cargo test --workspace
```

### Migrating Existing Tests

To migrate tests to use hardware-adaptive expectations:

1. Add the dependency:
   ```toml
   [dev-dependencies]
   ra-test-utils = { workspace = true }
   ```

2. Update the test:
   ```rust
   use ra_test_utils::TestProfile;

   #[test]
   fn test_performance() {
       let profile = TestProfile::current();
       let expected_ms = profile.scale_time_ms(1000.0);

       // ... run operation ...

       assert!(duration.as_millis() < expected_ms as u128,
           "took {}ms (expected < {:.0}ms on this platform)",
           duration.as_millis(), expected_ms);
   }
   ```

### Platform Examples

Different platforms have different scale factors relative to the baseline:

| Platform | Time Scale | Description |
|----------|------------|-------------|
| AWS c7i.xlarge | 1.0 | Baseline (4 vCPU, 8GB RAM) |
| M1 MacBook Pro | 0.8 | 20% faster than baseline |
| GitHub Actions | 1.2 | 20% slower than baseline |
| Raspberry Pi 4 | 4.0 | 4x slower than baseline |
| RISC-V OrangePi | 8.0 | 8x slower than baseline |

### Troubleshooting

#### Tests fail even after calibration

- Check if profile is stale (> 90 days old)
- Verify hardware hasn't changed
- Re-run calibration: `cargo test --test calibrate calibrate_platform -- --ignored --nocapture`

#### Calibration fails

- Ensure ra-engine and ra-core are built
- Check for at least 1GB free memory
- Close CPU-intensive applications

#### No profile file found

The baseline profile (AWS c7i.xlarge) is used when `.ra-test-profile.toml` doesn't exist. This may cause failures on slower hardware.

## Property-Based Testing

We use proptest for property-based testing to verify algebraic properties and catch edge cases:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn join_associativity(
        a in arb_rel_expr(),
        b in arb_rel_expr(),
        c in arb_rel_expr()
    ) {
        // (a ⋈ b) ⋈ c ≡ a ⋈ (b ⋈ c)
        let left = join(join(a.clone(), b.clone()), c.clone());
        let right = join(a, join(b, c));
        assert_equivalent(left, right);
    }
}
```

## Integration Testing

Integration tests verify end-to-end behavior across components:

```rust
#[test]
fn test_sql_to_execution() {
    let sql = "SELECT * FROM users WHERE age > 21";
    let parsed = parse_sql(sql);
    let optimized = optimizer.optimize(parsed);
    let result = execute(optimized);
    assert_eq!(result.len(), expected_count);
}
```

## Benchmarking

Performance benchmarks use criterion:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_optimization(c: &mut Criterion) {
    c.bench_function("optimize_tpch_q1", |b| {
        b.iter(|| {
            optimizer.optimize(black_box(&tpch_q1))
        })
    });
}

criterion_group!(benches, bench_optimization);
criterion_main!(benches);
```

Run benchmarks:

```bash
cargo bench
cargo bench -- --save-baseline main
cargo bench -- --baseline main
```

## Best Practices

1. **Test at the right level**: Unit test individual functions, integration test workflows
2. **Use property testing**: For algebraic properties and edge cases
3. **Mock external dependencies**: Use trait objects for testability
4. **Test error conditions**: Verify error handling paths
5. **Use hardware-adaptive testing**: For performance-sensitive tests
6. **Document test purpose**: Explain what property is being verified
7. **Keep tests focused**: One logical assertion per test
8. **Use test helpers**: Extract common setup into helper functions