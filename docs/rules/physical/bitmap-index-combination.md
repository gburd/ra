# Rule: Bitmap Index Combination (BitmapAnd/BitmapOr)

**Category:** physical/access-path-selection
**File:** `rules/physical/access-path-selection/bitmap-index-combination.rra`

## Metadata

- **ID:** `bitmap-index-combination`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, greenplum
- **Tags:** physical, bitmap, index, combination, multi-index, access-path
- **Authors:** "O'Neil, Patrick", "Graefe, Goetz"


# Bitmap Index Combination

## Description

Combines multiple indexes on the same table using bitmap AND/OR operations
to satisfy complex predicates. Each index scan produces a bitmap of matching
row positions; bitmaps are combined with set operations before fetching
heap tuples. This avoids multiple index lookups per row.

**When to apply**: Queries with AND/OR predicates on different columns,
each having a separate index but no composite index.

**Key insight**: Bitmaps are compact (1 bit per row) and set operations
are fast, making multi-index combination cheaper than repeated index
lookups or a full table scan.

## Relational Algebra

```algebra
-- Before: two separate indexes, complex predicate
sigma[a = 1 AND b > 50](TableScan(R))

-- After: bitmap combination
HeapFetch(BitmapAnd(
    BitmapIndexScan(idx_a, a = 1),
    BitmapIndexScan(idx_b, b > 50)
))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("bitmap-and-combination";
    "(filter (and ?p1 ?p2) (tablescan ?table))" =>
    "(bitmap-heap-scan ?table
        (bitmap-and
            (bitmap-index-scan ?table ?p1)
            (bitmap-index-scan ?table ?p2)))"
    if has_separate_indexes("?table", "?p1", "?p2")
),

rw!("bitmap-or-combination";
    "(filter (or ?p1 ?p2) (tablescan ?table))" =>
    "(bitmap-heap-scan ?table
        (bitmap-or
            (bitmap-index-scan ?table ?p1)
            (bitmap-index-scan ?table ?p2)))"
    if has_separate_indexes("?table", "?p1", "?p2")
),
```

## Preconditions

```rust
fn applicable(pred: &Predicate, table: &Table, catalog: &Catalog) -> bool {
    let sub_preds = pred.conjuncts_or_disjuncts();
    // At least two sub-predicates with separate indexes
    let indexed_count = sub_preds.iter()
        .filter(|p| catalog.has_index_for(table, p))
        .count();
    indexed_count >= 2
        // Combined selectivity must be low enough
        && pred.estimated_selectivity() < 0.3
}
```

**Restrictions:**
- Heap fetch after bitmap can lose ordering (need explicit sort if ORDER BY)
- Very large bitmaps may spill to disk (lossy bitmap)
- Not available on all storage engines (e.g., MySQL InnoDB uses index merge)

## Cost Model

```rust
fn estimated_benefit(
    table_rows: f64,
    combined_selectivity: f64,
    num_indexes: usize,
) -> f64 {
    let bitmap_cost = num_indexes as f64 * table_rows * 0.01; // bitmap scans
    let heap_fetch_cost = table_rows * combined_selectivity * 4.0;
    let full_scan_cost = table_rows;
    full_scan_cost - (bitmap_cost + heap_fetch_cost)
}
```

**Typical benefit**: 20-80% for multi-predicate queries with separate indexes.

## Test Cases

```sql
-- Positive: AND of two indexed columns
CREATE INDEX idx_a ON t(a);
CREATE INDEX idx_b ON t(b);
SELECT * FROM t WHERE a = 1 AND b > 50;
-- BitmapAnd of both index scans

-- Positive: OR of two indexed columns
SELECT * FROM t WHERE a = 1 OR b = 2;
-- BitmapOr of both index scans

-- Negative: composite index exists
CREATE INDEX idx_ab ON t(a, b);
SELECT * FROM t WHERE a = 1 AND b > 50;
-- Single index scan is better
```

## References

- O'Neil, P. "Model 204 Architecture and Performance" (HPTS 1987)
- PostgreSQL: Combining Multiple Indexes documentation
