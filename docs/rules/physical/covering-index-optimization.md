# Rule: Covering Index Optimization

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/covering-index-optimization.rra`

## Metadata

- **ID:** `covering-index-optimization`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql, oracle, duckdb
- **Tags:** index, covering, index-only-scan, included-columns
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(project ?cols (filter ?pred (scan ?table)))"
    description: "Projection + filter on table with covering index"
  - type: "predicate"
    condition: "exists_covering_index(?table, ?pred, ?cols)"
    description: "Index must cover all projected and predicate columns"
```


# Covering Index Optimization

## Description

Detects when all columns referenced by a query are available in an index
(key columns plus INCLUDE columns), enabling an index-only scan that
eliminates heap/table fetches entirely.

**When to apply**: Every column in SELECT, WHERE, and ORDER BY is present
in a single index (either as a key column or an included column).

**Why it works**: Heap fetches are the dominant cost of secondary index
scans. Eliminating them turns each matching row into a single leaf-page
read instead of a leaf-page read plus a random heap-page read.

## Relational Algebra

```algebra
pi[cols](sigma[pred](R))
  -> index_only_scan[I_covering](pred)
  where is_covering(I, cols $\cup$ pred_columns)
```

## Implementation

```rust
rw!("covering-index-scan";
    "(project ?cols (filter ?pred (scan ?table)))" =>
    "(index-only-scan ?idx ?pred)"
    if has_covering_index("?table", "?cols", "?pred")
),
```

## Cost Model

```rust
fn cost_index_only(leaf_pages: u64, selectivity: f64) -> f64 {
    (leaf_pages as f64 * selectivity).ceil() * SEQUENTIAL_IO_COST
}

fn cost_with_heap_fetch(leaf_pages: u64, rows: u64, selectivity: f64) -> f64 {
    let leaf = (leaf_pages as f64 * selectivity).ceil() * SEQUENTIAL_IO_COST;
    let heap = (rows as f64 * selectivity).ceil() * RANDOM_IO_COST;
    leaf + heap
}

fn benefit(leaf_pages: u64, rows: u64, selectivity: f64) -> f64 {
    let with_fetch = cost_with_heap_fetch(leaf_pages, rows, selectivity);
    let without = cost_index_only(leaf_pages, selectivity);
    (with_fetch - without) / with_fetch
}
```

**Typical benefit**: 30-80% depending on selectivity and row width.

## Test Cases

### Positive: All columns in index

```sql
-- Index: (customer_id) INCLUDE (order_date, total)
SELECT order_date, total FROM orders WHERE customer_id = 42;

-- Index-only scan: no heap fetch needed
```

### Positive: COUNT with indexed column

```sql
-- Index on status
SELECT COUNT(*) FROM orders WHERE status = 'shipped';

-- COUNT only needs existence, not row data
```

### Negative: Extra column not in index

```sql
-- Index: (customer_id) INCLUDE (order_date)
SELECT order_date, total FROM orders WHERE customer_id = 42;

-- total not in index: heap fetch still required
```

## References

- PostgreSQL: Index-only scans and visibility map
- mssql: INCLUDE columns in nonclustered indexes
- MySQL: InnoDB covering indexes
- Oracle: Index-organized tables as covering structures
