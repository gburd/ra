# Rule: mssql Subquery to Apply (Lateral Join)

**Category:** database-specific/mssql
**File:** `rules/database-specific/mssql/subquery-to-apply.rra`

## Metadata

- **ID:** `mssql-subquery-to-apply`
- **Version:** "1.0.0"
- **Databases:** mssql
- **Tags:** database-specific, mssql, subquery, apply, lateral, cross-apply
- **Authors:** "RA Contributors"


# mssql Subquery to Apply (Lateral Join)

## Description

Converts correlated subqueries into Apply operators (CROSS APPLY /
OUTER APPLY), mssql's implementation of lateral joins.  The Apply
operator evaluates the inner subquery once per outer row, using index
seeks on the inner table when available.

**When to apply**: A correlated subquery can be converted to a lateral
join, especially when the inner side can use index seeks on the
correlation predicate.

**Why it works**: The Apply operator is mssql's native mechanism
for row-by-row evaluation of parameterized subqueries.  When the inner
side has an index on the correlation column, each evaluation is an
efficient index seek rather than a full scan.

**Database version**: mssql 2005+

## Relational Algebra

```algebra
-- Before: correlated scalar subquery
pi[R.*, (SELECT TOP 1 S.val FROM S WHERE S.k = R.k ORDER BY S.ts DESC)](R)

-- After: OUTER APPLY with index seek
R outer-apply (
    TOP 1 (index-seek(S, ix_k_ts, S.k = R.k) ORDER BY S.ts DESC))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mssql-scalar-subquery-to-outer-apply";
    "(project (list ?cols (scalar-subquery
        (top 1 (sort ?order (filter (eq ?sk ?rk) ?inner)))))
     ?outer)" =>
    "(outer-apply ?outer
        (top 1 (sort ?order (filter (eq ?sk ?rk) ?inner))))"
    if is_database("mssql")
    if is_correlated("?rk", "?outer")
),

rw!("mssql-exists-to-cross-apply";
    "(filter (exists (filter (eq ?sk ?rk) ?inner)) ?outer)" =>
    "(cross-apply ?outer
        (top 1 (filter (eq ?sk ?rk) ?inner)))"
    if is_database("mssql")
    if is_correlated("?rk", "?outer")
    if inner_has_index("?inner", "?sk")
),
```

## Preconditions

```rust
fn applicable(
    subquery: &Expr,
    inner_table: &Table,
    correlation_col: &Column,
) -> bool {
    subquery.is_correlated()
    && inner_table.has_index(correlation_col)
}
```

**Restrictions:**
- Apply with full scan inner side is worse than hash semi-join
- Optimizer may choose hash join over apply if outer side is large
- OUTER APPLY preserves NULL-extended rows (like LEFT JOIN)
- Performance depends on outer side cardinality * inner seek cost

## Cost Model

```rust
fn apply_cost(
    outer_rows: f64,
    inner_seek_cost: f64,
) -> f64 {
    outer_rows * inner_seek_cost
}

fn hash_semijoin_cost(
    outer_rows: f64,
    inner_rows: f64,
) -> f64 {
    outer_rows + inner_rows
}
```

**Typical benefit**: For 100 outer rows with indexed inner lookups,
Apply (100 seeks) beats hash semi-join (full inner scan of 1M rows).

## Test Cases

```sql
-- Positive: correlated subquery benefits from Apply
SELECT e.name,
    (SELECT TOP 1 s.amount FROM sales s
     WHERE s.emp_id = e.id ORDER BY s.sale_date DESC)
FROM employees e WHERE e.active = 1;
-- OUTER APPLY with index seek on sales(emp_id, sale_date)
```

```sql
-- Negative: large outer side makes hash join better
SELECT * FROM million_row_table t
WHERE EXISTS (SELECT 1 FROM small_table s WHERE s.id = t.ref_id);
-- Hash semi-join better than 1M Apply iterations
```

## References

mssql: CROSS APPLY and OUTER APPLY
mssql: Nested Loops Join (Apply variant in Showplan)
