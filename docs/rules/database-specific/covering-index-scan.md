# Rule: SQLite Covering Index Scan

**Category:** database-specific/sqlite
**File:** `rules/database-specific/sqlite/covering-index-scan.rra`

## Metadata

- **ID:** `sqlite-covering-index-scan`
- **Version:** "1.0.0"
- **Databases:** sqlite
- **Tags:** database-specific, sqlite, covering-index, scan
- **Authors:** "RA Contributors"


# SQLite Covering Index Scan

## Description

When all columns referenced by a query are present in an index,
SQLite reads only the index B-tree and never accesses the main
table B-tree.  Since SQLite stores each table as a separate B-tree
keyed by rowid, a secondary index lookup normally requires two
B-tree traversals: one for the index and one for the table.  A
covering index eliminates the second traversal entirely.

**When to apply**: Every column in the SELECT list, WHERE clause,
ORDER BY, and GROUP BY is present in a single index.

**Why it works**: The index B-tree is typically much smaller than
the table B-tree (fewer columns per entry), so scanning it is
faster. Eliminating the table lookup halves the I/O per row.

**Database version**: SQLite 3.0+

## Relational Algebra

```algebra
-- Before: index scan + table lookup
pi[a, b](table_lookup(index_scan[idx_abc, a > 10](T)))

-- After: index-only scan
pi[a, b](index_only_scan[idx_abc, a > 10](T))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("sqlite-covering-index-scan";
    "(project ?cols
        (table-lookup
            (index-scan ?table ?index ?pred)))" =>
    "(project ?cols
        (index-only-scan ?table ?index ?pred))"
    if is_database("sqlite")
    if index_covers_columns("?index", "?cols", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    index: &Index,
    referenced_columns: &[Column],
) -> bool {
    referenced_columns.iter().all(|c| {
        index.columns().contains(c)
    })
}
```

**Restrictions:**
- Only applies when all referenced columns are in the index
- Does not apply if the query references rowid explicitly
  and rowid is not the index's implicit trailing column

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    table_page_cost: f64,
) -> f64 {
    // Save one table B-tree lookup per row
    rows * table_page_cost
}
```

**Typical benefit**: 40-60% reduction in I/O for read-heavy
queries on well-indexed tables.

## Test Cases

```sql
-- Positive: all columns in index
CREATE INDEX idx_emp ON employees(department, salary);
SELECT department, salary FROM employees
WHERE department = 'Engineering';
-- Index-only scan, no table access
```

```sql
-- Negative: query references column not in index
SELECT department, salary, name FROM employees
WHERE department = 'Engineering';
-- Must access table for 'name' column
```

## References

SQLite: "Query Planning" documentation (sqlite.org)
SQLite: EXPLAIN output shows "COVERING INDEX" annotation
Source: src/where.c, `whereLoopAddBtree()`
