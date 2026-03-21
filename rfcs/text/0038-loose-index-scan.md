# RFC 0038: Loose Index Scan (Skip Scan)

## Status
PROPOSED

## Summary
Implement loose index scan optimization for efficiently executing DISTINCT queries and GROUP BY operations on low-cardinality indexed columns by skipping duplicate values in the index.

## Motivation
For queries like `SELECT DISTINCT status FROM orders` where status has low cardinality, a full index scan reads many duplicate values. Loose index scan can skip to the next distinct value, providing 10-100x speedup. This optimization is available in MySQL and discussed extensively in PostgreSQL community.

## Design

### Core Concept

Instead of scanning all index entries, skip to the next distinct value:

```sql
-- Traditional approach: scan all rows
SELECT DISTINCT category FROM products;

-- Loose index scan: jump between distinct values
-- Effectively becomes multiple index seeks
```

### Implementation Strategy

```rust
pub struct LooseIndexScan {
    index: IndexRef,
    distinct_columns: Vec<ColumnId>,
    skip_duplicates: bool,
}

impl LooseIndexScan {
    fn next_distinct(&mut self) -> Option<Row> {
        let current_value = self.current()?;
        // Skip to first entry > current_value
        self.index.seek_greater_than(current_value);
        self.current()
    }
}
```

### Applicability Conditions

1. **Index Requirements**:
   - Index must have DISTINCT/GROUP BY columns as prefix
   - B-tree index structure (not hash)
   - Statistics indicate low cardinality

2. **Query Patterns**:
   - `SELECT DISTINCT col FROM table`
   - `SELECT col, COUNT(*) FROM table GROUP BY col`
   - `SELECT MIN(col) FROM table GROUP BY other_col`

### Cost Model

```rust
fn loose_index_scan_cost(
    distinct_values: f64,
    total_rows: f64,
    index_height: f64,
) -> Cost {
    // Cost of seeks to distinct values
    let seek_cost = distinct_values * index_height * random_page_cost;
    // Much cheaper than scanning all rows
    seek_cost
}
```

## Implementation Plan

1. Detect applicable query patterns
2. Implement index skip operation
3. Add loose scan physical operator
4. Update cost model for skip scan
5. Add statistics for cardinality estimation
6. Optimize for multi-column scenarios

## Alternatives Considered

- **Hash Aggregate**: Requires reading all data first
- **Bitmap Index Scan**: Still reads all index entries
- **Materialized View**: Static, requires maintenance

## Success Criteria

- 10x+ speedup for low cardinality DISTINCT queries
- Correct results for all GROUP BY patterns
- Automatic detection of applicable scenarios
- Graceful fallback when not beneficial