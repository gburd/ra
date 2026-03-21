# Rule: NOT EXISTS to Anti-Join

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/not-exists-to-anti-join.rra`

## Metadata

- **ID:** `not-exists-to-anti-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, oracle
- **Tags:** subquery, unnesting, anti-join, not-exists
- **Authors:** "RA Contributors"


# NOT EXISTS to Anti-Join

## Description

Transforms NOT EXISTS subqueries into anti-joins, returning rows from the outer
relation that have no matches in the inner relation.

**When to apply**: NOT EXISTS subqueries with correlation on join keys.

**Why it works**: Anti-joins use hash tables or bitmaps for efficient "no match"
detection, avoiding repeated subquery execution.

## Relational Algebra

```algebra
filter[NOT EXISTS(subquery)](R)
  -> anti_join[join_condition](R, subquery)

NOT EXISTS(SELECT * FROM S WHERE S.id = R.id)
  -> anti_join[R.id = S.id](R, S)
```

## Implementation

```rust
rw!("not-exists-to-anti-join";
    "(filter (not (exists ?subquery)) ?outer)" =>
    "(anti-join ?join_cond ?outer ?subquery)"
),
```

## Cost Model

```rust
// Build hash table on inner, probe with outer marking misses
fn benefit(outer: u64, inner: u64) -> f64 {
    let nested = outer * inner; // Repeated execution
    let anti_join = outer + inner; // Build + probe
    (nested - anti_join) as f64 / nested as f64
}
```

**Typical benefit**: 60-90% speedup

## Test Cases

### Positive: Find customers without orders

```sql
SELECT * FROM customers c
WHERE NOT EXISTS (
  SELECT 1 FROM orders o WHERE o.customer_id = c.id
);
```

### Positive: Exclude deleted records

```sql
SELECT * FROM products p
WHERE NOT EXISTS (
  SELECT 1 FROM deleted_products d WHERE d.id = p.id
);
```

### Negative: Complex NOT EXISTS with OR

```sql
SELECT * FROM emp e
WHERE NOT EXISTS (
  SELECT 1 FROM dept d
  WHERE d.id = e.dept_id OR d.manager_id = e.id
);

-- Complex predicate complicates anti-join
```

## References

- Galindo-Legaria & Rosenthal, "Outerjoin Simplification and Reordering", ACM TODS 1997
- PostgreSQL: transform_null_equals for anti-join optimization
- DuckDB: Anti-join with mark-based execution
