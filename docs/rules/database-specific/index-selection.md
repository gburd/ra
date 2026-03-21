# Rule: Apache Derby Cost-Based Index Selection

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/index-selection.rra`

## Metadata

- **ID:** `derby-index-selection`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, index, selection, cost-based, access-path
- **Authors:** "RA Contributors"


# Apache Derby Cost-Based Index Selection

## Description

Derby's optimizer evaluates multiple access paths for each table and
selects the cheapest.  Access paths include full table scan, index
scan (with start/stop keys), and multi-column index scan.  The cost
model accounts for I/O cost (pages read), CPU cost (rows evaluated),
and the selectivity of predicates.

**When to apply**: A table has one or more indexes and the query has
predicates or join conditions that could use them.

**Why it works**: The right index can reduce I/O from reading the
entire table to reading only the qualifying pages.  The optimizer's
cost model prevents choosing an index when a full scan is cheaper
(e.g., very low selectivity).

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Access path options for sigma[a = 5 AND b > 10](T):
-- 1. Full table scan + filter: O(N)
-- 2. Index on (a): O(k) where k = rows with a=5
-- 3. Index on (a, b): O(j) where j = rows with a=5 AND b>10
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-index-selection";
    "(filter ?pred (scan ?table))" =>
    "(filter ?remaining
        (index-scan ?table ?idx ?start ?stop))"
    if is_database("derby")
    if best_index_for("?table", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    table: &Table,
    predicate: &Predicate,
) -> bool {
    table.indexes().iter().any(|idx| {
        predicate.can_use_index(idx)
    })
}
```

**Restrictions:**
- Derby uses a simple cost model based on row counts and page sizes
- Statistics must be up-to-date (`CALL SYSCS_UTIL.SYSCS_UPDATE_STATISTICS`)
- Stale statistics can lead to poor index choices
- Derby supports compound indexes with start/stop key optimization

## Cost Model

```rust
fn estimated_benefit(
    table_pages: f64,
    index_pages: f64,
    selectivity: f64,
) -> f64 {
    let scan_cost = table_pages * 1.0; // sequential I/O
    let index_cost = (index_pages * selectivity) * 1.5; // random I/O
    scan_cost - index_cost
}
```

**Typical benefit**: 10-10000x for highly selective predicates on
indexed columns.

## Test Cases

```sql
-- Positive: selective predicate with index
CREATE INDEX idx_email ON users(email);
SELECT * FROM users WHERE email = 'alice@example.com';
-- Index scan on idx_email; one page read
```

```sql
-- Negative: non-selective predicate
SELECT * FROM users WHERE active = true;
-- 90% of users active; full scan cheaper than index
```

## References

Apache Derby: "Query Optimization" in Technical Architecture
Source: org.apache.derby.impl.sql.compile.FromBaseTable,
  `estimateCost()`
