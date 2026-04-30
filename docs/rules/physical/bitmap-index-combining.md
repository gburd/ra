# Rule: Bitmap Index Combining with AND/OR

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/bitmap-index-combining.rra`

## Metadata

- **ID:** `bitmap-index-combining`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle
- **Tags:** index, bitmap, combining, and, or, multi-predicate
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (and ?pred1 ?pred2) (scan ?table))"
    description: "Conjunctive filter on table with bitmap indexes"
  - type: "predicate"
    condition: "has_bitmap_index(?table, columns(?pred1)) && has_bitmap_index(?table, columns(?pred2))"
    description: "Bitmap indexes must exist on both predicate columns"
  - type: "capability"
    database: "current"
    requires: "bitmap_index"
    description: "Database supports bitmap indexes"
```


# Bitmap Index Combining with AND/OR

## Description

Combines multiple bitmap index scans using bitwise AND and OR
operations to efficiently evaluate complex multi-predicate queries.
Each bitmap index scan produces a bitmap of matching row positions;
these bitmaps are combined before accessing the heap.

**When to apply**: A query has multiple predicates on different
low-cardinality columns, each with its own bitmap index.

**Why it works**: Bitmap AND/OR operations run at memory bandwidth
speed (64 bits per operation). Combining bitmaps first, then
accessing only the matching rows, avoids redundant heap fetches.

## Relational Algebra

```algebra
-- AND combination
filter[p1 AND p2 AND p3](scan[T])
  -> bitmap_heap_scan(
       bitmap_and(
         bitmap_scan[I1](p1),
         bitmap_scan[I2](p2),
         bitmap_scan[I3](p3)))

-- OR combination
filter[p1 OR p2](scan[T])
  -> bitmap_heap_scan(
       bitmap_or(
         bitmap_scan[I1](p1),
         bitmap_scan[I2](p2)))
```

## Implementation

```rust
rw!("bitmap-index-and-combine";
    "(filter (and ?p1 (and ?p2 ?p3)) (scan ?table))" =>
    "(bitmap-heap-scan
        (bitmap-and
            (bitmap-index-scan ?i1 ?p1)
            (bitmap-and
                (bitmap-index-scan ?i2 ?p2)
                (bitmap-index-scan ?i3 ?p3))))"
    if has_bitmap_indexes("?table", "?p1", "?p2", "?p3")
),

rw!("bitmap-index-or-combine";
    "(filter (or ?p1 ?p2) (scan ?table))" =>
    "(bitmap-heap-scan
        (bitmap-or
            (bitmap-index-scan ?i1 ?p1)
            (bitmap-index-scan ?i2 ?p2)))"
    if has_bitmap_indexes("?table", "?p1", "?p2")
),
```

## Cost Model

```rust
fn cost(
    bitmap_sizes: &[u64],
    result_rows: u64,
    table_pages: u64,
) -> f64 {
    let bitmap_scan: f64 = bitmap_sizes.iter()
        .map(|&b| b as f64 / 64.0)
        .sum();
    let combine_cost = bitmap_scan * 0.1; // Bitwise ops
    let pages_to_fetch = (result_rows as f64 / 100.0)
        .min(table_pages as f64);
    bitmap_scan + combine_cost + pages_to_fetch
}
```

**Typical benefit**: 30-80% for multi-predicate queries on indexed columns.

## Test Cases

### Positive: AND of three bitmap predicates

```sql
SELECT * FROM orders
WHERE status = 'shipped'
  AND priority = 'high'
  AND region = 'US';

-- Three bitmap scans ANDed together; result fetched once
```

### Positive: OR combination

```sql
SELECT * FROM products
WHERE category = 'Electronics'
   OR category = 'Appliances';

-- Two bitmap scans ORed; union of matching rows
```

### Negative: High-selectivity single predicate

```sql
SELECT * FROM users WHERE id = 42;

-- Single-row lookup: regular B-tree index scan is faster
```

## References

- PostgreSQL: BitmapAnd and BitmapOr nodes
- Oracle: Bitmap index combining
- IndexType::Bitmap in ra-stats-advanced/src/index_types.rs
