# Rule: Index Union

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/index-union.rra`

## Metadata

- **ID:** `index-union`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql
- **Tags:** index, union, bitmap, or
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (or ?pred1 ?pred2) (scan ?table))"
    description: "Disjunctive filter with index union opportunity"
  - type: "predicate"
    condition: "has_index(?table, columns(?pred1)) && has_index(?table, columns(?pred2))"
    description: "Indexes must exist on both disjunct columns"
```


# Index Union

## Description

Combines multiple index scans with bitmap OR for disjunctive (OR) predicates.

**When to apply**: OR predicates where each branch can use an index.

**Why it works**: Each index scan efficient; bitmap OR combines results without duplicates.

## Relational Algebra

```algebra
filter[col1=v1 OR col2=v2](scan[T])
  -> bitmap_or(index_scan[I1](col1=v1),
               index_scan[I2](col2=v2))
```

## Implementation

```rust
rw!("use-index-union";
    "(filter (or (= ?col1 ?val1) (= ?col2 ?val2)) (scan ?table))" =>
    "(bitmap-or (index-scan ?idx1 (= ?col1 ?val1))
                (index-scan ?idx2 (= ?col2 ?val2)))"
    if has_index("?table", "?col1") && has_index("?table", "?col2")
),
```

## Cost Model

```rust
fn cost(idx1_rows: u64, idx2_rows: u64) -> f64 {
    let scan_both = idx1_rows as f64 + idx2_rows as f64;
    let union_op = (idx1_rows.max(idx2_rows) / 64) as f64;
    let table_lookups = (idx1_rows + idx2_rows) as f64 * 0.8; // Some overlap
    scan_both + union_op + table_lookups
}
```

**Typical benefit**: 30-60% vs full scan for OR predicates

## Test Cases

### Positive: OR with multiple indexes

```sql
CREATE INDEX idx_status ON orders(status);
CREATE INDEX idx_date ON orders(created_date);

SELECT * FROM orders
WHERE status = 'urgent' OR created_date < '2025-01-01';

-- Union two index scans
```

### Negative: One branch scans most rows

```sql
SELECT * FROM users
WHERE email = 'admin@example.com' OR active = true;

-- active=true is 90%: full scan better
```

## References

- PostgreSQL: BitmapOr node
- MySQL: Index merge union
