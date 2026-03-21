# Rule: "CockroachDB Merge Join Generation with Ordering"

**Category:** physical/join-selection
**File:** `rules/database-specific/cockroachdb/cockroachdb-generate-merge-joins.rra`

## Metadata

- **ID:** `cockroachdb-generate-merge-joins`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** cockroachdb, join, merge-join, physical-rewrite, interesting-orderings
- **Authors:** "Cockroach Labs", "RA Contributors"


# CockroachDB Merge Join Generation

## Description

CockroachDB's GenerateMergeJoins function explores merge join execution strategies
when both join inputs provide compatible "interesting orderings" on the join columns.
A merge join reads both inputs in sorted order and combines tuples in a single
forward pass, achieving O(n+m) tuple comparisons instead of O(n*m) for nested
loop joins. This is the most efficient join method when inputs are pre-sorted.

The rule works with CockroachDB's interesting orderings framework, which tracks
what sort orderings are available from each possible input plan in the memo.

**When to apply**: After join reordering (via join graph), when CockroachDB explores
different physical implementations for a join operator.

**CockroachDB-specific**: Interacts with the memo and interesting orderings to
understand what plans are available for each input. Works with both local and
distributed execution contexts.

## Relational Algebra

```algebra
-- Generic join input, no specified ordering
(inner-join
  (scan orders)
  (scan customers)
  (eq order.customer_id customer.id))

-- After merge join generation with explicit sort orderings
(merge-join
  (sort (scan orders) [customer_id ASC])
  (sort (scan customers) [id ASC])
  (eq order.customer_id customer.id)
  (synchronizer))
```

## Implementation Notes

The CockroachDB optimizer implements this through:

1. **Interesting Orderings Analysis**: Examine interesting orderings from both
   input branches (left and right side of join)

2. **Ordering Compatibility Check**: Verify that join columns are contiguous
   in the proposed orderings and match in direction (both ASC or both DESC)

3. **Functional Dependency Validation**: Use functional dependencies to confirm
   the sort ordering is preserved through intermediate operations (like filters)

4. **Merge Join Construction**: Create MergeJoin operator with:
   - Left input ordered by join columns
   - Right input ordered by join columns
   - Synchronizer for joining sorted streams
   - Optional filters for remaining predicates

## Preconditions

```
1. Both join inputs must have compatible sort orderings
2. Join condition must be an equality predicate (or set of ANDed equalities)
3. Join columns must appear contiguously in the ordering
4. Ordering direction must match between left and right (both ASC or DESC)
5. Functional dependencies must guarantee ordering is maintained
6. No dynamic runtime parameters that break determinism
```

## Restrictions

- Join condition must be on indexed columns or columns with known selectivity
- Complex predicates (functions applied to join columns) may prevent merge join
- For OUTER JOINs, sort stability requirements become more stringent
- Non-equality join conditions cannot use merge join strategy

## Cost Model

```
Merge join cost = sort_cost(left) + sort_cost(right) + merge_cost

Where:
  sort_cost(input) = 0 if already sorted, else O(n log n) comparison cost
  merge_cost = O(n + m) where n, m are input cardinalities

Comparison with nested loop: ε × (n × m) where ε is small constant
Comparison with hash join: + sort overhead, but saves hash table memory
```

## Test Cases

```sql
-- Positive: Both sides naturally sorted on join column
SELECT o.id, o.total, c.name
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.status = 'completed'
-- Table orders is clustered on (status, customer_id)
-- Table customers is clustered on (id)
-- Result: Merge join without additional sorts

-- Positive: One side requires sort, other naturally sorted
SELECT l.id, r.id
FROM left_table l
JOIN right_table r ON l.key = r.key
WHERE l.value > 100
-- right_table already indexed on key, left_table needs sort
-- Result: Merge join with sort on left side only

-- Negative: No compatible orderings available
SELECT *
FROM orders o
JOIN customers c ON o.customer_id = c.id
-- Neither table naturally sorted on join columns
-- Result: Falls back to hash or nested loop join

-- Edge case: OUTER JOIN requires sort stability
SELECT o.*, c.name
FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
-- Merge join still viable but stability requirements stricter
```

## Performance Impact

- **Best case**: Both inputs already sorted → avoid sort cost, O(n+m) merge pass
- **Typical case**: One input sorted → sort one side + O(n+m) merge
- **Benefit magnitude**: 2-30x improvement vs nested loop for tables >10k rows
- **Memory efficiency**: Low memory overhead (constant, not proportional to input size)
- **Cache locality**: Sequential scan pattern excellent for CPU cache

## Related Rules

- CockroachDB Lookup Join Generation (when index available on join column)
- CockroachDB Hash Join (when no ordering available)
- Elide Unnecessary Sorts (remove redundant sorts from merged plan)

## References

- CockroachDB: pkg/sql/opt/xform/join_funcs.go#GenerateMergeJoins
- CockroachDB: pkg/sql/opt/interesting_orderings.go
- PostgreSQL: Merge Join execution overview
- System R: Multi-way joins research
