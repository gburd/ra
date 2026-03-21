# Rule: Window Partition Pruning

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/window-partition-pruning.rra`

## Metadata

- **ID:** `window-partition-pruning`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, mssql
- **Tags:** window, partition, pruning, filter
- **Authors:** "RA Contributors"


# Window Partition Pruning

## Description

When a filter on the PARTITION BY column exists above or below the window, partition pruning eliminates entire partitions from processing. If only one partition survives, the PARTITION BY can be removed entirely.

**When to apply**: An equality filter on a PARTITION BY column.

## Relational Algebra

```algebra
Filter[partition_col = V](Window[fns PARTITION BY partition_col](input))
  -> Window[fns](Filter[partition_col = V](input))
  -- Remove PARTITION BY since only one partition exists
```

## Implementation

```rust
rw!("window-partition-pruning";
    "(filter (= ?col ?val) (window ?fns ?input))" =>
    "(window (remove-partition ?col ?fns) (filter (= ?col ?val) ?input))"
    if is_partition_column("?col", "?fns")
    if is_constant("?val")
),
```

## Test Cases

### Positive: Single partition selected

```sql
SELECT *, RANK() OVER (PARTITION BY dept ORDER BY salary DESC) as rnk
FROM employees
WHERE dept = 'Engineering';

-- Only one partition; remove PARTITION BY
```

## References

- Partition pruning in analytical query processing
- Window function optimization in DuckDB
