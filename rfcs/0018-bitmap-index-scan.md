# RFC 0018: Bitmap Index Scan

- Start Date: 2026-03-20
- Author: System
- Status: Implemented

## Summary

Add bitmap index scan support to combine multiple indexes for multi-predicate queries, accessing heap pages in physical order to minimize random I/O.

## Motivation

Queries with multiple predicates often have indexes on individual columns but no covering index. Example:
```sql
SELECT * FROM users WHERE age > 25 AND city = 'Seattle' AND status = 'active';
```

With indexes on (age), (city), (status):
- **Index scan on one column**: Scans too many rows
- **Sequential scan**: Inefficient if selective
- **Bitmap scan**: Combines all three indexes efficiently

PostgreSQL bitmap scans are critical for real-world queries. RA currently has no bitmap scan modeling.

## Guide-level explanation

Bitmap scan works in two phases:

1. **Bitmap Build**: Scan indexes to build bitmap of matching heap pages
   ```
   Index(age > 25)     -> Bitmap A (pages: 1, 3, 5, 8, ...)
   Index(city = 'SEA') -> Bitmap B (pages: 2, 3, 6, 8, ...)
   Index(status = 'A') -> Bitmap C (pages: 3, 5, 8, 9, ...)

   Bitmap AND(A, B, C) -> Bitmap D (pages: 3, 8)
   ```

2. **Heap Scan**: Read heap pages in physical order
   - Pages 3, 8 read sequentially
   - Much faster than random index scans

## Technical design

### New Operators

```rust
pub enum RelExpr {
    // ...
    BitmapIndexScan {
        table: String,
        index: String,
        predicate: Expr,
    },
    BitmapAnd {
        inputs: Vec<Box<RelExpr>>,
    },
    BitmapOr {
        inputs: Vec<Box<RelExpr>>,
    },
    BitmapHeapScan {
        table: String,
        bitmap: Box<RelExpr>,
        recheck_cond: Option<Expr>,
    },
}
```

### Optimization Rules

```yaml
# rules/access-path/bitmap-and.rra
name: bitmap-and-combination
pattern: |
  Filter(AND(p1, p2), Scan(table))
condition: |
  has_index(table, columns_in(p1)) &&
  has_index(table, columns_in(p2))
transform: |
  BitmapHeapScan(table,
    BitmapAnd([
      BitmapIndexScan(table, index_for(p1), p1),
      BitmapIndexScan(table, index_for(p2), p2)
    ])
  )
```

### Cost Model

```rust
impl CostModel {
    fn bitmap_scan_cost(&self, predicates: &[Expr], table: &str) -> Cost {
        // 1. Index scan costs
        let index_costs: Vec<_> = predicates
            .iter()
            .map(|p| self.index_scan_cost(p, table))
            .collect();

        // 2. Bitmap AND selectivity
        let combined_sel = predicates
            .iter()
            .map(|p| self.selectivity(p, table))
            .product();

        // 3. Heap page access (sequential, not random)
        let table_pages = self.stats.page_count(table);
        let pages_accessed = table_pages as f64 * combined_sel;
        let heap_cost = pages_accessed * self.params.seq_page_cost;

        // 4. Total
        Cost::new(
            index_costs.iter().map(|c| c.io).sum::<f64>() + heap_cost,
            0.0,  // CPU negligible
            0.0,  // Network none
            0,    // Memory for bitmap
        )
    }
}
```

## Prior art

- PostgreSQL: BitmapAnd, BitmapOr, BitmapHeapScan nodes
- SQL Server: Bitmap hash joins
- Oracle: Bitmap index access

## Implementation plan

- Week 1: Add operators and tree construction
- Week 2: Implement optimization rules
- Week 3: Cost model integration and testing

## Gap addressed

Gap #2.1 (Medium severity) from postgres-planner-gaps.md
