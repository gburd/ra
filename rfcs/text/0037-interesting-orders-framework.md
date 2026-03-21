# RFC 0037: Interesting Orders Framework

## Status
PROPOSED

## Summary
Implement physical property tracking for sort orders throughout query plans, enabling the optimizer to avoid redundant sorts and choose optimal join algorithms based on available orderings.

## Motivation
RA currently lacks tracking of physical properties like sort order through plan nodes. This leads to unnecessary sort operations when data is already sorted, and misses opportunities to use merge joins when both inputs have compatible orderings. PostgreSQL's "pathkeys" system demonstrates 20-50% reduction in sort operations.

## Design

### Core Abstractions

```rust
pub struct PhysicalProperties {
    ordering: Option<SortOrder>,
    partitioning: Option<Partitioning>,
    distribution: Option<Distribution>,
}

pub struct SortOrder {
    keys: Vec<SortKey>,
    is_strict: bool,  // false if prefix is acceptable
}

pub struct InterestingOrders {
    required: Vec<SortOrder>,    // From ORDER BY, GROUP BY, joins
    provided: Vec<SortOrder>,     // From indexes, previous sorts
}
```

### Property Propagation

1. **Bottom-Up**: Each operator declares what orderings it provides
2. **Top-Down**: Parent operators request orderings from children
3. **Enforcement**: Insert Sort nodes when required order unavailable

### Operator Rules

- **TableScan**: Provides index ordering if index scan
- **Sort**: Provides requested ordering
- **MergeJoin**: Requires ordered inputs, preserves order
- **HashJoin**: Destroys input ordering
- **Filter**: Preserves input ordering
- **Project**: May preserve if no computed columns
- **Aggregate**: Preserves GROUP BY ordering

### Cost Integration

```rust
impl CostModel {
    fn sort_cost(&self, rows: f64, already_sorted: bool) -> Cost {
        if already_sorted {
            Cost::zero()
        } else {
            rows * rows.log2() * self.comparison_cost
        }
    }
}
```

## Implementation Plan

1. Define PhysicalProperties trait and types
2. Implement property derivation for each operator
3. Add interesting order collection from query
4. Modify plan enumeration to track properties
5. Update cost model for ordering-aware costing
6. Add sort enforcement rules

## Alternatives Considered

- **Annotation-Only**: Track but don't optimize (insufficient)
- **Full Cascades**: Too complex for initial implementation
- **Post-Process**: Miss opportunities during optimization

## Success Criteria

- Eliminate 90% of redundant sorts in TPC-H queries
- Choose merge join when beneficial due to existing order
- < 5% overhead in optimization time
- Correct ordering guarantees for all queries