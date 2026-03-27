# Verbose Mode for Optimization

## Overview

The `--verbose` flag enables detailed tracking of intermediate optimization steps, showing how the query plan transforms with each rule application.

## Usage

```bash
ra-cli optimize --rules-applied --verbose --resource-budget standard "SELECT ..."
```

## Requirements

- Must use `--rules-applied`, `--rules-evaluated`, `--rules-available`, or `--rules-all` flag
- Must use `--resource-budget` (e.g., `standard`, `batch`, etc.) to enable tracking
- The `--verbose` flag (global CLI flag) enables intermediate step capture

## Output Format

When verbose mode is enabled, the output shows:

1. **Original Plan**: The initial query plan before optimization
2. **Step-by-step transformations**: For each rule application:
   - Step number
   - Rule name that was applied
   - Reason why the rule was chosen (e.g., cost improvement)
   - Plan before the rule application
   - Plan after the rule application
   - Cost improvement (if applicable)
3. **Final Optimized Plan**: The resulting plan after all optimizations

### Example Output

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

Step 2: Applied filter-pushdown (iteration 1) rule
  Why: Cost improvement: 0.1245
  Cost improvement: 0.1245

  Plan before:
    └─ Filter ((status = 'pending') AND (year = 2024))
       └─ Scan(orders)

  Plan after:
    └─ Scan(orders, filter: (status = 'pending') AND (year = 2024))

Final Optimized Plan:
└─ Scan(orders, filter: (status = 'pending') AND (year = 2024))
```

## Implementation Details

### Data Structures

- `IntermediateStep`: Captures a single transformation step with:
  - Step number
  - Rule name
  - Reason for application
  - Plan before and after
  - Cost improvement

- `RuleTrackingResult`: Extended with `intermediate_steps` field (optional)

### Performance Impact

- **Non-verbose mode**: No overhead - intermediate steps are not captured
- **Verbose mode**: Minimal overhead - plan extraction happens only when rules modify the e-graph

### Code Changes

1. **ra-engine/src/egraph.rs**:
   - Added `IntermediateStep` struct
   - Added `intermediate_steps` field to `RuleTrackingResult`
   - Added `optimize_with_tracking_verbose()` method
   - Modified optimization loop to capture plans when verbose is enabled

2. **ra-cli/src/main.rs**:
   - Modified `optimize_bounded()` to pass verbose flag
   - Added `print_intermediate_steps()` function for formatted output
   - Updated output logic to show steps when verbose is enabled

3. **Tests**:
   - `test_verbose_mode_captures_intermediate_steps`: Verifies steps are captured
   - `test_non_verbose_mode_skips_intermediate_steps`: Verifies no overhead when disabled

## Use Cases

1. **Debugging optimization**: Understand why a particular optimization was chosen
2. **Learning**: See how rewrite rules transform queries step by step
3. **Performance analysis**: Identify which rules provide the most benefit
4. **Rule development**: Verify new rules are working as expected
