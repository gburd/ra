# Rule: Subquery to Window Function

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/subquery-to-window.rra`

## Metadata

- **ID:** `subquery-to-window`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, mssql, oracle
- **Tags:** subquery, window, transformation
- **Authors:** "RA Contributors"


# Subquery to Window Function

## Description

Converts correlated scalar subqueries that compute aggregates over a group into equivalent window functions. This avoids the nested-loop evaluation pattern of correlated subqueries.

**When to apply**: Correlated scalar subquery computes an aggregate (COUNT, SUM, etc.) with a correlation predicate that matches a GROUP BY pattern.

## Relational Algebra

```algebra
Project[cols, (SELECT agg(x) FROM t2 WHERE t2.fk = t1.pk)](Scan[t1])
  -> Project[cols, agg_result](Window[agg(x) PARTITION BY fk](Join[t1.pk = t2.fk](t1, t2)))
```

## Implementation

```rust
rw!("subquery-to-window";
    "(project ?cols (scan ?t1) (subquery-agg ?agg ?t2 (= ?fk ?pk)))" =>
    "(project ?cols (window (window-agg ?agg partition-by ?fk) (join inner (= ?pk ?fk) (scan ?t1) (scan ?t2))))"
    if is_correlated_aggregate("?agg", "?t2", "?fk", "?pk")
),
```

## Test Cases

### Positive: Running count as subquery

```sql
SELECT e.name,
    (SELECT COUNT(*) FROM orders o WHERE o.emp_id = e.id) as order_count
FROM employees e;

-- Convert to: SELECT e.name, COUNT(*) OVER (PARTITION BY o.emp_id)
```

## References

- Subquery decorrelation (Seshadri et al., 1996)
- Window function equivalences in modern optimizers
