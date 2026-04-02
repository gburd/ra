# Timeline Optimizer - Phase 2 Implementation

**Status:** Complete ✅

## Overview

Phase 2 of the timeline-based fingerprint configuration system implements the optimization loop that processes timeline configurations and tracks plan evolution across snapshots.

## Implementation

### Core Module: `timeline_optimizer.rs`

**Location:** `/home/gburd/ws/ra/crates/ra-engine/src/timeline_optimizer.rs` (600 lines)

#### Key Components

1. **TimelineOptimizer** - Main orchestration struct
   - Processes each snapshot in time order
   - Creates `SnapshotFactsProvider` for each snapshot
   - Calls `Optimizer::optimize()` with snapshot-specific facts
   - Tracks plan dependencies and evolution

2. **Result Types**
   - `TimelineOptimizationResult` - Complete optimization results
   - `SnapshotResult` - Per-snapshot optimization details
   - `ChangeDescription` - Detected changes between snapshots

3. **Change Detection Functions**
   - `detect_schema_changes()` - Indexes, columns, constraints
   - `detect_stats_changes()` - Row counts, NDV, histograms
   - `detect_hardware_changes()` - CPU, memory, GPU
   - `detect_fact_changes()` - Feature flags, configurations

4. **Output Formats**
   - JSON - Structured data for programmatic consumption
   - TOML - Config-like format
   - Markdown - Human-readable reports with emojis
   - Text - ASCII tables for terminal display

#### Change Severity Levels

- **Low** - Minor changes (unlikely to affect plans)
- **Medium** - Moderate changes (may affect plans)
- **High** - Major changes (likely to affect plans)
- **Critical** - Almost certain to affect plans

### Integration

#### Exports Added to `lib.rs`

```rust
pub use timeline_optimizer::{
    ChangeDescription, ChangeSeverity, ChangeType, SnapshotResult,
    TimelineOptimizationResult, TimelineOptimizer, detect_fact_changes,
    detect_hardware_changes, detect_schema_changes, detect_stats_changes,
};
```

#### Dependencies on Existing Types

- Uses `PlanDependencies` and `ResourceId` from `differential.rs`
- Uses `StalenessThresholds` for drift detection
- Uses `Optimizer` from `egraph.rs`
- Integrates with Phase 1's `TimelineConfig` and `SnapshotFactsProvider`

### Testing

#### Integration Tests: `timeline_optimizer_test.rs`

**Location:** `/home/gburd/ws/ra/crates/ra-engine/tests/timeline_optimizer_test.rs`

**Test Coverage:**

1. **Basic Functionality**
   - `load_index_addition_timeline` - Loads timeline configurations
   - `optimize_index_addition_timeline` - Full optimization through 3 snapshots
   - `empty_timeline_handling` - Single snapshot edge case

2. **Change Detection**
   - `detect_changes_across_snapshots` - Schema and hardware changes
   - `statistics_drift_detection` - Row count changes with custom thresholds
   - `change_severity_levels` - Proper severity assignment

3. **Output Formats**
   - `output_format_json` - JSON serialization
   - `output_format_markdown` - Markdown report generation
   - `output_format_text` - ASCII table formatting

4. **Advanced Features**
   - `dependencies_tracking` - Plan dependency extraction
   - `custom_thresholds` - Custom staleness thresholds

**All tests pass:** ✅ 11/11 integration tests + 7/7 unit tests

#### Test Timeline Data

Uses `/home/gburd/ws/ra/tests/data/timelines/index-addition.toml`:
- 3 snapshots over 1.5 hours
- Index addition after 30 minutes
- Hardware migration after 1.5 hours
- Row count growth: 1M → 1.05M → 1.1M

### Example Usage

```rust
use ra_engine::{TimelineConfig, TimelineOptimizer, Optimizer};
use ra_core::algebra::RelExpr;

let config = TimelineConfig::from_file("timeline.toml".as_ref())?;
let query = RelExpr::scan("orders");
let optimizer = Optimizer::new();

let mut timeline_optimizer = TimelineOptimizer::new(config, query, optimizer);
let result = timeline_optimizer.optimize_timeline()?;

// Generate report
println!("{}", result.to_markdown());

// Analyze changes
for snapshot in &result.snapshot_results {
    for change in &snapshot.changes_from_previous {
        println!("{:?}: {}", change.severity, change.description);
    }
}
```

### Key Design Decisions

1. **Lifetime Management** - Stores `HardwareProfile` by value rather than reference to avoid lifetime issues in the loop

2. **Threshold Customization** - Supports custom `StalenessThresholds` for different sensitivity levels

3. **Change Severity** - Graduated severity levels (Low/Medium/High/Critical) based on change magnitude

4. **Multiple Output Formats** - JSON for automation, Markdown for humans, Text for terminals

5. **Dependency Tracking** - Records which statistics resources each plan depends on for invalidation

## Files Created/Modified

### Created
- `/home/gburd/ws/ra/crates/ra-engine/src/timeline_optimizer.rs` - Main module (600 lines)
- `/home/gburd/ws/ra/crates/ra-engine/tests/timeline_optimizer_test.rs` - Integration tests (400 lines)
- `/home/gburd/ws/ra/docs/timeline-optimizer-phase2.md` - This document

### Modified
- `/home/gburd/ws/ra/crates/ra-engine/src/lib.rs` - Added module and exports

## Test Results

```
running 11 tests
test load_index_addition_timeline ... ok
test empty_timeline_handling ... ok
test optimize_index_addition_timeline ... ok
test change_severity_levels ... ok
test dependencies_tracking ... ok
test detect_changes_across_snapshots ... ok
test output_format_markdown ... ok
test output_format_json ... ok
test statistics_drift_detection ... ok
test custom_thresholds ... ok
test output_format_text ... ok

test result: ok. 11 passed; 0 failed
```

## Next Steps

Phase 2 is complete. Recommended next phases:

1. **Phase 3: Visualization** - TUI-based timeline visualization with:
   - Plan tree diff view
   - Statistics change visualization
   - Interactive playback mode

2. **Phase 4: CLI Integration** - Add timeline commands to `ra` CLI:
   - `ra timeline optimize <file>` - Run optimization
   - `ra timeline diff <file>` - Show changes
   - `ra timeline export <file>` - Export results

3. **Phase 5: Real-world Integration** - PostgreSQL extension for:
   - Capturing production timelines
   - Exporting system state snapshots
   - Automated timeline generation

## Performance Considerations

- Each snapshot optimization is independent (could parallelize)
- Change detection is O(tables × columns) per snapshot pair
- Memory usage scales linearly with snapshot count
- No differential dataflow used in this phase (future optimization)

## Limitations

- Cost extraction simplified (requires StatisticsProvider access)
- Rule tracking not yet implemented (optimizer doesn't expose applied rules)
- Histogram drift detection uses KL divergence (from differential.rs)
- No support for incremental updates (full reoptimization per snapshot)

## References

- RFC 0059: Plan dependency tracking
- `differential.rs`: Change detection types
- `timeline_config.rs`: Phase 1 configuration
- `timeline_facts.rs`: Phase 1 facts provider
