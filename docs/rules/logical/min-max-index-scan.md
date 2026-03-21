# Rule: MIN/MAX Index Scan

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/min-max-index-scan.rra`

## Metadata

- **ID:** `min-max-index-scan`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle
- **Tags:** aggregation, min, max, index
- **Authors:** "RA Contributors"


# MIN/MAX Index Scan

## Description

Optimizes MIN/MAX aggregates by using index boundaries instead of full scans.

**When to apply**: MIN/MAX on indexed column with no other aggregates.

**Why it works**: Indexes are sorted; MIN is first entry, MAX is last entry.

## Relational Algebra

```algebra
aggregate[MIN(col)](scan[T])
  -> index_first[idx on col](T)
  where has_index(T, col)

aggregate[MAX(col)](scan[T])
  -> index_last[idx on col](T)
  where has_index(T, col)
```

## Implementation

```rust
rw!("min-from-index";
    "(aggregate (min ?col) (scan ?table))" =>
    "(index-first ?index ?col)"
    if has_btree_index("?table", "?col")
),

rw!("max-from-index";
    "(aggregate (max ?col) (scan ?table))" =>
    "(index-last ?index ?col)"
    if has_btree_index("?table", "?col")
),
```

## Cost Model

```rust
fn benefit(table_rows: u64) -> f64 {
    let scan_cost = table_rows; // Full table scan
    let index_cost = 1.0; // Single index lookup
    (scan_cost as f64 - index_cost) / scan_cost as f64
}
```

**Typical benefit**: 90-99% (O(N) → O(1))

## Test Cases

### Positive: Simple MIN/MAX

```sql
SELECT MIN(created_at) FROM orders;

-- Use index on created_at: read first leaf entry
```

### Positive: MAX with WHERE

```sql
SELECT MAX(price) FROM products WHERE category = 'electronics';

-- Index scan on (category, price)
```

### Negative: Multiple aggregates

```sql
SELECT MIN(price), MAX(price), AVG(price) FROM products;

-- Must scan for AVG anyway
```

## References

- PostgreSQL: Optimize simple MIN/MAX via indexes
- MySQL: Loose Index Scan for MIN/MAX
- Oracle: INDEX FULL SCAN (MIN/MAX)
