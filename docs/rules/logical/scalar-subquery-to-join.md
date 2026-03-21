# Rule: Scalar Subquery to Join

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/scalar-subquery-to-join.rra`

## Metadata

- **ID:** `scalar-subquery-to-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** subquery, unnesting, scalar, join
- **Authors:** "RA Contributors"


# Scalar Subquery to Join

## Description

Converts scalar subqueries in SELECT list to LEFT JOIN for better optimization.

**When to apply**: SELECT with correlated scalar subquery that returns at most one row.

**Why it works**: Join can be optimized with indexes; avoids repeated subquery execution per row.

## Relational Algebra

```algebra
project[A.*, (select S.val from S where S.key = A.key)](A)
  -> project[A.*, S.val](left_join[A.key = S.key](A, S))
  where unique(S.key)
```

## Implementation

```rust
rw!("scalar-subquery-to-left-join";
    "(project (list ?cols (scalar-subquery ?sq)) ?input)" =>
    "(project (list ?cols ?sq.col) (left-join ?sq.cond ?input ?sq.from))"
    if is_unique_result("?sq")
),
```

## Cost Model

```rust
fn benefit(outer_size: u64, inner_size: u64, correlation: bool) -> f64 {
    let subquery = if correlation {
        outer_size as f64 * inner_size as f64 // Re-execute per row
    } else {
        inner_size as f64
    };
    let join = outer_size as f64 + inner_size as f64; // Hash join
    (subquery - join) / subquery
}
```

**Typical benefit**: 40-70% for correlated subqueries

## Test Cases

### Positive: Correlated scalar subquery

```sql
SELECT
    customer_id,
    (SELECT MAX(order_date) FROM orders WHERE customer_id = c.id) as last_order
FROM customers c;

-- Convert to LEFT JOIN with GROUP BY
```

### Positive: Lookup subquery

```sql
SELECT
    product_id,
    (SELECT name FROM categories WHERE id = products.category_id) as category_name
FROM products;

-- Convert to LEFT JOIN on category_id
```

### Negative: Non-unique result

```sql
SELECT
    customer_id,
    (SELECT order_id FROM orders WHERE customer_id = c.id) as order_id
FROM customers c;

-- Cannot convert: subquery might return multiple rows
```

### Negative: Nested aggregation

```sql
SELECT
    dept_id,
    (SELECT AVG(salary) FROM employees e
     WHERE e.dept_id = d.id AND e.salary > (SELECT AVG(salary) FROM employees)) as avg_sal
FROM departments d;

-- Cannot convert: complex nested correlation
```

## References

- PostgreSQL: pull_up_subqueries for scalar subqueries
- Oracle: Subquery unnesting transformation
- MySQL: Scalar subquery optimization
- mssql: Apply operator for correlated subqueries
