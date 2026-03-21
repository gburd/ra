# Rule: LEFT JOIN Null Rejection

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/left-join-null-rejection.rra`

## Metadata

- **ID:** `left-join-null-rejection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** join, elimination, left-join, null-rejection, inner-join
- **Authors:** "RA Contributors"


# LEFT JOIN Null Rejection

## Description

Converts a LEFT JOIN to an INNER JOIN when a WHERE clause contains an
IS NOT NULL predicate (or any null-rejecting expression) on a column from
the right (nullable) side. This is a specific, commonly triggered instance
of the general outer-join-to-inner simplification.

**When to apply**: LEFT JOIN where the WHERE clause tests a right-side
column with IS NOT NULL, a comparison, IN list, or any expression that
evaluates to FALSE or UNKNOWN on NULL input.

**Why it works**: LEFT JOIN preserves left rows that have no right match
by padding right columns with NULL. An IS NOT NULL filter on a right column
eliminates exactly those preserved-but-unmatched rows, making the LEFT JOIN
behaviorally identical to an INNER JOIN.

## Relational Algebra

```algebra
filter[S.col IS NOT NULL](left_join[cond](R, S))
  -> join[cond](R, S)

Generalization to any null-rejecting predicate p:
filter[p(S.col)](left_join[cond](R, S))
  -> filter[p(S.col)](join[cond](R, S))
  where p(NULL) = FALSE or p(NULL) = NULL
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Direct IS NOT NULL case
rw!("left-join-null-rejection-is-not-null";
    "(filter (is-not-null ?col) (left-join ?cond ?left ?right))" =>
    "(filter (is-not-null ?col) (join ?cond ?left ?right))"
    if column_from_table("?col", "?right")
),

// General null-rejecting predicate
rw!("left-join-null-rejection-general";
    "(filter ?pred (left-join ?cond ?left ?right))" =>
    "(filter ?pred (join ?cond ?left ?right))"
    if is_null_rejecting_on("?pred", "?right")
),

fn is_null_rejecting_on(pred: &Expr, table: &Table) -> bool {
    // Predicate references table and rejects NULLs
    let cols = pred.referenced_columns();
    let table_cols: Vec<_> = cols.iter()
        .filter(|c| c.table() == table)
        .collect();

    if table_cols.is_empty() {
        return false;
    }

    // Substitute NULLs for all table columns and evaluate
    let null_result = pred.evaluate_with_null_substitution(&table_cols);
    null_result == EvalResult::False || null_result == EvalResult::Null
}
```

**Restrictions:**
- Only the WHERE clause (not ON clause) triggers null rejection
- Predicates inside CASE, COALESCE, or OR may not be null-rejecting
- Disjunctions require all branches to be null-rejecting
- Does not apply if the predicate is IS NULL (anti-join pattern)

## Cost Model

```rust
fn estimated_benefit(
    left_rows: u64,
    match_ratio: f64,
) -> f64 {
    // LEFT JOIN: output = left_rows (all preserved)
    // INNER JOIN: output = left_rows * match_ratio
    let rows_eliminated = left_rows as f64 * (1.0 - match_ratio);
    let direct_benefit = rows_eliminated / left_rows as f64;

    // Indirect: INNER JOIN enables commutativity and reordering
    let reorder_bonus = 0.15;

    (direct_benefit + reorder_bonus).min(1.0)
}
```

**Typical benefit**: 20-60% from reduced output size plus optimization
enablement

## Test Cases

### Positive: Explicit IS NOT NULL in WHERE

```sql
SELECT o.id, c.name
FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
WHERE c.id IS NOT NULL;

-- c.id IS NOT NULL rejects the NULL-padded rows
-- Equivalent to: INNER JOIN
-- Rewrite to:
SELECT o.id, c.name
FROM orders o
JOIN customers c ON o.customer_id = c.id;
```

### Positive: Comparison predicate on right side

```sql
SELECT e.name, d.budget
FROM employees e
LEFT JOIN departments d ON e.dept_id = d.id
WHERE d.budget > 100000;

-- d.budget > 100000 is null-rejecting (NULL > 100000 = UNKNOWN)
-- Rewrite to INNER JOIN:
SELECT e.name, d.budget
FROM employees e
JOIN departments d ON e.dept_id = d.id
WHERE d.budget > 100000;
```

### Positive: IN predicate on right side

```sql
SELECT p.name, c.name AS category
FROM products p
LEFT JOIN categories c ON p.category_id = c.id
WHERE c.name IN ('Electronics', 'Books', 'Clothing');

-- IN with literal values is null-rejecting
-- NULL IN (...) = UNKNOWN, filtered out
```

### Positive: Multiple predicates with AND

```sql
SELECT a.val, b.status, b.priority
FROM table_a a
LEFT JOIN table_b b ON a.id = b.a_id
WHERE b.status = 'active' AND b.priority > 5;

-- Both predicates are null-rejecting on right side
-- AND of null-rejecting predicates is null-rejecting
```

### Negative: IS NULL predicate (anti-join pattern)

```sql
SELECT o.id
FROM orders o
LEFT JOIN shipments s ON o.id = s.order_id
WHERE s.id IS NULL;

-- IS NULL passes NULLs -> not null-rejecting
-- This is a correct anti-join: find orders without shipments
```

### Negative: COALESCE masks null rejection

```sql
SELECT e.name, COALESCE(d.name, 'Unknown') AS dept
FROM employees e
LEFT JOIN departments d ON e.dept_id = d.id
WHERE COALESCE(d.active, true) = true;

-- COALESCE(NULL, true) = true -> not null-rejecting
-- LEFT JOIN semantics are required
```

### Negative: OR disjunction with non-null-rejecting branch

```sql
SELECT a.id, b.val
FROM table_a a
LEFT JOIN table_b b ON a.id = b.a_id
WHERE b.val > 10 OR a.status = 'special';

-- a.status = 'special' does not reference right side
-- OR: if left branch is TRUE, right NULLs are preserved
-- Not null-rejecting overall
```

## References

**Academic papers:**
- Galindo-Legaria & Rosenthal, "Outerjoin Simplification and Reordering for Query Optimization", ACM TODS 1997
- Bhargava et al., "Simplifying Outer Joins", SIGMOD 1995

**Implementation:**
- PostgreSQL: `reduce_outer_joins()` checks `nonnullable_rels` in `analyzejoins.c`
- MySQL: `simplify_joins()` in `sql_optimizer.cc` with `OUTER_JOIN_NULL_REJECT`
- Oracle: Outer join conversion in cost-based optimizer
- mssql: `CNullRejection` analysis in algebrizer
- DuckDB: `FilterPullup` + `JoinOrderOptimizer` null rejection detection
