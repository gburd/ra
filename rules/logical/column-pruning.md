# Column Pruning

## Overview

Removes unused columns as early as possible in the query plan to reduce memory usage and I/O costs.

## Rules

### Projection Pushdown
- Push projections through filters to eliminate columns early
- Push projections through joins (both sides independently)
- Merge adjacent projections

### Join Column Pruning
- Semi/anti joins only need join columns from right side
- Outer joins can prune unused columns from nullable side
- Cross joins can eliminate entire side if columns unused

### Aggregate Column Pruning
- Push projection below aggregate to only keep needed columns
- Remove unused aggregate columns from output

### Set Operation Pruning
- Push projections through UNION/INTERSECT/EXCEPT
- All branches get same projection

### Sort Column Pruning
- Only keep columns needed for sort keys and final output
- Remove intermediate columns after sorting

### Scan-Level Pruning
- Add projections immediately after table scans
- Only read columns that will be used upstream

### Window Function Pruning
- Only keep columns needed for window functions and output
- Prune partition/order columns not in final result

## Benefits

1. **Reduced I/O**: Read fewer columns from storage
2. **Memory efficiency**: Smaller tuples in memory
3. **Network savings**: Less data transfer in distributed queries
4. **Cache efficiency**: Better CPU cache utilization

## Implementation

Located in: `crates/ra-engine/src/column_pruning.rs`

Priority: **MEDIUM** - Applied throughout optimization.

## Dependencies

- Column usage analysis
- Projection capability at all operator levels
- Storage engine column pruning support

## Example

```sql
-- Before: Reading all columns
SELECT a FROM (
    SELECT * FROM large_table
    WHERE b > 100
)

-- After: Early column pruning
SELECT a FROM (
    SELECT a, b FROM large_table  -- Only read needed columns
    WHERE b > 100
)
```

## Metrics

- Columns eliminated at each level
- I/O reduction percentage
- Memory usage reduction
- Network bandwidth savings

## Testing

- Unit tests for each pruning pattern
- Integration tests with wide tables
- Performance tests measuring I/O reduction
- Tests ensuring correctness with column references