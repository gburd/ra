# Rule: Covering Index Selection

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/covering-index-selection.rra`

## Metadata

- **ID:** `covering-index-selection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** index, covering, index-only-scan
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(project ?cols (filter ?pred (scan ?table)))"
    description: "Projection + filter eligible for index-only scan"
  - type: "predicate"
    condition: "exists_covering_index(?table, ?pred, ?cols)"
    description: "Index must cover all query columns (predicates and projections)"
```


# Covering Index Selection

## Description

Selects index that includes all query columns, enabling index-only scan without table access.

**When to apply**: Query columns fully covered by index (includes both predicates and projections).

**Why it works**: Avoids table lookups; all data retrieved from index; dramatically reduces I/O.

## Relational Algebra

```algebra
project[cols](filter[pred](scan[T]))
  -> index_only_scan[I](pred, cols)
  where index_covers(I, cols $\cup$ columns(pred))
```

## Implementation

```rust
rw!("select-covering-index";
    "(project ?cols (filter ?pred (scan ?table)))" =>
    "(index-only-scan ?index ?pred ?cols)"
    if exists_covering_index("?table", "?pred", "?cols")
),
```

## Cost Model

```rust
fn cost(index_pages: u64, matching_rows: u64) -> f64 {
    let index_scan = index_pages as f64;
    let no_table_access = 0.0; // Key benefit
    index_scan
}

fn benefit_over_index_scan(idx_pages: u64, matches: u64, table_pages: u64) -> f64 {
    let with_lookup = idx_pages as f64 + matches as f64;
    let covering = idx_pages as f64;
    (with_lookup - covering) / with_lookup
}
```

**Typical benefit**: 60-90% when avoiding table access

## Test Cases

### Positive: All columns in index

```sql
CREATE INDEX idx_user_email_name ON users(email, name);

SELECT email, name FROM users WHERE email LIKE 'admin%';

-- Index covers email + name: no table access needed
```

### Positive: Index includes WHERE and SELECT columns

```sql
CREATE INDEX idx_order_date_total ON orders(date, total);

SELECT date, total FROM orders WHERE date >= '2025-01-01';

-- Covering index: both predicate and projection columns
```

### Negative: Missing projection column

```sql
CREATE INDEX idx_product_category ON products(category);

SELECT category, name FROM products WHERE category = 'Electronics';

-- Index lacks 'name': must access table
```

## References

- PostgreSQL: Index-only scans
- MySQL: Covering indexes
- mssql: Covering indexes with INCLUDE
