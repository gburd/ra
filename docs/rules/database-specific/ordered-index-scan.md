# Rule: MonetDB Ordered Index (Persistent Sort)

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/ordered-index-scan.rra`

## Metadata

- **ID:** `monetdb-ordered-index-scan`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, ordered-index, persistent, sort, oidx
- **Authors:** "RA Contributors"


# MonetDB Ordered Index (Persistent Sort)

## Description

MonetDB supports ordered indexes (OIDX) which maintain a persistent
sorted mapping from column values to OIDs.  The ordered index enables
efficient range lookups and ORDER BY without re-sorting.  Unlike
cracking (which adapts lazily), an ordered index is built explicitly
and maintained across queries.

**When to apply**: A column is frequently queried with range
predicates or ORDER BY and an ordered index has been created.

**Why it works**: Binary search on the sorted index locates the range
boundaries in O(log n), then the qualifying OIDs are read
sequentially.  This avoids full column scans for selective range
queries.

**Database version**: MonetDB 11.25+

## Relational Algebra

```algebra
-- Before: full scan + filter
sigma[age BETWEEN 20 AND 30](scan(users.age))

-- After: ordered index lookup
oidx_range_scan(users.age, 20, 30)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-ordered-index-scan";
    "(filter (between ?col ?lo ?hi) (scan ?table))" =>
    "(oidx-range-scan ?table ?col ?lo ?hi)"
    if is_database("monetdb")
    if has_ordered_index("?col")
),
```

## Preconditions

```rust
fn applicable(column: &Column) -> bool {
    column.has_ordered_index()
}
```

**Restrictions:**
- Ordered index must be explicitly created
- Updates require index maintenance
- Only one ordered index per column
- Memory overhead for the OID mapping array

## Cost Model

```rust
fn estimated_benefit(
    total_rows: f64,
    selectivity: f64,
) -> f64 {
    let scan_cost = total_rows * 0.001;
    let oidx_cost = total_rows.log2() * 0.001
        + total_rows * selectivity * 0.001;
    scan_cost - oidx_cost
}
```

**Typical benefit**: 10-100x for selective range queries.

## Test Cases

```sql
-- Positive: range query with ordered index
CREATE ORDERED INDEX ON users(age);
SELECT * FROM users WHERE age BETWEEN 20 AND 30;
-- Binary search + sequential OID read
```

```sql
-- Negative: no ordered index
SELECT * FROM logs WHERE severity > 3;
-- Full scan or cracking; no ordered index
```

## References

MonetDB: Ordered index documentation
Source: monetdb5/modules/kernel/bat5.c (ordered index operations)
