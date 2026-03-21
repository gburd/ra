# Rule: Subquery with Aggregation Unnesting

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/subquery-with-aggregation-unnesting.rra`

## Metadata

- **ID:** `subquery-with-aggregation-unnesting`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, mssql, duckdb
- **Tags:** subquery, unnesting, aggregation, decorrelation, group-by
- **Authors:** "RA Contributors"


# Subquery with Aggregation Unnesting

## Description

Unnests correlated subqueries that contain aggregate functions by
transforming the per-row aggregate computation into a single grouped
aggregate followed by a join. This is one of the most impactful
transformations because correlated aggregate subqueries cause nested-loop
execution with repeated scans.

**When to apply**: Correlated subqueries in WHERE, SELECT, or HAVING that
contain aggregate functions (SUM, COUNT, AVG, MIN, MAX) with correlation
predicates.

**Why it works**: Instead of computing the aggregate once per outer row
(O(N * M)), compute all group-level aggregates in a single pass using
GROUP BY on the correlation columns, then join the result back. This
converts O(N * M) to O(N + M + N).

## Relational Algebra

```algebra
-- Correlated scalar aggregate in WHERE
filter[R.val > (SELECT AGG(S.col) FROM S WHERE S.fk = R.pk)](R)
  -> join[R.pk = T.fk](R, T)
  where T = project[fk, agg_result](
              aggregate[AGG(col), GROUP BY fk](S)
            )
  and filter condition becomes R.val > T.agg_result

-- Correlated COUNT in SELECT
project[R.*, (SELECT COUNT(*) FROM S WHERE S.fk = R.pk) AS cnt](R)
  -> project[R.*, COALESCE(T.cnt, 0)](
       left_join[R.pk = T.fk](R, T)
     )
  where T = aggregate[COUNT(*) AS cnt, GROUP BY fk](S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Correlated aggregate in WHERE -> group-by + join
rw!("correlated-aggregate-to-group-join";
    "(filter (?op ?col (scalar-subquery
        (aggregate ?agg_func (filter (= ?corr_col ?outer_col) ?inner))))
     ?outer)" =>
    "(filter (?op ?col ?agg_alias)
       (join (= ?outer_col ?corr_col)
         ?outer
         (aggregate (?agg_func group-by ?corr_col) ?inner)))"
),

// Correlated COUNT in SELECT -> left join with COALESCE
rw!("correlated-count-in-select";
    "(project ?cols
       (scalar-subquery
         (aggregate (count-star) (filter (= ?corr ?outer_col) ?inner)))
       ?outer)" =>
    "(project ?cols_with_coalesce
       (left-join (= ?outer_col ?corr)
         ?outer
         (aggregate ((count-star as ?cnt) group-by ?corr) ?inner)))"
    // Replace scalar-subquery reference with COALESCE(?cnt, 0)
),

// Correlated AVG -> SUM/COUNT group-by + join
rw!("correlated-avg-unnesting";
    "(scalar-subquery
       (aggregate (avg ?col) (filter (= ?corr ?outer_ref) ?inner)))" =>
    "(/ (sum_grouped ?col ?corr ?inner)
        (count_grouped ?col ?corr ?inner))"
    // Decompose AVG into SUM/COUNT for group-by pushdown
),
```

**Restrictions:**
- COUNT must use LEFT JOIN + COALESCE(0) to handle missing groups
- AVG requires decomposition into SUM/COUNT for correct grouped computation
- HAVING in the subquery adds post-aggregation filter
- Multiple correlation predicates require multi-column GROUP BY
- Subqueries with DISTINCT aggregates need special handling

## Cost Model

```rust
fn estimated_benefit(
    outer_size: u64,
    inner_size: u64,
    num_groups: u64,
) -> f64 {
    // Nested: per outer row, scan inner and aggregate
    let nested_cost = outer_size as f64 * inner_size as f64;

    // Unnested: one full scan + group-by, then join
    let group_by_cost = inner_size as f64 * 1.5; // scan + hash agg
    let join_cost = (outer_size + num_groups) as f64; // hash join
    let unnested_cost = group_by_cost + join_cost;

    (nested_cost - unnested_cost) / nested_cost
}
```

**Typical benefit**: 60-95% for large tables; transforms O(N*M) to O(N+M)

## Test Cases

### Positive: Correlated SUM in WHERE

```sql
-- Find employees earning more than their department average
SELECT e.name, e.salary
FROM employees e
WHERE e.salary > (
  SELECT AVG(e2.salary)
  FROM employees e2
  WHERE e2.dept_id = e.dept_id
);

-- Unnested form:
SELECT e.name, e.salary
FROM employees e
JOIN (
  SELECT dept_id, AVG(salary) AS avg_sal
  FROM employees
  GROUP BY dept_id
) d ON e.dept_id = d.dept_id
WHERE e.salary > d.avg_sal;

-- One aggregate pass instead of per-employee subquery
```

### Positive: Correlated COUNT in SELECT

```sql
-- Count orders per customer
SELECT c.name,
  (SELECT COUNT(*) FROM orders o WHERE o.customer_id = c.id) AS order_count
FROM customers c;

-- Unnested with LEFT JOIN for customers with no orders:
SELECT c.name, COALESCE(oc.cnt, 0) AS order_count
FROM customers c
LEFT JOIN (
  SELECT customer_id, COUNT(*) AS cnt
  FROM orders
  GROUP BY customer_id
) oc ON c.id = oc.customer_id;
```

### Positive: Correlated MAX in HAVING

```sql
SELECT d.name, COUNT(*) AS emp_count
FROM departments d
JOIN employees e ON e.dept_id = d.id
GROUP BY d.id, d.name
HAVING COUNT(*) > (
  SELECT AVG(dept_count) FROM (
    SELECT COUNT(*) AS dept_count
    FROM employees GROUP BY dept_id
  ) t
);

-- Inner subquery is uncorrelated -> compute once
-- Unnest and join with the grouped result
```

### Positive: Multiple correlated aggregates

```sql
SELECT p.name,
  (SELECT MIN(price) FROM offers o WHERE o.product_id = p.id) AS min_price,
  (SELECT MAX(price) FROM offers o WHERE o.product_id = p.id) AS max_price
FROM products p;

-- Merge into single group-by:
SELECT p.name, o_agg.min_price, o_agg.max_price
FROM products p
LEFT JOIN (
  SELECT product_id, MIN(price) AS min_price, MAX(price) AS max_price
  FROM offers
  GROUP BY product_id
) o_agg ON p.id = o_agg.product_id;
```

### Negative: Correlated aggregate with LIMIT

```sql
SELECT c.name,
  (SELECT SUM(o.total)
   FROM orders o
   WHERE o.customer_id = c.id
   ORDER BY o.date DESC
   LIMIT 5) AS recent_total
FROM customers c;

-- LIMIT inside aggregate subquery changes semantics
-- Cannot group-by without preserving LIMIT per group
-- Requires LATERAL join or window function approach
```

### Negative: Correlated aggregate with outer reference in aggregate expression

```sql
SELECT e.name
FROM employees e
WHERE (
  SELECT COUNT(CASE WHEN e2.salary > e.salary THEN 1 END)
  FROM employees e2
  WHERE e2.dept_id = e.dept_id
) > 5;

-- e.salary inside the CASE references the outer query
-- Cannot group by dept_id alone; aggregate depends on outer row
-- Requires apply/lateral approach
```

## References

**Academic papers:**
- Kim, "On Optimizing an SQL-like Nested Query", ACM TODS 1982
- Seshadri et al., "Decorrelating Subqueries with Window Aggregates", SIGMOD 1996
- Neumann & Kemper, "Unnesting Arbitrary Queries", BTW 2015

**Implementation:**
- PostgreSQL: `pull_up_subqueries()` with aggregate detection in `prepjointree.c`
- Oracle: Subquery unnesting with GROUP BY introduction
- mssql: Apply removal for correlated aggregates
- DuckDB: `FlattenCorrelatedAggregate` in the binder
