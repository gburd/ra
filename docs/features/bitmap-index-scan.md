# Bitmap Index Scan

**Status**: Implemented (RFC 0018)
**Category**: Physical Optimization
**Priority**: Medium
**Addresses**: Gap #2.1 (multi-predicate queries)

## Overview

Bitmap index scan combines multiple indexes for multi-predicate queries by building bitmaps of matching heap pages, then accessing those pages in physical order. This minimizes random I/O and makes effective use of multiple indexes.

## Motivation

Queries with multiple predicates often have indexes on individual columns but no covering index:

```sql
SELECT * FROM users WHERE age > 25 AND city = 'Seattle' AND status = 'active';
```

With indexes on `(age)`, `(city)`, and `(status)`:
- **Single index scan**: Scans too many rows (low selectivity on one predicate)
- **Sequential scan**: Inefficient if predicates are selective
- **Bitmap scan**: Combines all three indexes efficiently

PostgreSQL bitmap scans are critical for real-world multi-predicate queries.

## Architecture

### Operators

Four new operators in `ra-core/src/algebra.rs`:

1. **BitmapIndexScan**: Scans an index and produces a bitmap of matching heap pages
2. **BitmapAnd**: Combines bitmaps with bitwise AND
3. **BitmapOr**: Combines bitmaps with bitwise OR
4. **BitmapHeapScan**: Fetches heap tuples using the combined bitmap in physical page order

### Two-Phase Execution

**Phase 1: Bitmap Build**
```
Index(age > 25)     -> Bitmap A (pages: 1, 3, 5, 8, ...)
Index(city = 'SEA') -> Bitmap B (pages: 2, 3, 6, 8, ...)
Index(status = 'A') -> Bitmap C (pages: 3, 5, 8, 9, ...)

Bitmap AND(A, B, C) -> Bitmap D (pages: 3, 8)
```

**Phase 2: Heap Scan**
- Read pages 3 and 8 in physical order
- Much faster than random index scans

## Cost Model

Located in `ra-engine/src/cost.rs`:

### BitmapIndexScan Cost
```rust
pub fn bitmap_index_scan_cost(&self, table: &str, selectivity: f64) -> f64
```
- Index scan cost (random I/O)
- Bitmap construction (CPU, very cheap)

### BitmapAnd/BitmapOr Cost
```rust
pub fn bitmap_combine_cost(&self, table: &str, num_bitmaps: usize) -> f64
```
- Bitwise operations run at memory bandwidth speed
- 64 bits per operation
- Extremely cheap

### BitmapHeapScan Cost
```rust
pub fn bitmap_heap_scan_cost(&self, table: &str, combined_selectivity: f64) -> f64
```
- Sequential page access (~4x cheaper than random)
- Recheck condition overhead (CPU)

### Full Bitmap Scan
```rust
pub fn full_bitmap_scan_cost(&self, table: &str, selectivities: &[f64]) -> f64
```
Total cost = Σ(index costs) + combine cost + heap cost

## Optimization Rules

Located in `rules/physical/index-selection/`:

### bitmap-index-selection.rra
Basic bitmap index selection for low-cardinality columns.

### bitmap-index-combining.rra
Combines multiple bitmap index scans with AND/OR operations:

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
```

## Usage Example

### Input Query
```sql
SELECT * FROM orders
WHERE status = 'shipped'
  AND priority = 'high'
  AND region = 'US';
```

### Physical Plan
```
BitmapHeapScan(orders)
  BitmapAnd
    BitmapIndexScan(orders_status_idx, status='shipped')
    BitmapAnd
      BitmapIndexScan(orders_priority_idx, priority='high')
      BitmapIndexScan(orders_region_idx, region='US')
```

### Cost Advantage
- Three index scans: ~30 units each = 90 units
- Bitmap combine: ~0.1 units
- Heap scan (5% selectivity): ~50 units
- **Total**: ~140 units vs 300+ for three separate index scans

## Implementation Details

### E-Graph Integration
The bitmap operators are integrated into the egg-based optimizer in `ra-engine/src/egraph.rs`:

```rust
define_language! {
    pub enum RelLang {
        // ...
        "bitmap-index-scan" = BitmapIndexScan([Id; 3]),
        "bitmap-and" = BitmapAnd(Box<[Id]>),
        "bitmap-or" = BitmapOr(Box<[Id]>),
        "bitmap-heap-scan" = BitmapHeapScan([Id; 3]),
        // ...
    }
}
```

### Conversion Functions
- `add_rel_expr`: Converts `RelExpr` to e-graph representation
- `from_node`: Converts e-graph nodes back to `RelExpr`

## Performance Characteristics

### When Bitmap Scan Wins
- Multiple predicates with moderate selectivity (5-50%)
- Low-cardinality columns (5-100 distinct values)
- Each predicate has an index
- Combined selectivity is low (<10%)

### When Bitmap Scan Loses
- Single predicate (use regular index scan)
- Very high selectivity (>90% - use seq scan)
- High-cardinality columns (use B-tree)
- No indexes available

## Testing

Comprehensive tests in `crates/ra-engine/tests/bitmap_scan_test.rs`:

1. **Operator creation**: Verify all four operators can be created
2. **Cost model**: Test all cost functions return finite, positive values
3. **Cost comparison**: Bitmap scan cheaper than multiple sequential scans
4. **E-graph roundtrip**: Conversion to/from e-graph preserves structure
5. **Multi-predicate**: Full integration test with three predicates

## References

- RFC 0018: Bitmap Index Scan
- PostgreSQL: BitmapAnd, BitmapOr, BitmapHeapScan nodes
- `rules/physical/index-selection/bitmap-index-*.rra`
- `crates/ra-core/src/algebra.rs:226-270`
- `crates/ra-engine/src/cost.rs:297-403`
- `crates/ra-engine/src/egraph.rs` (RelLang definition and conversions)

## Future Enhancements

1. **Lossy bitmaps**: For very large result sets, compress bitmaps
2. **Parallel bitmap build**: Build multiple bitmaps concurrently
3. **Adaptive recheck**: Skip recheck when index is covering
4. **Bitmap caching**: Cache bitmaps for repeated predicates
5. **Cost calibration**: Tune cost model based on actual execution feedback
