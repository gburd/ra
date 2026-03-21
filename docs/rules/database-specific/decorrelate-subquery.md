# Rule: DataFusion Decorrelate Predicate Subquery

**Category:** database-specific/datafusion
**File:** `rules/database-specific/datafusion/decorrelate-subquery.rra`

## Metadata

- **ID:** `datafusion-decorrelate-subquery`
- **Version:** "1.0.0"
- **Databases:** datafusion
- **Tags:** database-specific, datafusion, subquery, decorrelation, exists, in
- **Authors:** "RA Contributors"


# DataFusion Decorrelate Predicate Subquery

## Description

Converts correlated EXISTS and IN subqueries into semi-joins or
anti-joins.  DataFusion's decorrelation pass rewrites the subquery
as a join so it can be executed once using a hash-based strategy
instead of re-evaluated per outer row.

**When to apply**: A WHERE clause contains EXISTS or IN with a
correlated subquery that references the outer table.

**Why it works**: Correlated subqueries execute once per outer row
(nested-loop semantics).  Converting to a semi-join allows DataFusion
to build a hash table once and probe for each outer row, changing
O(N * M) to O(N + M) complexity.

**Database version**: DataFusion 20.0+

## Relational Algebra

```algebra
-- EXISTS to semi-join
sigma[EXISTS(sigma[S.k = R.k](S))](R)
  -> R semi-join[R.k = S.k] S

-- NOT EXISTS to anti-join
sigma[NOT EXISTS(sigma[S.k = R.k](S))](R)
  -> R anti-join[R.k = S.k] S

-- IN to semi-join
sigma[R.a IN (pi[S.a](sigma[S.k = R.k](S)))](R)
  -> R semi-join[R.k = S.k AND R.a = S.a] S
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("datafusion-exists-to-semijoin";
    "(filter (exists (filter (eq ?sk ?rk) ?inner)) ?outer)" =>
    "(semi-join (eq ?rk ?sk) ?outer ?inner)"
    if is_database("datafusion")
    if is_correlated("?rk", "?outer")
),

rw!("datafusion-not-exists-to-antijoin";
    "(filter (not-exists (filter (eq ?sk ?rk) ?inner)) ?outer)" =>
    "(anti-join (eq ?rk ?sk) ?outer ?inner)"
    if is_database("datafusion")
    if is_correlated("?rk", "?outer")
),

rw!("datafusion-in-subquery-to-semijoin";
    "(filter (in-subquery ?col (project ?pcol
                (filter (eq ?sk ?rk) ?inner))) ?outer)" =>
    "(semi-join (and (eq ?rk ?sk) (eq ?col ?pcol)) ?outer ?inner)"
    if is_database("datafusion")
    if is_correlated("?rk", "?outer")
),
```

## Preconditions

```rust
fn applicable(subquery: &Expr, outer_schema: &Schema) -> bool {
    (subquery.is_exists() || subquery.is_in_subquery())
    && has_correlation(subquery, outer_schema)
    && correlation_is_equi_predicate(subquery)
}
```

**Restrictions:**
- Correlation must be on equality predicates (for hash semi-join)
- Non-equality correlations remain as correlated subqueries
- Disjunctive correlations (OR) are not decorrelated
- NULL handling differs between IN and semi-join for three-valued logic

## Cost Model

```rust
fn estimated_benefit(
    outer_rows: f64,
    inner_rows: f64,
) -> f64 {
    // Correlated: O(outer * inner)
    let correlated = outer_rows * inner_rows;
    // Semi-join: O(outer + inner)
    let semijoin = outer_rows + inner_rows;
    correlated - semijoin
}
```

**Typical benefit**: For 100K outer rows and 1M inner rows, converts
O(100B) nested evaluations to O(1.1M) hash semi-join operations.

## Test Cases

```sql
-- Positive: EXISTS to semi-join
SELECT * FROM orders o
WHERE EXISTS (
    SELECT 1 FROM returns r WHERE r.order_id = o.id
);
-- Converted to: orders SEMI JOIN returns ON orders.id = returns.order_id
```

```sql
-- Positive: NOT EXISTS to anti-join
SELECT * FROM customers c
WHERE NOT EXISTS (
    SELECT 1 FROM orders o WHERE o.customer_id = c.id
);
-- Converted to: customers ANTI JOIN orders
```

```sql
-- Negative: uncorrelated IN (already a semi-join candidate)
SELECT * FROM orders WHERE status IN ('shipped', 'delivered');
-- Simple IN-list, no subquery decorrelation needed
```

## References

DataFusion: datafusion/optimizer/src/decorrelate_predicate_subquery.rs
DataFusion: datafusion/optimizer/src/decorrelate.rs
