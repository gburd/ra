# Rule: Oracle Index Fast Full Scan

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/index-fast-full-scan.rra`

## Metadata

- **ID:** `oracle-index-fast-full-scan`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, index, fast-full-scan, covering
- **Authors:** "RA Contributors"


# Oracle Index Fast Full Scan

## Description

Replaces a full table scan with an index fast full scan (IFFS) when
all columns referenced by the query are present in a single index.
Unlike a regular index scan, IFFS reads the index using multi-block
I/O (like a table scan) but scans a smaller structure.

**When to apply**: All columns in the SELECT, WHERE, and ORDER BY
are covered by a single composite index.

**Why it works**: The index is typically much smaller than the table
(no non-indexed columns stored).  IFFS uses multi-block reads (db
file scattered read) for sequential I/O throughput, unlike single-block
index range scans.

**Database version**: Oracle 9i+

## Relational Algebra

```algebra
-- Before: full table scan
pi[a, b](sigma[a > 10](scan(T)))

-- After: index fast full scan (index covers a, b)
pi[a, b](sigma[a > 10](index-fast-full-scan(T, idx_ab)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-index-fast-full-scan";
    "(project ?cols (filter ?pred (scan ?table)))" =>
    "(project ?cols (filter ?pred
        (index-fast-full-scan ?table
            (covering-index ?table ?cols ?pred))))"
    if is_database("oracle")
    if has_covering_index("?table", "?cols", "?pred")
),
```

## Preconditions

```rust
fn applicable(
    query_columns: &HashSet<Column>,
    indexes: &[Index],
) -> bool {
    indexes.iter().any(|idx| {
        query_columns.is_subset(&idx.columns().into_iter().collect())
    })
}
```

**Restrictions:**
- All referenced columns must be in the index (covering index)
- NULL values are not stored in B-tree indexes; queries requiring
  NULLs cannot use IFFS unless index has NOT NULL constraint
- Index must not be marked INVISIBLE

## Cost Model

```rust
fn estimated_benefit(
    table_blocks: f64,
    index_blocks: f64,
) -> f64 {
    (table_blocks - index_blocks) * 8192.0 // bytes per block
}
```

**Typical benefit**: Index is 5-20x smaller than table; IFFS reads
proportionally less data from disk.

## Test Cases

```sql
-- Positive: composite index covers all columns
CREATE INDEX idx_emp_dept_sal ON employees(department_id, salary);
SELECT department_id, salary FROM employees WHERE salary > 100000;
-- IFFS on idx_emp_dept_sal; no table access needed
```

```sql
-- Negative: query needs column not in index
SELECT department_id, salary, name FROM employees WHERE salary > 100000;
-- name not in index; must access table
```

## References

Oracle: Oracle Database Performance Tuning Guide, "Index Fast Full Scans"
Oracle: EXPLAIN PLAN INDEX FAST FULL SCAN operation
