# Rule: "CockroachDB Merge Join Generation"

**Category:** physical/join-selection
**File:** `rules/database-specific/cockroachdb/cockroachdb-join-merge-generation.rra`

## Metadata

- **ID:** `cockroachdb-merge-join-generation`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** cockroachdb, join, merge-join, ordering, physical, xform
- **Authors:** "Cockroach Labs", "RA Contributors"


# CockroachDB Merge Join Generation

## Description

CockroachDB's GenerateMergeJoins function explores merge join execution strategies
when both join inputs have compatible interesting orderings. A merge join reads
both inputs in sorted order and matches tuples from each side via a merge step,
requiring O(n+m) comparisons instead of O(n*m) for nested loop joins.

This rule is critical for large join performance and is generated after join
reordering has determined the join tree structure.

**When to apply**: After join reordering, when both left and right inputs can
provide sort orderings that align on the join columns.

**CockroachDB specifics**: Interacts with the interesting ordering framework to
understand what sort orderings are available from each input's potential plans.

## Relational Algebra

```algebra
-- Before: nested loop join (generic)
(inner-join
  (scan $table1)
  (scan $table2)
  (eq $a $b))

-- After: merge join when both sides have compatible orderings
(merge-join
  (sort (scan $table1) $ordering)
  (sort (scan $table2) $ordering)
  (eq $a $b))
```

## Implementation Notes

CockroachDB's xform/join_funcs.go implements this through:
- Examining interesting orderings from both input expressions
- Checking if join columns are contiguous in the proposed ordering
- Verifying functional dependencies that guarantee ordering preservation
- Creating MergeJoin operator with appropriate sync flags

## Preconditions

```
Both inputs have sorted ordering on join columns
Join columns appear contiguously in the ordering
The ordering is compatible with the join condition (equality predicates)
No functional dependency violations that would break sort equivalence
```

## Test Cases

```sql
-- Positive: both sides naturally sorted on join column
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.region = 'us-east' AND c.country = 'US'
-- Orders is clustered on (region, customer_id)
-- Customers is clustered on (country, id)
-- Result: merge join on customer_id/id

-- Negative: join column not in ordering
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
-- If neither table is sorted on join column, uses nested loop

-- Edge case: partial ordering
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
-- If one side provides ordering, nested loop with parameterized inner side
```

## Performance Impact

- Merge joins reduce comparison operations from O(n*m) to O(n+m)
- Benefit significant for tables >10k rows
- Requires sort cost overhead if inputs aren't naturally ordered
- Network I/O patterns favorable for distributed execution

## References

- CockroachDB: Join execution overview
- PostgreSQL: Merge Join description
- db/query/algebra - join operator definitions
