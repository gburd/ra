# Rule: Reverse Index Scan

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/reverse-index-scan.rra`

## Metadata

- **ID:** `reverse-index-scan`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle
- **Tags:** index, reverse, descending
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(sort (desc ?col) (scan ?table))"
    description: "Descending sort on indexed column"
  - type: "predicate"
    condition: "has_btree_index(?table, ?col)"
    description: "B-tree index must exist (supports reverse traversal)"
```


# Reverse Index Scan

## Description

Scans B-tree index backwards to satisfy DESC ordering or find MAX values efficiently.

**When to apply**: ORDER BY DESC or finding maximum values.

**Why it works**: B-tree traversable in both directions; backwards scan avoids sort.

## Relational Algebra

```algebra
sort[key DESC](scan[T])
  -> reverse_index_scan[I(key)]
  where has_index(I, key)

limit[1](sort[key DESC](scan[T]))
  -> reverse_index_scan[I(key), limit=1]  // MAX without full scan
```

## Implementation

```rust
rw!("use-reverse-index-scan";
    "(sort (desc ?key) (scan ?table))" =>
    "(reverse-index-scan ?index)"
    if has_index("?table", "?key")
),

rw!("max-via-reverse-scan";
    "(limit 1 (sort (desc ?key) (scan ?table)))" =>
    "(reverse-index-scan ?index :limit 1)"
    if has_index("?table", "?key")
),
```

## Cost Model

```rust
fn cost(index_height: usize, limit: Option<u64>) -> f64 {
    match limit {
        Some(k) => index_height as f64 + k as f64,
        None => index_height as f64 + (index_height as f64 * 1000.0), // Full scan
    }
}
```

**Typical benefit**: 30-70% vs sort, 90%+ for MAX/MIN queries

## Test Cases

### Positive: DESC ordering

```sql
CREATE INDEX idx_timestamp ON events(timestamp);

SELECT * FROM events ORDER BY timestamp DESC LIMIT 100;

-- Reverse index scan: no sort needed
```

### Positive: MAX value

```sql
SELECT MAX(price) FROM products;

-- Reverse scan index on price, return first value
```

### Negative: Complex ORDER BY

```sql
SELECT * FROM orders
ORDER BY YEAR(order_date) DESC, customer_id ASC;

-- Cannot use simple reverse scan: need sort
```

## References

- PostgreSQL: Backward index scan
- MySQL: Reverse index scan optimization
- Oracle: Index range scan descending
