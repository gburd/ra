# Rule: Multi-Column Index Selection

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/multi-column-index-selection.rra`

## Metadata

- **ID:** `multi-column-index-selection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** index, composite, multi-column
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (scan ?table))"
    description: "Multi-column predicate with composite index"
  - type: "predicate"
    condition: "has_multi_column_index(?table, columns(?pred))"
    description: "Multi-column index must exist covering predicate columns"
```


# Multi-Column Index Selection

## Description

Selects composite index matching query predicates in column order; maximizes index utilization.

**When to apply**: Query with multiple equality predicates matching index prefix.

**Why it works**: B-tree indexes efficient when predicates match column order; narrows search space maximally.

## Relational Algebra

```algebra
filter[col1=v1 AND col2=v2](scan[T])
  -> index_scan[I(col1, col2)](col1=v1 AND col2=v2)
  where prefix_match(I, [col1, col2])
```

## Implementation

```rust
rw!("select-composite-index";
    "(filter (and (= ?col1 ?val1) (= ?col2 ?val2)) (scan ?table))" =>
    "(index-scan ?index (and (= ?col1 ?val1) (= ?col2 ?val2)))"
    if has_index("?table", ["?col1", "?col2"])
),
```

## Cost Model

```rust
fn selectivity(col1_sel: f64, col2_sel: f64) -> f64 {
    col1_sel * col2_sel // Combined selectivity
}

fn cost(index_height: usize, selectivity: f64, table_size: u64) -> f64 {
    let index_lookup = index_height as f64;
    let scan_rows = table_size as f64 * selectivity;
    index_lookup + scan_rows
}
```

**Typical benefit**: 40-80% vs full scan

## Test Cases

### Positive: Equality predicates match index order

```sql
CREATE INDEX idx_order_date_customer ON orders(date, customer_id);

SELECT * FROM orders
WHERE date = '2025-01-15' AND customer_id = 12345;

-- Index fully utilized: narrows to specific date and customer
```

### Negative: Predicates skip first column

```sql
CREATE INDEX idx_order_date_customer ON orders(date, customer_id);

SELECT * FROM orders WHERE customer_id = 12345;

-- Cannot use index: skips first column (date)
```

## References

- PostgreSQL: Multi-column indexes
- MySQL: Composite index optimization
