# Rule: Filtered (Partial) Index Matching

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/filtered-index-matching.rra`

## Metadata

- **ID:** `filtered-index-matching`
- **Version:** "1.0.0"
- **Databases:** postgresql, mssql, sqlite
- **Tags:** index, filtered, partial, where-clause
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (scan ?table))"
    description: "Filter matching a partial/filtered index"
  - type: "predicate"
    condition: "has_filtered_index(?table, ?pred)"
    description: "Filtered/partial index exists whose WHERE clause matches the predicate"
```


# Filtered (Partial) Index Matching

## Description

Matches a query predicate against a filtered index (also called a partial
index) whose WHERE clause is implied by the query's WHERE clause. Filtered
indexes are smaller than full indexes because they only include rows
matching the filter, yielding faster lookups and less storage.

**When to apply**: The query's WHERE clause logically implies the index's
filter predicate. For example, if the index filters `WHERE status = 'active'`
and the query has `WHERE status = 'active' AND created > '2025-01-01'`.

**Why it works**: The index is smaller (fewer pages, fewer levels) so lookups
are faster. The optimizer can use the index knowing every row in it satisfies
the filter predicate.

## Relational Algebra

```algebra
sigma[P AND Q](R)
  -> filtered_index_scan[I_filtered](Q)
  where filter_predicate(I) = P AND Q implies P
```

## Implementation

```rust
rw!("filtered-index-match";
    "(filter ?pred (scan ?table))" =>
    "(filtered-index-scan ?idx ?residual_pred)"
    if has_filtered_index("?table") &&
       query_implies_filter("?pred", index_filter("?idx"))
),
```

## Cost Model

```rust
fn cost(filtered_leaf_pages: u64, selectivity: f64) -> f64 {
    // Filtered index is smaller; leaf_pages already reflects the filter
    (filtered_leaf_pages as f64 * selectivity).ceil() * IO_COST
}

fn benefit_vs_full_index(full_pages: u64, filtered_pages: u64) -> f64 {
    (full_pages - filtered_pages) as f64 / full_pages as f64
}
```

**Typical benefit**: 50-95% when the filter excludes a large fraction of rows.

## Test Cases

### Positive: Query matches filter exactly

```sql
-- Partial index: CREATE INDEX idx_active ON users(email) WHERE active = true
SELECT email FROM users WHERE active = true AND email LIKE 'a%';

-- Uses the smaller partial index
```

### Positive: Query implies filter

```sql
-- Partial index: CREATE INDEX idx_recent ON orders(total) WHERE created > '2024-01-01'
SELECT total FROM orders WHERE created > '2025-01-01' AND total > 100;

-- Query's date range implies the index's date range
```

### Negative: Query does not imply filter

```sql
-- Partial index: CREATE INDEX idx_active ON users(email) WHERE active = true
SELECT email FROM users WHERE email LIKE 'a%';

-- No active=true predicate; cannot use the filtered index
```

## References

- PostgreSQL: Partial indexes (CREATE INDEX ... WHERE)
- mssql: Filtered indexes
- SQLite: Partial indexes
- Stonebraker, "Partial Indexes", SIGMOD 1989
