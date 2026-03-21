# Correlated Subquery

## Description

A subquery that references columns from the outer query, executing once per outer row. Ra transforms these into efficient joins or semi-joins.

## Use Cases

- Finding rows with related maximum/minimum values
- Existence checks (EXISTS)
- Per-row calculations from other tables
- "For each X, find Y" patterns

## Relational Algebra

Correlated subquery as dependent join:

$$
R \rtimes_{\theta(r, s)} S
$$

Where $\theta$ is a predicate referencing both $r \in R$ and $s \in S$.

General form:

$$
\pi_{R.*, (\text{subquery})}(R)
$$

Where subquery depends on $R$ columns.

## How Ra Optimizes

### 1. Subquery Unnesting

**Rule:** `logical/subquery-unnesting`

Transform correlated subquery to join:

$$
\pi_{R.*}(R \rtimes_{\theta} S) \rightarrow \pi_{R.*}(R \ltimes_{\theta} S)
$$

Where $\ltimes$ is semi-join (returns rows from $R$ with matches in $S$).

**Advantage:** Executes once instead of $|R|$ times.

### 2. Apply Operator Elimination

**Rule:** `logical/apply-to-join`

Convert Apply (correlated execution) to join when possible:

$$
R \text{ APPLY } (\text{subquery}(r)) \rightarrow R \bowtie_{\theta} S
$$

### 3. Decorrelation with Aggregates

For aggregates:

$$
\pi_{R.*, (\text{SELECT MAX}(S.y) \text{ WHERE } S.x = R.x)}(R)
$$

Becomes:

$$
R \bowtie_{R.x = S.x} (\gamma_{S.x; \text{MAX}(S.y)}(S))
$$

Pre-compute aggregates, then join.

## Statistics API

```rust
use ra_optimizer::{Statistics, ColumnStatistics};

// Outer query table
optimizer.add_table_stats("employees", Statistics {
    row_count: 10_000,
});

// Subquery table
optimizer.add_table_stats("salaries", Statistics {
    row_count: 100_000,  // 10 salary records per employee average
});

optimizer.add_column_stats("employees", "id", ColumnStatistics {
    distinct_count: 10_000,
    null_fraction: 0.0,
});

optimizer.add_column_stats("salaries", "employee_id", ColumnStatistics {
    distinct_count: 10_000,  // FK to employees
    null_fraction: 0.0,
});
```

## Examples

### Scalar Correlated Subquery

❌ **Original (Inefficient):**

```sql
SELECT
  e.name,
  e.salary,
  (SELECT AVG(s.salary)
   FROM employees s
   WHERE s.department = e.department) as dept_avg
FROM employees e;
```

**Naive Execution:** Subquery runs 10,000 times.

✅ **Ra Optimized Plan:**

```
Project [name, salary, dept_avg]
  HashJoin [e.department = agg.department]
    SeqScan [employees e]
    HashAggregate [department]
      Aggregates: AVG(salary) AS dept_avg
      SeqScan [employees s]
```

**Relational Algebra:**

$$
\pi_{\text{name}, \text{salary}, \text{dept\_avg}}(\text{employees} \bowtie_{\text{dept}} \gamma_{\text{dept}; \text{AVG}(\text{salary})}(\text{employees}))
$$

**Execution:** Subquery computed once, joined.

### EXISTS Correlated Subquery

❌ **Original:**

```sql
SELECT c.name
FROM customers c
WHERE EXISTS (
  SELECT 1
  FROM orders o
  WHERE o.customer_id = c.id
    AND o.status = 'completed'
);
```

**Naive Cost:** $|C| \times \text{Cost}(\text{subquery})$

✅ **Ra Optimized Plan:**

```
Project [name]
  SemiJoin [c.id = o.customer_id]
    SeqScan [customers c]
    SeqScan [orders o]
      Filter: status = 'completed'
```

**Relational Algebra:**

$$
\pi_{\text{name}}(\text{customers} \ltimes_{\text{id=customer\_id}} \sigma_{\text{status='completed'}}(\text{orders}))
$$

**Cost:** $O(|C| + |O|)$ instead of $O(|C| \times |O|)$.

### NOT EXISTS (Anti-Join)

```sql
SELECT p.product_name
FROM products p
WHERE NOT EXISTS (
  SELECT 1
  FROM order_items oi
  WHERE oi.product_id = p.id
);
```

**Ra Plan:**

```
Project [product_name]
  AntiJoin [p.id = oi.product_id]
    SeqScan [products p]
    SeqScan [order_items oi]
```

**Relational Algebra:**

$$
\pi_{\text{product\_name}}(\text{products} \triangleright_{\text{id=product\_id}} \text{order\_items})
$$

Where $\triangleright$ is anti-join (returns rows from left with no match in right).

### Correlated Subquery with Aggregate

❌ **Original:**

```sql
SELECT e.name, e.salary
FROM employees e
WHERE e.salary > (
  SELECT AVG(s.salary)
  FROM employees s
  WHERE s.department = e.department
);
```

✅ **Ra Plan:**

```
Filter (salary > dept_avg)
  HashJoin [e.department = agg.department]
    SeqScan [employees e]
    HashAggregate [department]
      Aggregates: AVG(salary) AS dept_avg
      SeqScan [employees s]
```

**Transformation:**

$$
\sigma_{\text{salary} > \text{dept\_avg}}(\text{employees} \bowtie_{\text{dept}} \gamma_{\text{dept}; \text{AVG}(\text{salary})}(\text{employees}))
$$

### Multiple Correlated Subqueries

```sql
SELECT
  p.product_name,
  (SELECT COUNT(*) FROM orders o WHERE o.product_id = p.id) as order_count,
  (SELECT SUM(quantity) FROM order_items oi WHERE oi.product_id = p.id) as total_quantity
FROM products p;
```

**Ra Plan:**

```
Project [product_name, order_count, total_quantity]
  HashJoin [p.id = oi_agg.product_id]
    HashJoin [p.id = o_agg.product_id]
      SeqScan [products p]
      HashAggregate [product_id]
        Aggregates: COUNT(*) AS order_count
        SeqScan [orders o]
    HashAggregate [product_id]
      Aggregates: SUM(quantity) AS total_quantity
      SeqScan [order_items oi]
```

**Optimization:** Both aggregates computed once, then joined to products.

### Lateral Subquery (SQL:2003)

```sql
SELECT e.name, recent_orders.order_date
FROM employees e,
LATERAL (
  SELECT order_date
  FROM orders o
  WHERE o.salesperson_id = e.id
  ORDER BY order_date DESC
  LIMIT 3
) recent_orders;
```

**Ra Plan:**

```
NestedLoopJoin [LATERAL]
  SeqScan [employees e]
  TopN (3) [order_date DESC]
    IndexScan [orders.salesperson_idx]
      Filter: salesperson_id = $e.id  -- Correlated parameter
```

**Note:** LATERAL requires correlated execution (cannot decorrelate Top-N with filter).

## Decorrelation Conditions

Ra can decorrelate when:

1. ✅ Subquery is simple aggregation
2. ✅ EXISTS/NOT EXISTS without complex logic
3. ✅ Scalar subquery with aggregates
4. ❌ Subquery has LIMIT/TOP-N
5. ❌ Subquery has complex conditionals

## Performance Impact

| Pattern | Before Unnesting | After Unnesting | Improvement |
|---------|-----------------|----------------|-------------|
| EXISTS with index | $O(n \log m)$ | $O(n + m)$ | 10-100x |
| Scalar aggregate | $O(n \times m)$ | $O(n + m)$ | 100-1000x |
| NOT EXISTS | $O(n \times m)$ | $O(n + m)$ | 100-1000x |

## Anti-Patterns

### 1. Unnecessary Correlation

❌ **Bad:**
```sql
SELECT e.name,
       (SELECT d.department_name FROM departments d WHERE d.id = e.department_id)
FROM employees e;
```

Better written as explicit join.

✅ **Good:**
```sql
SELECT e.name, d.department_name
FROM employees e
JOIN departments d ON d.id = e.department_id;
```

### 2. Multiple Identical Subqueries

❌ **Bad:**
```sql
SELECT
  name,
  CASE WHEN salary > (SELECT AVG(salary) FROM employees) THEN 'above' ELSE 'below' END,
  salary - (SELECT AVG(salary) FROM employees) as diff
FROM employees;
```

Ra will optimize this, but better to compute once:

✅ **Good:**
```sql
WITH avg_sal AS (SELECT AVG(salary) as avg FROM employees)
SELECT
  name,
  CASE WHEN salary > avg_sal.avg THEN 'above' ELSE 'below' END,
  salary - avg_sal.avg as diff
FROM employees, avg_sal;
```

## See Also

- [Scalar Subqueries](scalar-subquery.md) - Single-value subqueries
- [EXISTS Subqueries](exists-subquery.md) - Semi-join patterns
- [IN Subqueries](in-subquery.md) - Set membership
- [Lateral Subqueries](lateral-subquery.md) - LATERAL joins
- [Semi Joins](../joins/semi-join.md) - EXISTS optimization
- [Rule: Subquery Unnesting](../../../rules/logical/subquery-unnesting.md)
- [Example: Subquery Unnesting](../../../examples/subquery-unnesting.md)

## References

- Kim, "On Optimizing an SQL-like Nested Query", *TODS 1982*
- Galindo-Legaria & Joshi, "Orthogonal Optimization of Subqueries and Aggregation", *SIGMOD 2001*
- Bellamkonda et al., "Adaptive and Big Data Scale Parallel Execution in Oracle", *VLDB 2013*
