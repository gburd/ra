# Rule: MySQL Derived Table Merge

**Category:** database-specific/mysql
**File:** `rules/database-specific/mysql/derived-table-merge.rra`

## Metadata

- **ID:** `mysql-derived-table-merge`
- **Version:** "1.0.0"
- **Databases:** mysql
- **Tags:** database-specific, mysql, derived-table, view, merge, inline
- **Authors:** "RA Contributors"


# MySQL Derived Table Merge

## Description

Merges derived tables (subqueries in FROM clause) and views into the
outer query block, eliminating materialization.  The derived table's
SELECT list, WHERE clause, and JOIN conditions are folded into the
outer query, allowing the optimizer to consider all tables together
for join ordering and index selection.

**When to apply**: A derived table or view does not use aggregation,
DISTINCT, LIMIT, UNION, or window functions that would prevent merging.

**Why it works**: Without merging, the derived table is materialized
into a temp table with no indexes, forcing a full scan when joined
with the outer query.  Merging lets the optimizer push predicates and
choose join order across all tables.

**Database version**: MySQL 5.7+ (automatic), 8.0+ (improved)

## Relational Algebra

```algebra
-- Before: derived table materialized
outer_rel join (materialize(
    sigma[cond](pi[cols](inner_rel))))

-- After: merged into outer query
sigma[cond](outer_rel join inner_rel)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mysql-derived-table-merge";
    "(join ?pred
        ?outer_rel
        (materialize (project ?cols
            (filter ?inner_pred ?inner_rel))))" =>
    "(project ?cols
        (filter ?inner_pred
            (join ?pred ?outer_rel ?inner_rel)))"
    if is_database("mysql")
    if is_mergeable_derived("?inner_rel", "?cols",
        "?inner_pred")
),
```

## Preconditions

```rust
fn applicable(derived: &DerivedTable) -> bool {
    !derived.has_aggregation()
    && !derived.has_distinct()
    && !derived.has_limit()
    && !derived.has_union()
    && !derived.has_window_functions()
    && !derived.is_recursive_cte()
}
```

**Restrictions:**
- Cannot merge if derived table uses GROUP BY, HAVING, or aggregates
- Cannot merge UNION or INTERSECT derived tables
- Cannot merge if LIMIT is present (would change semantics)
- Controlled by `optimizer_switch='derived_merge=on'`

## Cost Model

```rust
fn estimated_benefit(
    derived_rows: f64,
    outer_rows: f64,
) -> f64 {
    // Materialization cost saved
    let mat_cost = derived_rows * 0.01;
    // Temp table scan cost saved
    let scan_cost = derived_rows * 0.005;
    mat_cost + scan_cost
}
```

**Typical benefit**: Eliminates temp table creation; allows
predicate pushdown into merged tables.

## Test Cases

```sql
-- Positive: simple derived table
SELECT * FROM orders o
JOIN (SELECT id, name FROM customers WHERE active = 1) c
    ON o.cust_id = c.id;
-- Merged: customers joined directly with predicate active=1
```

```sql
-- Negative: derived table with GROUP BY
SELECT * FROM orders o
JOIN (SELECT cust_id, COUNT(*) cnt FROM returns
      GROUP BY cust_id) r ON o.cust_id = r.cust_id;
-- Cannot merge: GROUP BY prevents merging
```

## References

MySQL: "Optimizing Derived Tables, View References, and CTEs"
MySQL: `optimizer_switch` flag `derived_merge=on`
Source: sql/sql_derived.cc, `mysql_derived_merge()`
