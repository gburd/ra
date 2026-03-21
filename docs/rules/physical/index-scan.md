# Rule: Index Scan

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/index-scan.rra`

## Metadata

- **ID:** `index-scan`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, sqlite, duckdb, cockroachdb
- **Tags:** index, scan, b-tree, access-path, range-scan
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (scan ?table))"
    description: "Filter eligible for index scan"
  - type: "predicate"
    condition: "has_index(?table, columns(?pred))"
    description: "Index must exist on the filter column"
  - type: "fact"
    fact_type: "statistics.selectivity"
    table: "?table"
    comparator: "<"
    threshold: 0.15
    optional: true
    description: "Selectivity should be low enough to benefit from index"
```


# Index Scan

## Metadata
- **Rule ID**: `index-scan`
- **Category**: Physical / Index Selection
- **Complexity**: O(log n + k) where n = index size, k = matching rows
- **Introduced**: System R (1970s) -- one of the original access paths
- **Prerequisites**: B-tree or equivalent ordered index on predicate column(s)
- **Alternatives**: sequential-scan, index-only-scan, bitmap-index-scan

## Description

Index scan traverses a B-tree (or similar ordered index) to locate rows
matching a predicate, then fetches the corresponding heap tuples. It
consists of two phases: (1) traverse the index to find matching leaf entries,
and (2) follow row pointers to retrieve full tuples from the heap.

**When to use:**
- Selective predicates (typically < 10-15% of table)
- Equality or range predicates on indexed columns
- ORDER BY on indexed column (avoids separate sort)
- LIMIT queries on indexed column

**Advantages:**
- Dramatically reduces I/O for selective queries
- Provides sorted output if index order matches query
- Sub-millisecond lookup for point queries
- Handles range predicates naturally with B-tree

**Disadvantages:**
- Random I/O for heap tuple fetches (one per matching row)
- Overhead not justified for low-selectivity queries
- Index maintenance cost on writes
- Additional storage for index structure

## Relational Algebra

```
sigma_{pred}(R)
  where index I covers columns in pred
-> IndexScan(I, pred) + HeapFetch(matching_rids)

Cost = B-tree traversal + leaf scan + heap fetches
     = log_b(n) + k/b + k * random_io
  where b = branching factor, k = matching rows
```

## Implementation (egg rewrite rules)

```lisp
;; Convert filter+scan to index scan when index exists
(rewrite (filter ?pred (scan ?table))
  (heap-fetch (index-scan ?index ?pred))
  :if (has-btree-index ?table ?pred ?index)
  :if (< (selectivity ?pred) 0.15))

;; Prefer index scan over sequential scan for selective predicates
(rewrite (filter ?pred (seq-scan ?table))
  (heap-fetch (index-scan ?index ?pred))
  :if (has-btree-index ?table ?pred ?index)
  :if (< (* (selectivity ?pred) (table-pages ?table))
         (table-pages ?table)))

;; Index scan provides sorted output -- eliminate redundant sort
(rewrite (sort ?key (heap-fetch (index-scan ?index ?pred)))
  (heap-fetch (index-scan ?index ?pred))
  :if (index-provides-order ?index ?key))
```

## Cost Model

```rust
pub fn cost_index_scan(
    table_card: u64,
    selectivity: f64,
    index_height: u64,
    correlation: f64,
    hardware: &HardwareModel,
) -> Cost {
    let matching_rows = (table_card as f64 * selectivity) as u64;

    // Index traversal: root to leaf
    let traversal_cost = Cost::io(
        index_height as f64 * hardware.random_page_read_cost(),
    );

    // Leaf page scan for matching entries
    let leaf_pages = matching_rows / hardware.index_entries_per_page();
    let leaf_cost = Cost::io(
        leaf_pages as f64 * hardware.sequential_page_read_cost(),
    );

    // Heap tuple fetches -- correlation determines sequential vs random
    // correlation near 1.0 = clustered, near 0.0 = uncorrelated
    let heap_pages = if correlation > 0.9 {
        // Clustered: sequential page reads
        (matching_rows as f64 / hardware.tuples_per_page()) as u64
    } else {
        // Uncorrelated: each row may hit a different page
        matching_rows.min(hardware.table_pages())
    };
    let heap_cost = if correlation > 0.9 {
        Cost::io(heap_pages as f64 * hardware.sequential_page_read_cost())
    } else {
        Cost::io(heap_pages as f64 * hardware.random_page_read_cost())
    };

    traversal_cost + leaf_cost + heap_cost
}
```

## Test Cases

### Test 1: Point query on primary key
```sql
CREATE TABLE users (id INT PRIMARY KEY, name TEXT, email TEXT);

SELECT * FROM users WHERE id = 42;

-- Expected: IndexScan on PK index
-- Cost: ~3 page reads (index traversal) + 1 heap fetch
-- Vs sequential scan: read entire table
```

### Test 2: Range query on indexed column
```sql
CREATE INDEX idx_orders_date ON orders(order_date);

SELECT * FROM orders
WHERE order_date BETWEEN '2025-01-01' AND '2025-01-31';

-- Expected: IndexScan on idx_orders_date
-- Scan leaf pages for January range, fetch matching heap tuples
-- Beneficial if January is < 15% of all orders
```

### Test 3: Index scan avoids sort
```sql
CREATE INDEX idx_ts ON events(timestamp);

SELECT * FROM events
WHERE timestamp > '2025-01-01'
ORDER BY timestamp
LIMIT 100;

-- Expected: IndexScan provides sorted output
-- No separate sort needed; reads first 100 matching entries
```

### Test 4: Negative -- low selectivity
```sql
SELECT * FROM orders WHERE status IN ('pending', 'shipped', 'delivered');

-- Matches 90% of rows: sequential scan is cheaper
-- Random I/O of index scan exceeds sequential full table scan
```

## Performance Characteristics

| Selectivity | Index Scan | Sequential Scan | Winner |
|-------------|-----------|----------------|--------|
| 0.01% | 3-4 I/Os | Full table | Index |
| 1% | ~100 random I/Os | Full table | Index |
| 10% | ~1000 random I/Os | Full table | Depends on correlation |
| 50% | ~5000 random I/Os | Full table | Sequential |

## References

1. **Selinger et al.**: "Access Path Selection in a Relational Database Management System"
   - SIGMOD 1979, the System R optimizer paper
   - DOI: 10.1145/582095.582099

2. **PostgreSQL Documentation**: Index scanning
   - https://www.postgresql.org/docs/current/indexes-examine.html

3. **Graefe**: "Query Evaluation Techniques for Large Databases"
   - ACM Computing Surveys, 1993, DOI: 10.1145/152610.152611
