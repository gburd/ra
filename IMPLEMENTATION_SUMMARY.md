# Verbose Mode Implementation Summary

## Overview

Implemented `--verbose` mode for the `ra-cli optimize` command to display intermediate optimization steps, showing how each rule transforms the query plan.

## Changes Made

### 1. Core Engine Changes (`crates/ra-engine/src/egraph.rs`)

#### New Data Structures

```rust
/// A single step in the optimization process showing plan transformation.
pub struct IntermediateStep {
    pub step_number: usize,
    pub rule_name: String,
    pub reason: String,
    pub plan_before: RelExpr,
    pub plan_after: RelExpr,
    pub cost_improvement: Option<f64>,
}
```

Extended `RuleTrackingResult`:
```rust
pub struct RuleTrackingResult {
    pub applied: Vec<RuleApplication>,
    pub evaluated: Vec<RuleEvaluation>,
    pub available: Vec<String>,
    pub intermediate_steps: Option<Vec<IntermediateStep>>,  // NEW
}
```

#### New Methods

- `optimize_with_tracking_verbose(expr: &RelExpr, verbose: bool)`: Main entry point for verbose optimization
- Existing `optimize_with_tracking()` now delegates to the verbose version with `verbose=false`

#### Optimization Loop Changes

When verbose mode is enabled:
1. Extract plan **before** applying each rule
2. Apply the rule
3. Extract plan **after** applying the rule
4. If the rule modified the e-graph, capture:
   - Step number
   - Rule name
   - Reason (cost improvement or pattern match)
   - Plans before and after
   - Cost improvement

When verbose mode is disabled:
- No plan extraction overhead
- Only tracks rule applications (existing behavior)

#### Helper Functions

- `build_detailed_tracking_with_steps()`: Creates tracking result with optional intermediate steps
- Updated `build_aggregate_tracking()` to include `intermediate_steps: None`

### 2. CLI Changes (`crates/ra-cli/src/main.rs`)

#### Updated Functions

**`optimize_bounded()`**:
```rust
let result = if show_rules.should_track() {
    optimizer.optimize_with_tracking_verbose(plan, verbose)  // Pass verbose flag
        .with_context(|| format!("failed to optimize query: {query}"))?
} else {
    optimizer.optimize_bounded(plan)
        .with_context(|| format!("failed to optimize query: {query}"))?
};
```

**New display function**:
```rust
fn print_intermediate_steps(
    tracking: &ra_engine::RuleTrackingResult,
    original_plan: &ra_core::algebra::RelExpr,
)
```

Displays:
- Original plan
- Each transformation step with:
  - Step number and rule name
  - Reason for application
  - Cost improvement
  - Plan before and after (indented)
- Final optimized plan

#### Output Logic

```rust
if show_rules != RuleDisplayMode::None {
    if verbose {
        // Show detailed step-by-step transformations
        if let Some(tracking) = &result.rule_tracking {
            print_intermediate_steps(tracking, plan);
        }
    } else {
        // Show summary rule tracking (existing behavior)
        print_rule_tracking(&result, show_rules);
    }
}
```

### 3. Tests (`crates/ra-engine/src/egraph.rs`)

#### Test: `test_verbose_mode_captures_intermediate_steps`

Verifies:
- Intermediate steps are captured when verbose=true
- Each step has complete information (step number, rule name, reason)
- Steps contain plan transformations

#### Test: `test_non_verbose_mode_skips_intermediate_steps`

Verifies:
- No intermediate steps when verbose=false
- Zero overhead in non-verbose mode

## Usage

### Command Line

```bash
# Verbose mode - show step-by-step transformations
ra-cli optimize --rules-applied --verbose --resource-budget standard \
  "SELECT * FROM orders WHERE status = 'pending' AND year = 2024"

# Normal mode - show summary
ra-cli optimize --rules-applied --resource-budget standard \
  "SELECT * FROM orders WHERE status = 'pending' AND year = 2024"
```

### Requirements

1. Must use one of the `--rules-*` flags:
   - `--rules-applied`: Show rules that modified the plan
   - `--rules-evaluated`: Show rules that were tried
   - `--rules-available`: Show all available rules
   - `--rules-all`: Show all of the above

2. Must use `--resource-budget` to enable tracking (e.g., `standard`, `batch`, `interactive`)

3. Add `--verbose` global flag to enable intermediate step capture

## Example Output

```
Intermediate Optimization Steps:

Original Plan:
└─ Filter (status = 'pending')
   └─ Filter (year = 2024)
      └─ Scan(orders)

Step 1: Applied filter-merge (iteration 1) rule
  Why: Cost improvement: 0.0523
  Cost improvement: 0.0523

  Plan before:
    └─ Filter (status = 'pending')
       └─ Filter (year = 2024)
          └─ Scan(orders)

  Plan after:
    └─ Filter ((status = 'pending') AND (year = 2024))
       └─ Scan(orders)

Final Optimized Plan:
└─ Filter ((status = 'pending') AND (year = 2024))
   └─ Scan(orders)
```

## Performance Impact

### Non-Verbose Mode (default)
- **Zero overhead**: `intermediate_steps` field is `None`
- Only tracks rule applications (existing behavior)
- No plan extraction

### Verbose Mode
- **Minimal overhead**: Plan extraction only when rules modify e-graph
- Extracts plan before/after each successful rule application
- Overhead proportional to number of rules that fire

## Design Decisions

1. **Optional Field**: `intermediate_steps` is `Option<Vec<IntermediateStep>>` to avoid overhead when not needed

2. **Verbose Flag at Optimizer Level**: Passed to `optimize_with_tracking_verbose()` to control capture at the source

3. **Step-by-Step Display**: Shows transformations sequentially for clarity

4. **Original Plan First**: Displays the starting point before showing transformations

5. **Cost Information**: Shows cost improvement when measurable to justify transformations

6. **Indented Output**: Uses indentation to clearly separate before/after plans

## Files Modified

1. `/home/gburd/ws/ra/crates/ra-engine/src/egraph.rs`
   - Added `IntermediateStep` struct
   - Extended `RuleTrackingResult`
   - Added `optimize_with_tracking_verbose()` method
   - Modified optimization loop to capture steps
   - Added helper functions
   - Added tests

2. `/home/gburd/ws/ra/crates/ra-cli/src/main.rs`
   - Updated `optimize_bounded()` to pass verbose flag
   - Added `print_intermediate_steps()` function
   - Modified output logic to show steps when verbose

## Testing

All tests pass:
- `test_verbose_mode_captures_intermediate_steps`: ✓
- `test_non_verbose_mode_skips_intermediate_steps`: ✓
- Code compiles successfully: ✓

## Documentation

Created:
- `VERBOSE_MODE.md`: User-facing documentation
- `IMPLEMENTATION_SUMMARY.md`: This file (developer documentation)
- `test_verbose_mode.sh`: Test script for manual verification

## Future Enhancements

Possible improvements:
1. Add plan diff visualization showing only changed parts
2. Add timing information for each step
3. Add filtering to show only specific rule applications
4. Add JSON output format for programmatic analysis
5. Add visualization of the e-graph growth
