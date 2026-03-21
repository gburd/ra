# Rule: Index-Only Scan Preference

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/index-only-scan-preference.rra`

## Metadata

- **ID:** `index-only-scan-preference`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql, oracle
- **Tags:** index, index-only-scan, visibility-map, covering
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(project ?cols (filter ?pred (scan ?table)))"
    description: "Query eligible for index-only scan"
  - type: "predicate"
    condition: "exists_covering_index(?table, ?pred, ?cols)"
    description: "Covering index must exist for index-only scan"
```


# Index-Only Scan Preference

## Description

Prefers an index-only scan over an index scan + heap fetch whenever the
index contains all columns needed by the query and the table's visibility
map indicates that most pages are all-visible (PostgreSQL) or the engine
supports index-only reads natively.

**When to apply**: The index covers all required columns, and system
statistics indicate that index-only access avoids most heap fetches.

**Why it works**: Index-only scans skip the heap entirely, saving one
random I/O per row. On well-vacuumed PostgreSQL tables (high all-visible
ratio), this eliminates the visibility check overhead as well.

## Relational Algebra

```algebra
pi[cols](sigma[pred](R))
  -> index_only_scan[I](pred)
  where covers(I, cols ∪ pred_cols)
    AND all_visible_ratio(R) > 0.9
```

## Implementation

```rust
rw!("prefer-index-only-scan";
    "(project ?cols (filter ?pred (index-scan ?idx ?table)))" =>
    "(index-only-scan ?idx ?pred)"
    if index_covers("?idx", "?cols", "?pred") &&
       all_visible_ratio("?table") > 0.9
),
```

## Cost Model

```rust
fn cost_index_only(matching_leaf_pages: u64) -> f64 {
    matching_leaf_pages as f64 * IO_COST
}

fn cost_index_plus_heap(matching_leaf_pages: u64, matching_rows: u64) -> f64 {
    matching_leaf_pages as f64 * IO_COST
        + matching_rows as f64 * RANDOM_IO_COST
}

fn benefit(matching_leaf_pages: u64, matching_rows: u64) -> f64 {
    let heap = matching_rows as f64 * RANDOM_IO_COST;
    let total = cost_index_plus_heap(matching_leaf_pages, matching_rows);
    heap / total
}
```

**Typical benefit**: 30-80% depending on selectivity and heap-fetch cost.

## Test Cases

### Positive: Narrow query on covering index

```sql
-- Index: (customer_id) INCLUDE (name)
SELECT name FROM customers WHERE customer_id = 42;

-- Index-only scan: no heap access needed
```

### Positive: Aggregate on indexed column

```sql
-- Index on amount
SELECT SUM(amount) FROM orders WHERE status = 'completed';

-- If (status, amount) index exists: index-only scan
```

### Negative: Table not recently vacuumed

```sql
-- Same query but all_visible_ratio < 0.5
-- PostgreSQL must check heap for visibility
-- Index-only scan degrades to index scan + heap fetch
```

## References

- PostgreSQL: Index-only scans and visibility map
- MySQL: InnoDB covering index optimization
- Oracle: Index-only access path
- Selinger et al., "Access Path Selection", SIGMOD 1979
