# Redundant Join Elimination

## Overview

Identifies and removes joins that don't contribute meaningful data or filtering to the query result.

## Rules

### Cross Join with Single Row
- `JOIN CROSS with single-row relation -> left input`
- Eliminates cross joins with relations that add no columns
- Example: `SELECT * FROM t1 CROSS JOIN (SELECT 1)`

### Inner Join with TRUE Condition
- `JOIN INNER TRUE with single-row -> left input`
- Removes inner joins that don't filter when right has one row

### Self-Join on Unique Key
- `Self-join on unique key -> single scan`
- When joining a table with itself on a unique key and only one side is used
- Requires uniqueness constraint metadata

### Semi-Join with TRUE
- `SEMI JOIN TRUE with non-empty right -> left input`
- Semi-join with always-true condition is redundant if right is non-empty

### Anti-Join with Empty Right
- `ANTI JOIN with empty right -> left input`
- All left rows are preserved when right is empty

### Unused Join Results
- Cross/left join followed by projection using only one side
- Join can be eliminated if its columns aren't used

### Inner Join + Distinct to Semi-Join
- Convert inner join + distinct to semi-join for existence checking
- More efficient when only checking if rows exist

## Benefits

1. **Reduced I/O**: Eliminates unnecessary table scans
2. **Memory savings**: Reduces join buffer requirements
3. **CPU savings**: Avoids join processing overhead
4. **Simpler plans**: Easier to optimize and execute

## Implementation

Located in: `crates/ra-engine/src/redundant_join.rs`

Priority: **MEDIUM** - Applied after null simplification and basic rules.

## Dependencies

- Requires table statistics for non-empty detection
- Needs uniqueness constraint information
- Column usage analysis for projection-based elimination

## Testing

- Unit tests for each elimination pattern
- Integration tests with complex join queries
- Performance benchmarks showing I/O reduction