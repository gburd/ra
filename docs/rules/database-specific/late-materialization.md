# Rule: MonetDB Late Materialization

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/late-materialization.rra`

## Metadata

- **ID:** `monetdb-late-materialization`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, late-materialization, columnar, projection
- **Authors:** "RA Contributors"


# MonetDB Late Materialization

## Description

MonetDB defers fetching non-essential columns until the final
projection.  During filtering and joining, only the join/filter
columns and OID vectors are processed.  Non-essential columns are
fetched only for qualifying rows at the end, minimizing memory
bandwidth.

**When to apply**: A query filters or joins on a subset of columns
and projects additional columns that are not used in intermediate
operations.

**Why it works**: In a columnar store, each column access costs
proportional to the column width times rows accessed.  Late
materialization ensures wide columns (strings, BLOBs) are only read
for rows that survive all filters and joins.

**Database version**: MonetDB 5+ (fundamental design principle)

## Relational Algebra

```algebra
-- Before: early materialization (fetch all columns first)
pi[name, email, bio](
    sigma[status = 'active'](
        scan(users[id, name, email, bio, status])))

-- After: late materialization
qualifying_oids = sigma[status = 'active'](scan(users.status))
pi[name, email, bio](fetch(users, qualifying_oids))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-late-materialization";
    "(project ?output_cols
        (filter ?pred
            (scan ?table)))" =>
    "(fetch ?table ?output_cols
        (filter ?pred
            (scan-columns ?table (pred-columns ?pred))))"
    if is_database("monetdb")
    if output_exceeds_filter_columns("?output_cols", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    output_cols: &[Column],
    filter_cols: &[Column],
) -> bool {
    let deferred: Vec<_> = output_cols.iter()
        .filter(|c| !filter_cols.contains(c))
        .collect();
    !deferred.is_empty()
}
```

**Restrictions:**
- Fetch phase requires random access to deferred columns by OID,
  which is efficient for columnar stores with dense OID ranges
- If selectivity is very low (many rows survive), late materialization
  adds overhead from the fetch phase
- MonetDB's OID-based storage makes fetch by OID nearly free

## Cost Model

```rust
fn estimated_benefit(
    total_rows: f64,
    surviving_rows: f64,
    deferred_col_width: f64,
) -> f64 {
    let early_cost = total_rows * deferred_col_width * 0.001;
    let late_cost = surviving_rows * deferred_col_width * 0.001;
    early_cost - late_cost
}
```

**Typical benefit**: 2-50x for selective queries on wide tables.

## Test Cases

```sql
-- Positive: selective filter on narrow column, wide output
SELECT name, email, bio FROM users WHERE status = 'active';
-- Only status column scanned during filter; name, email, bio
-- fetched only for active users
```

```sql
-- Negative: all columns needed for filter
SELECT * FROM users WHERE name LIKE '%John%' AND bio LIKE '%eng%';
-- Both wide columns needed in filter; late materialization less useful
```

## References

Abadi, D. et al. "Materialization Strategies in a Column-Oriented
DBMS" (ICDE 2007)
MonetDB: Column store architecture documentation
