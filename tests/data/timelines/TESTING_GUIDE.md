# Timeline Testing Guide

Complete guide to testing the timeline-based fingerprint configuration system.

## Quick Start

### Running All Tests

```bash
# Integration tests - timeline loading and validation
cargo test --test timeline_integration_test

# Property-based tests (once generators implemented)
cargo test --package ra-engine --test timeline_property_tests

# Unit tests - helper functions
cargo test --package ra-test-utils
```

### Running Specific Timeline Test

```bash
cargo test --test timeline_integration_test test_index_addition_timeline
```

### Verbose Output

```bash
cargo test --test timeline_integration_test -- --nocapture
```

## Test Organization

### Test Levels

```
┌─────────────────────────────────────────────┐
│         Property-Based Tests               │
│    (Broad input space, invariants)         │
│  ra-engine/tests/timeline_property_tests   │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│         Integration Tests                   │
│   (Complete timelines, expectations)        │
│     tests/timeline_integration_test         │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│            Unit Tests                       │
│   (Helper functions, validation)            │
│   ra-test-utils/src/timeline_helpers        │
└─────────────────────────────────────────────┘
```

### Test Files

| File | Purpose | Status |
|------|---------|--------|
| `timeline_helpers.rs` | Helper utilities, assertions | ✅ Complete |
| `timeline_integration_test.rs` | Load/validate timelines | ✅ Complete |
| `timeline_property_tests.rs` | Property-based tests | ⚠ Structure only |

## Writing Timeline Tests

### Basic Integration Test

```rust
#[test]
fn test_my_timeline() {
    // Load timeline
    let config = load_timeline("my-scenario")
        .expect("Failed to load timeline");

    // Validate structure
    assert_eq!(config.snapshots.len(), 3);
    assert!(!config.hardware_profiles.is_empty());

    // Check expectations
    let exp = &config.expectations[0];
    assert!(exp.expected_plan_pattern.is_some());
}
```

### Testing Expectations

```rust
use ra_test_utils::timeline_helpers::*;

#[test]
fn test_timeline_expectations() {
    let config = load_timeline("index-addition").unwrap();

    // Test pattern matching
    let plan = optimize_snapshot(&config, 1);
    assert_plan_contains(&plan, ".*IndexScan.*");

    // Test cost reduction
    let cost_before = get_cost(&config, 0);
    let cost_after = get_cost(&config, 1);
    assert_cost_reduction(cost_before, cost_after, 0.80);

    // Test cardinality
    let cardinality = get_cardinality(&config, 1);
    assert_cardinality_within_tolerance(20.0, cardinality, 0.3);

    // Test rules
    let rules = get_rules_applied(&config, 1);
    assert_rules_applied(&rules, &vec!["index-scan-selection".to_string()]);
    assert_rules_not_applied(&rules, &vec!["parallel-scan".to_string()]);
}
```

### Property-Based Test Template

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_my_property(
        param1 in strategy1(),
        param2 in strategy2()
    ) {
        let snapshot = create_snapshot(param1, param2);
        let plan = optimize(&snapshot);

        // Assert property holds
        prop_assert!(plan.cost > 0.0);
    }
}
```

## Test Data

### Timeline File Structure

```toml
[metadata]
name = "My Scenario"
description = "What this demonstrates"
query = "SELECT ..."
dialect = "postgresql"

[[hardware_profiles]]
name = "laptop"
cpu_cores = 4
total_memory = 16_000_000_000

[[snapshots]]
time_offset = 0
label = "Initial state"
hardware_profile = "laptop"

  [snapshots.schema]
    [[snapshots.schema.tables]]
    name = "orders"
    # ... table definition

  [[snapshots.statistics.tables]]
  name = "orders"
  row_count = 1_000_000
  # ... statistics

  [snapshots.facts]
  supports_hash_join = true
  parallel_workers = 4

[[expectations]]
snapshot_index = 0
expected_plan_pattern = ".*SeqScan.*"
expected_cost_range = [10000.0, 20000.0]
rules_applied_must_include = ["filter-pushdown"]
```

### Minimal Timeline Example

```toml
[metadata]
name = "Minimal Test"
description = "Bare minimum timeline"

[[hardware_profiles]]
name = "default"
cpu_cores = 8
total_memory = 32_000_000_000

[[snapshots]]
time_offset = 0
hardware_profile = "default"

  [snapshots.schema]
  tables = []

  [snapshots.statistics]
  tables = []

  [snapshots.facts]
```

## Test Scenarios

### Scenario 1: Schema Change

```rust
#[test]
fn test_schema_change_invalidation() {
    let config = load_timeline("index-addition").unwrap();

    // Snapshot 0: No index
    assert!(config.snapshots[0].schema.tables[0].indexes.is_empty());

    // Snapshot 1: Index added
    assert!(!config.snapshots[1].schema.tables[0].indexes.is_empty());

    // Expectation: Different plans
    let exp0 = &config.expectations[0];
    let exp1 = &config.expectations[1];
    assert!(exp0.invalidation_trigger == "None");
    assert!(exp1.invalidation_trigger == "Index");
}
```

### Scenario 2: Table Growth

```rust
#[test]
fn test_table_growth_impact() {
    let config = load_timeline("growth-replan").unwrap();

    // Get row counts from statistics
    let rows_0 = get_table_rows(&config.snapshots[0], "orders");
    let rows_1 = get_table_rows(&config.snapshots[1], "orders");
    let rows_2 = get_table_rows(&config.snapshots[2], "orders");

    // Verify growth pattern
    assert!(rows_1 >= rows_0 * 10);
    assert!(rows_2 >= rows_1 * 10);

    // Verify join algorithm changes
    assert!(config.expectations[0].expected_plan_pattern
        .as_ref().unwrap().contains("NestedLoop"));
    assert!(config.expectations[2].expected_plan_pattern
        .as_ref().unwrap().contains("Parallel"));
}
```

### Scenario 3: Hardware Upgrade

```rust
#[test]
fn test_hardware_scaling() {
    let config = load_timeline("hardware-upgrade").unwrap();

    // Get hardware profiles
    let hw0 = config.get_hardware_profile(
        &config.snapshots[0].hardware_profile
    ).unwrap();
    let hw2 = config.get_hardware_profile(
        &config.snapshots[2].hardware_profile
    ).unwrap();

    // Verify CPU scaling
    assert!(hw2.cpu_cores > hw0.cpu_cores * 10);

    // Verify parallelism enabled
    assert!(config.expectations[2]
        .rules_applied_must_include
        .contains(&"parallel-scan-introduction".to_string()));
}
```

## Debugging Tests

### Enable Verbose Logging

```rust
#[test]
fn test_with_logging() {
    env_logger::init();

    let config = load_timeline("index-addition").unwrap();
    log::info!("Loaded timeline with {} snapshots", config.snapshots.len());

    for (i, snapshot) in config.snapshots.iter().enumerate() {
        log::debug!("Snapshot {}: {}", i, snapshot.label);
    }
}
```

### Dump Timeline Structure

```rust
#[test]
fn test_dump_timeline() {
    let config = load_timeline("index-addition").unwrap();

    println!("Timeline: {}", config.metadata.name);
    println!("Snapshots: {}", config.snapshots.len());

    for (i, snapshot) in config.snapshots.iter().enumerate() {
        println!("  [{}] {} (offset: {})",
            i, snapshot.label, snapshot.time_offset);
    }

    println!("Expectations: {}", config.expectations.len());
    for (i, exp) in config.expectations.iter().enumerate() {
        println!("  [{}] Snapshot {} - {:?}",
            i, exp.snapshot_index, exp.expected_plan_pattern);
    }
}
```

### Compare Snapshots

```rust
fn compare_snapshots(config: &TimelineConfig, idx1: usize, idx2: usize) {
    let s1 = &config.snapshots[idx1];
    let s2 = &config.snapshots[idx2];

    println!("Comparing snapshots {} vs {}", idx1, idx2);

    // Compare hardware
    if s1.hardware_profile != s2.hardware_profile {
        println!("  Hardware changed: {} -> {}",
            s1.hardware_profile, s2.hardware_profile);
    }

    // Compare schema (simplified)
    // Compare statistics (simplified)
    // Compare facts (simplified)
}
```

## Common Patterns

### Pattern 1: Progressive Optimization

Test that each snapshot improves on the previous:

```rust
#[test]
fn test_progressive_optimization() {
    let config = load_timeline("schema-evolution").unwrap();

    let mut prev_cost = f64::INFINITY;
    for exp in &config.expectations {
        if let Some([min, max]) = exp.expected_cost_range {
            let avg_cost = (min + max) / 2.0;
            assert!(avg_cost < prev_cost,
                "Cost should decrease progressively");
            prev_cost = avg_cost;
        }
    }
}
```

### Pattern 2: Invalidation Triggers

Test that changes trigger appropriate invalidation:

```rust
#[test]
fn test_invalidation_triggers() {
    let config = load_timeline("index-addition").unwrap();

    for exp in &config.expectations {
        if exp.snapshot_index > 0 {
            assert!(exp.invalidation_trigger.is_some(),
                "Non-initial snapshot should have trigger");
        }
    }
}
```

### Pattern 3: Rule Progression

Test that rules are applied in expected order:

```rust
#[test]
fn test_rule_progression() {
    let config = load_timeline("tpch-q5-evolution").unwrap();

    // First snapshot: no hash joins
    assert!(config.expectations[0]
        .rules_applied_must_not_include
        .contains(&"hash-join-introduction".to_string()));

    // Second snapshot: hash joins introduced
    assert!(config.expectations[1]
        .rules_applied_must_include
        .contains(&"hash-join-introduction".to_string()));

    // Third snapshot: parallel hash joins
    assert!(config.expectations[2]
        .rules_applied_must_include
        .contains(&"parallel-hash-join".to_string()));
}
```

## Best Practices

### Timeline Design

1. **Start Simple:** 2-3 snapshots showing clear before/after
2. **Label Clearly:** Descriptive labels for each snapshot
3. **Document Events:** Mark when changes occur
4. **Add Expectations:** Include test assertions
5. **Realistic Data:** Use real-world row counts, NDVs

### Test Writing

1. **Test Structure:** Load → Validate → Assert
2. **Fail Fast:** Check critical invariants early
3. **Clear Messages:** Descriptive assertion messages
4. **Independent Tests:** Each test should be self-contained
5. **Clean Code:** Extract helpers for repeated patterns

### Maintenance

1. **Update Timelines:** When optimizer changes, update expectations
2. **Add Coverage:** Create timelines for new rules
3. **Review Regularly:** Monthly coverage review
4. **Document Changes:** Update README when adding timelines

## Coverage Tracking

### Check Current Coverage

```bash
# Run all tests with coverage tracking
cargo test --all -- --test-threads=1

# Generate coverage report (requires coverage tools)
cargo tarpaulin --out Html --output-dir coverage/
```

### Identify Gaps

```bash
# List all timeline files
ls tests/data/timelines/*.toml

# Check which rules are exercised
grep -r "rules_applied_must_include" tests/data/timelines/

# Compare against rule catalog
diff <(find crates/ra-engine/src/rules -name "*.rs") \
     <(grep -h "rules_applied_must_include" tests/data/timelines/*.toml)
```

### Coverage Report Structure

See `COVERAGE_ANALYSIS.md` for:
- Coverage by timeline
- Coverage by category (scans, joins, aggregates)
- Priority gaps
- Suggested timelines for gaps

## Troubleshooting

### Timeline Won't Load

```rust
// Debug TOML parsing errors
match load_timeline("my-timeline") {
    Ok(config) => println!("Loaded successfully"),
    Err(e) => {
        println!("Failed to load: {:?}", e);
        // Check TOML syntax
        // Verify file exists
        // Check field names match TimelineConfig
    }
}
```

### Validation Fails

```rust
// Check validation errors
let config: TimelineConfig = toml::from_str(&content)?;
match config.validate() {
    Ok(_) => println!("Valid"),
    Err(e) => {
        println!("Validation error: {}", e);
        // Check time offsets are sorted
        // Check hardware profile references
        // Check expectation indices
    }
}
```

### Expectation Doesn't Match

```rust
// Debug expectation mismatches
let plan = optimize_snapshot(&config, 1);
let exp = &config.expectations[1];

if let Some(pattern) = &exp.expected_plan_pattern {
    let regex = Regex::new(pattern).unwrap();
    if !regex.is_match(&plan) {
        println!("Pattern: {}", pattern);
        println!("Plan:\n{}", plan);
        println!("Did not match!");
    }
}
```

## References

- [Timeline README](README.md) - Timeline file format and usage
- [Coverage Analysis](COVERAGE_ANALYSIS.md) - Rule coverage and gaps
- [Phase 5 Summary](PHASE5_SUMMARY.md) - Implementation overview
- [RFC 0105](../../../docs/rfcs/0105-timeline-fingerprints.md) - Design document

## Contributing

When adding new tests:

1. Create timeline file in `tests/data/timelines/`
2. Add integration test in `timeline_integration_test.rs`
3. Update `README.md` with scenario description
4. Update `COVERAGE_ANALYSIS.md` with coverage impact
5. Run tests to verify: `cargo test --test timeline_integration_test`
6. Submit PR with timeline + tests + documentation

For questions or issues, refer to the test files for examples.
