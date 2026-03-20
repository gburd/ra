# Functional Dependency Exploitation

## Overview

Uses functional dependencies from unique constraints and keys to simplify GROUP BY clauses and eliminate redundant operations.

## Rules

### GROUP BY Simplification
- `GROUP BY pk, col1, col2 -> GROUP BY pk`
- When grouping by a unique key, dependent columns can be removed
- Reduces grouping overhead and memory usage

### DISTINCT Elimination
- `DISTINCT on unique columns -> remove DISTINCT`
- `DISTINCT after GROUP BY -> remove DISTINCT`
- GROUP BY already produces unique groups

### Aggregate Simplification
- `COUNT(DISTINCT unique_col) -> COUNT(col)`
- `MIN/MAX(col) when grouping by col -> project col`
- Aggregates on functionally dependent columns can be simplified

### Self-Join Elimination
- Self-joins on primary keys with identical filters can be simplified
- `t1 JOIN t2 ON t1.pk = t2.pk WHERE t1.x = 5 AND t2.x = 5 -> single scan`

### ORDER BY Simplification
- `ORDER BY pk, dependent_col -> ORDER BY pk`
- Dependent columns don't affect sort order

### Window Function Optimization
- `PARTITION BY pk, dependent_col -> PARTITION BY pk`
- Simplifies window function partitioning

## Benefits

1. **Reduced sorting**: Fewer columns in GROUP BY/ORDER BY
2. **Memory efficiency**: Smaller hash tables for grouping
3. **CPU savings**: Less comparison operations
4. **I/O reduction**: Can eliminate self-joins entirely

## Implementation

Located in: `crates/ra-engine/src/functional_deps.rs`

Priority: **MEDIUM** - Applied after basic simplification rules.

## Dependencies

- Requires constraint metadata (primary keys, unique constraints)
- Needs functional dependency tracking in analysis
- Column dependency graph construction

## Example

```sql
-- Before: GROUP BY with dependent columns
SELECT customer_id, customer_name, SUM(amount)
FROM orders o JOIN customers c ON o.customer_id = c.id
GROUP BY c.id, c.name  -- name is dependent on id

-- After: Simplified GROUP BY
SELECT customer_id, customer_name, SUM(amount)
FROM orders o JOIN customers c ON o.customer_id = c.id
GROUP BY c.id  -- name removed (functionally dependent)
```

## Testing

- Unit tests for each dependency pattern
- Integration tests with constraint metadata
- Performance tests showing reduction in grouping overhead