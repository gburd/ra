# Rule: Calcite IntersectReorderRule

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/intersect-reorder.rra`

## Metadata

- **ID:** `calcite-intersect-reorder`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, mysql
- **Tags:** logical, calcite, intersect, reorder, cardinality
- **Authors:** "RA Contributors"


# Calcite IntersectReorderRule

## Description

Reorders inputs of an INTERSECT to put smaller inputs first. Since
INTERSECT is commutative, processing the smallest input first reduces
the size of intermediate results in hash-based implementations.

**When to apply**: An INTERSECT has multiple inputs with different
estimated cardinalities.

**Why it works**: Hash-based INTERSECT builds a hash table from
the first input and probes with subsequent inputs. Starting with
the smallest input creates the smallest hash table.

**Calcite class**: `org.apache.calcite.rel.rules.IntersectReorderRule`

## Relational Algebra

```algebra
-- Before: large input first
INTERSECT(A[1M rows], B[100 rows], C[10K rows])

-- After: smallest first
INTERSECT(B[100 rows], C[10K rows], A[1M rows])
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-intersect-reorder";
    "(intersect ?inputs)" =>
    "(intersect (sort-by-cardinality ?inputs))"
    if inputs_not_already_sorted("?inputs")
),
```

## Preconditions

```rust
fn applicable(intersect: &Intersect) -> bool {
    let mq = intersect.metadata_query();
    let inputs = intersect.inputs();
    let row_counts: Vec<f64> = inputs.iter()
        .map(|i| mq.row_count(i))
        .collect();
    // Check if not already sorted
    !row_counts.windows(2).all(|w| w[0] <= w[1])
}
```

**Restrictions:**
- Requires cardinality estimates from metadata
- Only beneficial when there are significant size differences
- Preserves ALL vs DISTINCT semantics

## Cost Model

```rust
fn estimated_benefit(
    row_counts: &[f64],
) -> f64 {
    let min_count = row_counts.iter().copied()
        .fold(f64::MAX, f64::min);
    let first_count = row_counts[0];
    if first_count > 0.0 {
        (first_count - min_count) / first_count
    } else {
        0.0
    }
}
```

**Typical benefit**: 10-50% through smaller hash tables.

## Test Cases

```sql
-- Positive: reorder for smaller first
(SELECT id FROM large_table)       -- 1M rows
INTERSECT
(SELECT id FROM small_table);      -- 100 rows
-- Reordered: small_table first
```

```sql
-- Negative: already optimal order
(SELECT id FROM small_table)       -- 100 rows
INTERSECT
(SELECT id FROM large_table);      -- 1M rows
-- Already smallest first
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/IntersectReorderRule.java (commit af6367d)
