# Timeline Configuration Files

This directory contains timeline configuration files that describe how database systems evolve over time. Timelines capture complete system state (schema, statistics, hardware, facts) at discrete points in time, enabling:

- **Temporal Fingerprints**: Complete environment snapshots for reproducible optimization
- **Deterministic Testing**: "Under conditions (c), expect plan (p)" assertions
- **Adaptive Planning Demonstration**: Visualize how plans adapt as conditions change
- **Query Cache Invalidation**: Show what changes trigger reoptimization
- **What-If Analysis**: Test optimizer behavior under hypothetical scenarios

## File Format

Timeline files use TOML format with the following structure:

```toml
[metadata]
name = "Scenario Name"
description = "What this timeline demonstrates"
query = "SELECT ..."  # Optional: query being optimized
dialect = "postgresql"  # Optional: SQL dialect
duration_seconds = 3600  # Optional: total timeline duration

# Reusable hardware profiles
[[hardware_profiles]]
name = "laptop"
cpu_cores = 4
total_memory = 16_000_000_000
# ... more hardware specs

# Complete snapshots at discrete points
[[snapshots]]
time_offset = 0  # Seconds from timeline start
label = "Initial state"
hardware_profile = "laptop"  # Reference to hardware profile

  [snapshots.schema]
    # Table definitions with columns, indexes, constraints
    [[snapshots.schema.tables]]
    name = "orders"
    storage_format = "row_based"
    # ... columns and indexes

  [snapshots.statistics]
    # Table and column statistics
    [[snapshots.statistics.tables]]
    name = "orders"
    row_count = 1_000_000
    # ... column statistics

  [snapshots.facts]
    # Feature flags and configurations
    supports_hash_join = true
    parallel_workers = 4
    # ... custom facts

# Events mark what changed
[[events]]
time_offset = 1800
kind = "schema_change"
table = "orders"
description = "CREATE INDEX ..."

# Test expectations for validation
[[expectations]]
snapshot_index = 0
expected_plan_pattern = ".*SeqScan.*"  # Regex
expected_cost_range = [20000.0, 30000.0]
rules_applied_must_include = ["filter-pushdown"]
invalidation_trigger = "Index"
```

## Timeline Scenarios

### index-addition.toml
Demonstrates how query plan changes when an index is added mid-execution:
- **Snapshot 0**: No index → Sequential scan
- **Snapshot 1**: Index added → Index scan (700x cost reduction)
- **Snapshot 2**: Hardware upgrade → Parallel index scan

**Key Learning**: Schema changes (adding indexes) trigger plan cache invalidation and reoptimization.

### growth-replan.toml
Demonstrates how 10x-100x table growth causes join algorithm changes:
- **Snapshot 0**: Small dataset (10K rows) → Nested loop join
- **Snapshot 1**: 10x growth (100K rows) → Hash join
- **Snapshot 2**: 100x growth (1M rows) → Parallel hash join

**Key Learning**: Statistics changes (row count growth) trigger reoptimization when staleness threshold exceeded.

### hardware-upgrade.toml
Demonstrates how hardware migration enables parallel execution strategies:
- **Snapshot 0**: Laptop (4 cores, 16GB) → Serial scan
- **Snapshot 1**: Workstation (16 cores, 64GB) → Moderate parallelism (8 workers)
- **Snapshot 2**: Server (64 cores, 512GB) → High parallelism (32 workers)

**Key Learning**: Hardware changes (CPU cores, memory) enable different execution strategies. Parallelism decisions adapt to available resources.

### schema-evolution.toml
Demonstrates progressive index optimization through multiple schema changes:
- **Snapshot 0**: Basic schema, no indexes → Sequential scan
- **Snapshot 1**: Add index on customer_id → Index scan (100x faster)
- **Snapshot 2**: Add composite index (customer_id, status) → Tighter selectivity (10x faster)
- **Snapshot 3**: Add covering index with INCLUDE columns → Index-only scan (5x faster)

**Key Learning**: Each schema change progressively improves query performance. Composite and covering indexes provide substantial benefits.

### staleness-drift.toml
Demonstrates how statistics staleness degrades estimate confidence:
- **Snapshot 0**: Fresh statistics (confidence=1.0) → Accurate estimates
- **Snapshot 1**: 20% data change (confidence=0.7) → Slightly degraded estimates
- **Snapshot 2**: 50% data change (confidence=0.4) → Poor estimates, high tolerance
- **Snapshot 3**: Re-analyzed (confidence=1.0) → Accurate estimates restored

**Key Learning**: Statistics quality directly impacts estimate confidence and plan quality. Re-analyzing restores accuracy.

### join-order.toml
Demonstrates join order flip as relative table sizes change:
- **Snapshot 0**: Orders small (50K), Customers large (500K) → Orders first (build on smaller table)
- **Snapshot 1**: Orders medium (250K), Customers (500K) → Still orders first
- **Snapshot 2**: Orders large (1M), Customers (500K) → JOIN ORDER FLIPS → Customers first

**Key Learning**: Join order selection depends on relative table sizes. When size ratios flip, optimal join order flips.

### tpch-q1-evolution.toml
TPC-H Q1 (Pricing Summary Report) with realistic scale factor progression:
- **Snapshot 0**: SF=0.1 (600K rows), dev hardware, no index → Serial scan/aggregate
- **Snapshot 1**: SF=1 (6M rows), shipdate index added → Index-assisted scan
- **Snapshot 2**: SF=10 (60M rows), production hardware, columnar storage → Parallel vectorized execution

**Key Learning**: Real-world query evolution through scale factors, schema optimizations, and hardware upgrades. Multiple optimization axes.

### tpch-q5-evolution.toml
TPC-H Q5 (Local Supplier Volume) - 5-way join optimization:
- **Snapshot 0**: SF=1, no FK indexes → Nested loop joins (slow)
- **Snapshot 1**: SF=1, FK indexes added → Hash joins (much faster)
- **Snapshot 2**: SF=1, production hardware → Parallel hash joins (fastest)

**Key Learning**: Join-heavy queries benefit enormously from proper indexing and parallelism. Foreign key indexes enable hash joins.

## Creating Timeline Files

### Minimal Example
```toml
[metadata]
name = "My Scenario"
description = "Test scenario"

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

### Best Practices

1. **Start Simple**: Begin with 2-3 snapshots showing clear before/after states
2. **Label Snapshots**: Use descriptive labels explaining what changed
3. **Document Events**: Add events to mark when changes occurred
4. **Add Expectations**: Include test assertions for deterministic testing
5. **Realistic Data**: Use realistic row counts, NDVs, correlations from real workloads
6. **Hardware Profiles**: Define profiles once, reuse across snapshots
7. **Incremental Changes**: Show gradual evolution, not sudden jumps (unless demonstrating sudden events)

### Common Scenarios

**Schema Evolution**:
```toml
# Snapshot 0: No index
# Snapshot 1: Add index on customer_id
# Snapshot 2: Add composite index on (customer_id, status)
# Expectation: Plan switches from SeqScan → IndexScan → IndexOnlyScan
```

**Data Growth**:
```toml
# Snapshot 0: 10K rows → NestedLoop
# Snapshot 1: 100K rows → HashJoin
# Snapshot 2: 1M rows → ParallelHashJoin
# Expectation: Join algorithm adapts to table size
```

**Hardware Changes**:
```toml
# Snapshot 0: laptop (4 cores) → Serial scan
# Snapshot 1: server (64 cores) → Parallel scan
# Expectation: Plan uses parallelism when more cores available
```

**Statistics Staleness**:
```toml
# Snapshot 0: Fresh stats → Good estimates
# Snapshot 1: 50% data change, stale stats → Poor estimates
# Snapshot 2: Re-analyzed → Good estimates again
# Expectation: Plan quality degrades with stale stats
```

**Configuration Changes**:
```toml
# Snapshot 0: work_mem = 64MB → Disk-based sort
# Snapshot 1: work_mem = 512MB → In-memory sort
# Expectation: Plan uses in-memory algorithms when more memory available
```

## Using Timeline Files

### Command Line
```bash
# Optimize query through timeline
ra-cli optimize --timeline timelines/index-addition.toml

# Output in different formats
ra-cli optimize --timeline timelines/growth-replan.toml --output json
ra-cli optimize --timeline timelines/index-addition.toml --output markdown

# Test mode (validate expectations)
ra-cli optimize --timeline timelines/index-addition.toml --test

# Launch TUI visualization
ra-cli optimize --timeline timelines/growth-replan.toml --tui
```

### Rust API
```rust
use ra_engine::timeline_config::TimelineConfig;
use ra_engine::timeline_facts::SnapshotFactsProvider;

// Load timeline
let config = TimelineConfig::from_file(
    Path::new("timelines/index-addition.toml")
)?;

// Iterate through snapshots
for (i, snapshot) in config.snapshots.iter().enumerate() {
    let hardware = config.get_hardware_profile(&snapshot.hardware_profile).unwrap();
    let facts = SnapshotFactsProvider::new(snapshot, hardware);

    // Optimize query with these facts
    let plan = optimizer.optimize_with_facts(&query, &facts)?;
    println!("Snapshot {}: cost = {}", i, plan.cost);
}
```

### Testing
```rust
#[test]
fn test_index_addition_timeline() {
    let config = TimelineConfig::from_file(
        Path::new("timelines/index-addition.toml")
    ).unwrap();

    // Validate expectations
    for expectation in &config.expectations {
        let snapshot = &config.snapshots[expectation.snapshot_index];
        let plan = optimize_snapshot(&config, snapshot);

        // Check plan pattern
        if let Some(pattern) = &expectation.expected_plan_pattern {
            assert!(regex::Regex::new(pattern).unwrap().is_match(&plan.to_string()));
        }

        // Check cost range
        if let Some([min, max]) = expectation.expected_cost_range {
            assert!(plan.cost >= min && plan.cost <= max);
        }
    }
}
```

## Timeline Validation

Timeline files are validated on load:
- ✓ At least one snapshot exists
- ✓ Time offsets are in ascending order
- ✓ Hardware profile references are valid
- ✓ Expectation snapshot indices are in bounds
- ✓ Regex patterns in expectations are valid

Invalid timelines will fail with clear error messages.

## Future Extensions

Planned features for timeline system:
- **Timeline Templates**: Reusable schema/hardware templates
- **Timeline Composition**: Combine multiple timelines
- **Interactive Builder**: TUI for building timelines interactively
- **Cloud Repository**: Share and discover timeline configs
- **PostgreSQL Capture**: Export timelines from live PostgreSQL databases
- **Proxy Capture**: Real-time timeline capture from query traffic

## Contributing Timeline Files

When adding new timeline files:
1. Place in `tests/data/timelines/`
2. Use descriptive filename (e.g., `join-order-flip.toml`)
3. Add documentation in this README
4. Include test expectations
5. Verify with `ra-cli optimize --timeline <file> --test`

## References

- [RFC 0105: Timeline-Based Fingerprint Configuration](../../../docs/rfcs/0105-timeline-fingerprints.md)
- [Timeline Optimizer Design](../../../crates/ra-engine/src/timeline_optimizer.rs)
