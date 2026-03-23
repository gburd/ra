# Rule: EXISTS to Semi-Join

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/exists-to-semi-join.rra`

## Metadata

- **ID:** `exists-to-semi-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, oracle, mssql
- **Tags:** subquery, unnesting, semi-join, exists
- **Authors:** "RA Contributors"


# EXISTS to Semi-Join

## Description

Converts EXISTS subqueries to semi-joins, enabling hash or merge-based execution
instead of nested loops. This is the most common and impactful subquery transformation.

**When to apply**: Any uncorrelated or decorrelated EXISTS subquery.

**Why it works**: Semi-joins avoid repeated subquery execution and enable
efficient join algorithms (hash, merge).

## Relational Algebra

```algebra
filter[EXISTS(subquery)](R)
  -> semi_join[join_condition](R, subquery)

EXISTS(SELECT * FROM S WHERE S.id = R.id AND S.active)
  -> semi_join[R.id = S.id](R, filter[S.active](S))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("exists-to-semi-join";
    "(filter (exists ?subquery) ?outer)" =>
    "(semi-join ?join_cond ?outer ?subquery)"
    if extract_join_condition("?subquery", "?outer")
),
```

## Cost Model

```rust
// Nested: O(N $\times$ M), Semi-join: O(N + M)
fn benefit(outer: u64, inner: u64) -> f64 {
    let nested = outer * inner;
    let semi_join = outer + inner;
    (nested - semi_join) as f64 / nested as f64
}
```

**Typical benefit**: 70-95% for large outer tables

## Test Cases

### Positive: Basic EXISTS

```sql
SELECT * FROM customers c
WHERE EXISTS (SELECT 1 FROM orders o WHERE o.customer_id = c.id);

-- Unnest to hash semi-join
```

### Positive: EXISTS with filter

```sql
SELECT * FROM users u
WHERE EXISTS (
  SELECT 1 FROM purchases p
  WHERE p.user_id = u.id AND p.amount > 1000
);
```

### Negative: Correlated with aggregation

```sql
SELECT * FROM dept d
WHERE EXISTS (
  SELECT 1 FROM emp e
  WHERE e.dept_id = d.id
  HAVING COUNT(*) > (SELECT AVG(count) FROM dept_stats)
);

-- Complex correlation requires additional transformation
```

## References

- Pirahesh et al., "Extensible/Rule Based Query Rewrite Optimization in Starburst", SIGMOD 1992
- PostgreSQL: pull_up_subqueries in optimizer/util/clauses.c
- Oracle: Complex View Merging documentation
