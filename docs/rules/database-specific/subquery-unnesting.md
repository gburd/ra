# Rule: Oracle Subquery Unnesting

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/subquery-unnesting.rra`

## Metadata

- **ID:** `oracle-subquery-unnesting`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, subquery, unnesting, decorrelation, join
- **Authors:** "RA Contributors"


# Oracle Subquery Unnesting

## Description

Converts correlated and uncorrelated subqueries into joins (semi-joins,
anti-joins, or inline views).  Oracle's optimizer aggressively unnests
subqueries to enable join ordering and cost-based access path selection
on the subquery's tables.

**When to apply**: A query contains EXISTS, NOT EXISTS, IN, NOT IN,
scalar subqueries, or ANY/ALL subqueries that can be expressed as joins.

**Why it works**: Subqueries force a fixed evaluation order (outer then
inner).  Unnesting into joins allows Oracle's cost-based optimizer to
consider all possible join orders, access paths, and join methods --
often finding plans orders of magnitude faster.

**Database version**: Oracle 9i+

## Relational Algebra

```algebra
-- EXISTS to semi-join
sigma[EXISTS(sigma[S.k = R.k](S))](R)
  -> R semi-join[R.k = S.k] S

-- NOT IN to anti-join (with null handling)
sigma[R.a NOT IN (SELECT S.a FROM S)](R)
  -> R anti-join[R.a = S.a] S
  -- Special null handling: NOT IN returns NULL if subquery has NULLs

-- Scalar subquery to outer join
pi[R.*, (SELECT MAX(S.v) FROM S WHERE S.k = R.k)](R)
  -> R left-join[R.k = S.k] (gamma[k; max_v=MAX(v)](S))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-exists-unnesting";
    "(filter (exists (filter (eq ?sk ?rk) ?inner)) ?outer)" =>
    "(semi-join (eq ?rk ?sk) ?outer ?inner)"
    if is_database("oracle")
),

rw!("oracle-not-in-unnesting";
    "(filter (not-in ?col (project ?pcol ?inner)) ?outer)" =>
    "(anti-join-null-aware (eq ?col ?pcol) ?outer ?inner)"
    if is_database("oracle")
),

rw!("oracle-scalar-subquery-unnesting";
    "(project (list ?cols (scalar-subquery
        (aggregate ?agg (filter (eq ?sk ?rk) ?inner)))) ?outer)" =>
    "(project (list ?cols ?agg_alias)
        (left-join (eq ?rk ?sk) ?outer
            (aggregate-group ?agg ?sk ?inner)))"
    if is_database("oracle")
),
```

## Preconditions

```rust
fn applicable(subquery: &Expr) -> bool {
    subquery.is_subquery()
    && (subquery.has_equi_correlation()
        || subquery.is_uncorrelated())
}
```

**Restrictions:**
- NOT IN with NULLs requires null-aware anti-join (NAAJ)
- CONNECT BY (hierarchical) subqueries cannot be unnested
- Subqueries with ROWNUM references prevent unnesting
- Oracle hint UNNEST / NO_UNNEST controls this transformation

## Cost Model

```rust
fn estimated_benefit(
    outer_rows: f64,
    inner_rows: f64,
    correlation_selectivity: f64,
) -> f64 {
    let nested_cost = outer_rows * inner_rows * correlation_selectivity;
    let join_cost = outer_rows + inner_rows;
    nested_cost - join_cost
}
```

**Typical benefit**: Converts O(N*M) nested evaluation to O(N+M)
hash join, often 100x-10000x speedup for large tables.

## Test Cases

```sql
-- Positive: EXISTS unnested to semi-join
SELECT * FROM departments d
WHERE EXISTS (SELECT 1 FROM employees e WHERE e.dept_id = d.id);
-- Semi-join allows hash join implementation
```

```sql
-- Positive: NOT IN unnested to null-aware anti-join
SELECT * FROM customers
WHERE id NOT IN (SELECT customer_id FROM orders);
-- Null-aware anti-join handles NULL customer_ids correctly
```

```sql
-- Negative: ROWNUM in subquery prevents unnesting
SELECT * FROM t WHERE x IN (SELECT * FROM s WHERE ROWNUM <= 10);
-- ROWNUM dependency prevents flattening
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Subquery Unnesting"
Oracle: UNNEST / NO_UNNEST optimizer hints
Oracle: Note 62298.1 "Subquery Unnesting"
