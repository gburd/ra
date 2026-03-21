# Rule: Index Intersection

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/index-intersection.rra`

## Metadata

- **ID:** `index-intersection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql
- **Tags:** index, intersection, bitmap
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (and ?pred1 ?pred2) (scan ?table))"
    description: "Conjunctive filter with index intersection opportunity"
  - type: "predicate"
    condition: "has_index(?table, columns(?pred1)) && has_index(?table, columns(?pred2))"
    description: "Separate indexes must exist for intersection"
```


# Index Intersection

## Description

Combines multiple single-column indexes using bitmap intersection; avoids need for composite index.

**When to apply**: Multiple AND predicates, each with its own index.

**Why it works**: Each index scan produces bitmap of matching row IDs; bitmap AND operation efficient.

## Relational Algebra

```algebra
filter[col1=v1 AND col2=v2](scan[T])
  -> bitmap_and(index_scan[I1](col1=v1),
                index_scan[I2](col2=v2))
  where has_index(I1, col1) && has_index(I2, col2)
```

## Implementation

```rust
rw!("use-index-intersection";
    "(filter (and (= ?col1 ?val1) (= ?col2 ?val2)) (scan ?table))" =>
    "(bitmap-and (index-scan ?idx1 (= ?col1 ?val1))
                 (index-scan ?idx2 (= ?col2 ?val2)))"
    if has_index("?table", "?col1") && has_index("?table", "?col2")
),
```

## Cost Model

```rust
fn cost(idx1_rows: u64, idx2_rows: u64, table_size: u64) -> f64 {
    let scan_idx1 = (table_size as f64).log2() + idx1_rows as f64;
    let scan_idx2 = (table_size as f64).log2() + idx2_rows as f64;
    let intersection = (idx1_rows.min(idx2_rows) / 64) as f64; // Bitmap ops
    let final_rows = (idx1_rows as f64 * idx2_rows as f64) / table_size as f64;
    scan_idx1 + scan_idx2 + intersection + final_rows
}
```

**Typical benefit**: 40-70% vs full scan when both predicates selective

## Test Cases

### Positive: Two selective predicates

```sql
CREATE INDEX idx_status ON orders(status);
CREATE INDEX idx_priority ON orders(priority);

SELECT * FROM orders
WHERE status = 'pending' AND priority = 'high';

-- Intersect two index scans (5% rows each = 0.25% combined)
```

### Negative: One predicate not selective

```sql
SELECT * FROM orders
WHERE status = 'completed' AND priority = 'normal';

-- status='completed' = 95% of rows: full scan better
```

## References

- PostgreSQL: Bitmap index scan + BitmapAnd
- MySQL: Index merge intersection
- mssql: Index intersection execution plan
