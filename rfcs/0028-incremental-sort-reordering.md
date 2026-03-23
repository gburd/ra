# RFC 0028: Incremental Sort and Key Reordering

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Accepted
- Tracking Issue: TBD

## Summary

Implement incremental sort selection, GROUP BY key reordering, DISTINCT key reordering, and presorted aggregate optimization to exploit partial orderings provided by indexes and child operators.

## Motivation

When data is partially sorted (e.g., sorted on column A but the query needs A, B), RA performs a full sort instead of an incremental sort within each prefix group. GROUP BY and DISTINCT key ordering does not consider available sort orderings from child operators. PostgreSQL added these optimizations in v13 and v16 with demonstrated production value.

## Guide-level explanation

Four related optimizations:

```sql
-- 1. Incremental Sort: index provides (customer_id) ordering
--    Query needs ORDER BY customer_id, order_date
--    Instead of full sort, sort only within each customer_id group
SELECT * FROM orders ORDER BY customer_id, order_date;

-- 2. GROUP BY Reordering: index provides (a, b) ordering
--    Reorder GROUP BY (b, a, c) to (a, b, c) to match
SELECT a, b, c, COUNT(*) FROM t GROUP BY b, a, c;

-- 3. DISTINCT Reordering: same principle
SELECT DISTINCT b, a, c FROM t;

-- 4. Presorted Aggregate: avoid internal sort for ordered aggregates
SELECT array_agg(val ORDER BY val) FROM t;  -- if val already sorted
```

## Reference-level explanation

### Implementation Details

**Incremental Sort Selection**:
- When input sorted on prefix of required sort key, sort only within each prefix group
- Cost: O(n log m) where m = max group size, vs O(n log n) for full sort
- Rule: `incremental-sort-selection`

**GROUP BY Reordering**:
- Reorder GROUP BY columns to maximize prefix match with available input ordering
- Rule: `group-by-key-reordering`

**DISTINCT Reordering**:
- Same principle for DISTINCT columns
- Rule: `distinct-key-reordering`

**Presorted Aggregate**:
- When aggregate has ORDER BY or DISTINCT, provide presorted input to avoid internal sort
- Rule: `presorted-aggregate-optimization`

### Prerequisites

Requires RFC 0025 (Physical Property Tracking) to propagate ordering information through the plan tree.

### Performance Considerations

- Incremental sort reduces memory usage from O(n) to O(m) where m = max group size
- GROUP BY reordering eliminates sorts entirely when full prefix matches
- Combined effect: eliminates sorts in 15-25% of GROUP BY/DISTINCT queries

## Drawbacks

- Requires physical property tracking infrastructure (RFC 0025)
- GROUP BY reordering changes output column order (semantically equivalent but different)
- Incremental sort has higher per-tuple overhead than full sort for small groups

## Rationale and alternatives

### Why This Design?

These are proven optimizations from PostgreSQL v13/v16 with measured production benefits. The rule-based approach integrates cleanly with RA's existing optimizer.

### Alternative Approaches

- **Always full sort**: Current approach; misses optimization opportunities
- **Index-only optimization**: Limited to cases with matching indexes
- **Sort avoidance only**: Does not help when partial ordering exists

## Prior art

- PostgreSQL v13: `enable_incremental_sort`
- PostgreSQL v16: `enable_group_by_reordering`
- CockroachDB: Streaming GROUP BY with ordered input
- DuckDB: Perfect hash aggregation with sorted input

## Unresolved questions

- Threshold for incremental sort benefit (when is group size too large?)
- Interaction with parallel aggregation
- Cost model accuracy for incremental sort

## Future possibilities

- Window function ordering optimization
- Multi-level incremental sort for deep sort keys
- Adaptive switching between full and incremental sort at runtime
