# Semi-Join Reduction

## Overview

Converts EXISTS subqueries and IN predicates to efficient semi-joins, and optimizes semi-join patterns for better performance.

## Rules

### EXISTS to Semi-Join
- `EXISTS(subquery) -> SEMI JOIN`
- `NOT EXISTS(subquery) -> ANTI JOIN`
- Converts correlated EXISTS patterns to joins

### IN to Semi-Join
- `IN(subquery) -> SEMI JOIN`
- `NOT IN(subquery) -> ANTI JOIN`
- Handles single-column subquery patterns

### Semi-Join Optimization
- `DISTINCT after SEMI JOIN -> remove DISTINCT`
- Semi-joins already produce distinct left-side results
- Push filters through semi-joins to reduce data early

### Semi-Join Merging
- Adjacent semi-joins with same right side can be merged
- `(A semi-join B) semi-join B -> A semi-join (B with combined conditions)`

### Anti-Join Optimization
- `ANTI JOIN with empty right -> left input`
- Push filters through anti-joins

### ANY/ALL Patterns
- `col op ANY(subquery) -> SEMI JOIN`
- `col op ALL(subquery) -> ANTI JOIN with negated condition`

### Scalar Subquery
- Scalar subqueries in projection converted to left join + aggregate
- Enables better optimization of correlated scalar subqueries

## Benefits

1. **Efficient execution**: Semi-joins are more efficient than nested loops
2. **Early filtering**: Reduces intermediate result sizes
3. **Better join ordering**: Semi-joins can be reordered with other joins
4. **Bloom filter eligibility**: Semi-joins work well with runtime filters

## Implementation

Located in: `crates/ra-engine/src/semi_join.rs`

Priority: **MEDIUM** - Applied after basic join optimizations.

## Example

```sql
-- Before: EXISTS subquery
SELECT * FROM orders o
WHERE EXISTS (
    SELECT 1 FROM customers c
    WHERE c.id = o.customer_id AND c.country = 'USA'
)

-- After: Semi-join
SELECT * FROM orders o
SEMI JOIN customers c
    ON c.id = o.customer_id AND c.country = 'USA'
```

## Testing

- Unit tests for EXISTS/IN conversion
- Integration tests with correlated subqueries
- Performance tests comparing nested loops vs semi-joins
- Tests for NULL handling in NOT IN patterns