# Rule: MySQL Index Condition Pushdown (ICP)

**Category:** database-specific/mysql
**File:** `rules/database-specific/mysql/index-condition-pushdown.rra`

## Metadata

- **ID:** `mysql-index-condition-pushdown`
- **Version:** "1.0.0"
- **Databases:** mysql
- **Tags:** database-specific, mysql, index, icp, pushdown
- **Authors:** "RA Contributors"


# MySQL Index Condition Pushdown (ICP)

## Description

Pushes WHERE conditions that reference indexed columns down to the
storage engine layer so they are evaluated during the index scan
rather than after rows are fetched from the base table.  Without
ICP, the server reads full rows matching the index prefix, then
applies remaining WHERE clauses.  With ICP, the storage engine
evaluates conditions on index columns before reading the row data.

**When to apply**: A composite index covers some but not all WHERE
conditions, and the uncovered conditions reference columns in the
index (beyond the usable prefix).

**Why it works**: Avoids reading base table rows for index entries
that will be filtered out by conditions on trailing index columns.
Reduces I/O by skipping row reads for non-matching index entries.

**Database version**: MySQL 5.6+

## Relational Algebra

```algebra
-- Before: filter after full row fetch
sigma[a = 1 AND b LIKE '%foo%'](
    index_scan[idx_a_b, prefix=a](T))

-- After: filter pushed into index scan
index_scan[idx_a_b, prefix=a, icp=(b LIKE '%foo%')](T)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mysql-index-condition-pushdown";
    "(filter ?remaining_pred
        (index-scan ?table ?index ?prefix_pred))" =>
    "(index-scan ?table ?index ?prefix_pred ?remaining_pred)"
    if is_database("mysql")
    if pred_references_index_columns("?remaining_pred", "?index")
),
```

## Preconditions

```rust
fn applicable(
    index: &Index,
    remaining_pred: &Predicate,
) -> bool {
    // ICP applies when remaining predicates reference
    // columns in the index, even if they cannot use the
    // B-tree prefix (e.g., LIKE '%foo%' on second column).
    remaining_pred.columns().iter().all(|c| {
        index.columns().contains(c)
    })
}
```

**Restrictions:**
- Only applies with InnoDB or MyISAM storage engines
- Does not work with subqueries in WHERE clause
- Requires range, ref, eq_ref, or ref_or_null access methods
- Not applicable to virtual generated columns

## Cost Model

```rust
fn estimated_benefit(
    rows_scanned: f64,
    selectivity_of_pushed_pred: f64,
) -> f64 {
    // Rows avoided = those filtered by ICP before row fetch
    let rows_avoided = rows_scanned * (1.0 - selectivity_of_pushed_pred);
    // Each avoided row saves one base table read
    rows_avoided * 0.01 // cost per row read
}
```

**Typical benefit**: 30-70% reduction in row reads for queries with
multi-column indexes and selective conditions on trailing columns.

## Test Cases

```sql
-- Positive: composite index, trailing column filter
CREATE INDEX idx_name ON people(last_name, first_name);
SELECT * FROM people
WHERE last_name = 'Smith' AND first_name LIKE '%John%';
-- ICP evaluates first_name LIKE '%John%' during index scan
```

```sql
-- Negative: all conditions use index prefix
SELECT * FROM people WHERE last_name = 'Smith';
-- No ICP needed, prefix scan is sufficient
```

## References

MySQL: MySQL Reference Manual, "Index Condition Pushdown Optimization"
MySQL: `optimizer_switch` flag `index_condition_pushdown=on`
Source: sql/sql_optimizer.cc, `push_index_cond()`
