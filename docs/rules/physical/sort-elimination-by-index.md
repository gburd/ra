# Rule: Sort Elimination by Index Order

**Category:** physical/sort
**File:** `rules/physical/sort/sort-elimination-by-index.rra`

## Metadata

- **ID:** `sort-elimination-by-index`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, sqlite
- **Tags:** physical, sort, index, elimination, interesting-orders
- **Authors:** "Selinger et al., System R"


# Sort Elimination by Index Order

## Description

Eliminates a Sort operator when the input is already ordered by virtue
of an index scan. If the query requires ORDER BY (a, b) and an index
on (a, b) is used for the scan, no additional sort is needed. This is
the "interesting orders" optimization from System R.

**When to apply**: Input is an index scan whose key order matches the
required sort order.

## Relational Algebra

```algebra
-- Before
Sort[a, b](IndexScan[idx_ab](R))

-- After (sort removed)
IndexScan[idx_ab](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw\!("sort-elimination-by-index";
    "(sort ?order (index-scan ?idx ?table))" =>
    "(index-scan ?idx ?table)"
    if index_provides_order("?idx", "?order")
),
```

## Preconditions

```rust
fn applicable(sort: &Sort, scan: &IndexScan) -> bool {
    let idx_order = scan.index().key_columns();
    sort.required_order().is_prefix_of(&idx_order)
}
```

## Cost Model

```rust
fn estimated_benefit(rows: f64) -> f64 {
    rows * (rows as f64).log2() * 0.001 // Sort cost saved
}
```

## Test Cases

```sql
-- Positive: index matches ORDER BY
CREATE INDEX idx_name ON users(last_name, first_name);
SELECT * FROM users ORDER BY last_name, first_name;
-- Index scan provides order, sort eliminated

-- Positive: prefix match
SELECT * FROM users ORDER BY last_name;
-- Index on (last_name, first_name) provides last_name order

-- Negative: order mismatch
SELECT * FROM users ORDER BY first_name;
-- Index on (last_name, first_name) does not provide first_name order
```

## References

- Selinger et al., "Access Path Selection in a Relational Database Management System", SIGMOD 1979
- Simmen, D., Shekita, E. & O'Keefe, T., "Fundamental Techniques for Order Optimization", SIGMOD 1996
