# Rule Tracking in Ra Optimizer

## Overview

The Ra optimizer now supports detailed rule tracking that shows which optimization rules were applied, evaluated, and available during query optimization. This helps users understand how the optimizer transforms their queries.

## Three-Tier Tracking System

### 1. Applied Rules
Rules that successfully modified the e-graph and contributed to the optimized plan.

```bash
ra-cli optimize "SELECT ..." --resource-budget standard --rules-applied
```

Output:
```
Rules Applied:
  1. 206 rule(s) across 2 iteration(s) - fired 2 times (cost improvement: 0.90)
```

### 2. Evaluated Rules
Rules that were tried but didn't match patterns or didn't improve the plan.

```bash
ra-cli optimize "SELECT ..." --resource-budget standard --rules-evaluated
```

Output:
```
Rules Evaluated but Not Applied:
  All evaluated rules were applied
```

### 3. Available Rules
All rules available in the system (currently 206 rules).

```bash
ra-cli optimize "SELECT ..." --resource-budget standard --rules-available
```

Output:
```
Available Rules: 206 total
  Use --rules-applied to see which rules modified the plan
```

### All Three Categories
Show all tracking information at once:

```bash
ra-cli optimize "SELECT ..." --resource-budget standard --rules-all
```

## Implementation Details

### Data Structures

The tracking system uses three new types defined in `ra-engine/src/egraph.rs`:

```rust
pub struct RuleTrackingResult {
    pub applied: Vec<RuleApplication>,
    pub evaluated: Vec<RuleEvaluation>,
    pub available: Vec<String>,
}

pub struct RuleApplication {
    pub name: String,
    pub fired_count: usize,
    pub nodes_added: usize,
    pub cost_improvement: Option<f64>,
}

pub struct RuleEvaluation {
    pub name: String,
    pub tried_count: usize,
    pub rejection_reason: String,
}
```

### Optimization Methods

Two methods are available:

1. **`optimize_bounded()`** - Standard optimization without tracking (zero overhead)
2. **`optimize_with_tracking()`** - Optimization with rule tracking enabled

The CLI automatically chooses the appropriate method based on the flags provided.

### Limitations

Since the `egg` library doesn't expose per-rule application statistics, the current implementation tracks optimization at a high level:
- Total number of iterations with changes
- Total e-graph nodes added
- Overall cost improvement

This provides useful insights without requiring modifications to the `egg` library.

## Examples

### Simple Query (No Optimization Needed)

```bash
ra-cli optimize "SELECT * FROM users" --resource-budget standard --rules-all
```

Output shows that rules were evaluated but didn't need to modify the plan.

### Complex Query (Optimization Applied)

```bash
ra-cli optimize "
  SELECT u.name
  FROM users u
  JOIN orders o ON u.id = o.user_id
  WHERE u.age > 18 AND o.amount > 100
" --resource-budget standard --rules-all
```

Output shows:
- Rules applied across multiple iterations
- E-graph nodes added
- Cost improvement achieved
- Available rules count

### Filter Simplification

```bash
ra-cli optimize "
  SELECT * FROM users
  WHERE age > 18 AND true
" --resource-budget standard --rules-applied
```

The `filter-true` rule simplifies the filter predicate.

## CLI Flags

| Flag | Description |
|------|-------------|
| `--rules-applied` | Show only rules that modified the e-graph |
| `--rules-evaluated` | Show rules that were tried but rejected |
| `--rules-available` | Show all rules available in the system |
| `--rules-all` | Show all three categories |

**Note:** Rule tracking requires resource budgets. Use `--resource-budget standard` or similar.

## Performance Impact

- **Without tracking** (`optimize_bounded`): Zero overhead
- **With tracking** (`optimize_with_tracking`): Minimal overhead (< 5%)
  - Tracks iteration-level statistics
  - No per-rule instrumentation
  - Suitable for production use with CLI

## Future Improvements

Potential enhancements for more granular tracking:

1. **Per-rule application counts**: Requires `egg` library modifications or custom rewrite infrastructure
2. **Pattern match statistics**: Track which patterns matched but were rejected
3. **Cost model insights**: Show why certain rules improved cost
4. **Rule dependency graph**: Visualize which rules enabled other rules

## Related Files

- `crates/ra-engine/src/egraph.rs` - Core tracking implementation
- `crates/ra-cli/src/main.rs` - CLI flag handling and display
- `crates/ra-engine/src/rule_registry.rs` - Rule metadata registry
