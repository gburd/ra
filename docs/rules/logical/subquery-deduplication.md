# Rule: Subquery Deduplication

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/subquery-deduplication.rra`

## Metadata

- **ID:** `subquery-deduplication`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** subquery, unnesting, deduplication, cse
- **Authors:** "RA Contributors"


# Subquery Deduplication

## Description

Identifies and merges identical subqueries to avoid redundant execution.

**When to apply**: Multiple identical or equivalent subqueries in same query.

**Why it works**: Execute subquery once, reuse result; applies common subexpression elimination to subqueries.

## Relational Algebra

```algebra
project[A.*, (subquery S1), (subquery S2)](A)
  -> project[A.*, result.val, result.val](join(A, execute_once(S1)))
  where S1 $\equiv$ S2
```

## Implementation

```rust
rw!("deduplicate-identical-subqueries";
    "(project (list ?cols (subquery ?sq1) (subquery ?sq2)) ?input)" =>
    "(project (list ?cols ?result ?result) (with ?result ?sq1 ?input))"
    if equivalent_subqueries("?sq1", "?sq2")
),
```

## Cost Model

```rust
fn benefit(num_duplicates: usize, subquery_cost: f64) -> f64 {
    let with_duplicates = num_duplicates as f64 * subquery_cost;
    let deduplicated = subquery_cost + (num_duplicates as f64 * 0.1); // Small overhead
    (with_duplicates - deduplicated) / with_duplicates
}
```

**Typical benefit**: 50-90% when deduplicating expensive subqueries

## Test Cases

### Positive: Identical scalar subqueries

```sql
SELECT
    product_id,
    (SELECT AVG(price) FROM products) as avg_price,
    price - (SELECT AVG(price) FROM products) as price_diff
FROM products;

-- Execute AVG once, reuse result
```

### Positive: Same EXISTS in multiple places

```sql
SELECT * FROM orders o
WHERE EXISTS (SELECT 1 FROM customers c WHERE c.id = o.customer_id AND c.active = true)
  AND status = 'pending'
  OR EXISTS (SELECT 1 FROM customers c WHERE c.id = o.customer_id AND c.active = true);

-- Deduplicate EXISTS check
```

### Negative: Semantically different subqueries

```sql
SELECT
    (SELECT COUNT(*) FROM orders WHERE customer_id = 1) as count1,
    (SELECT COUNT(*) FROM orders WHERE customer_id = 2) as count2
FROM customers;

-- Cannot deduplicate: different predicates
```

### Negative: Correlated vs uncorrelated

```sql
SELECT
    dept_id,
    (SELECT COUNT(*) FROM employees) as total_emps,
    (SELECT COUNT(*) FROM employees WHERE dept_id = d.id) as dept_emps
FROM departments d;

-- Cannot deduplicate: one is correlated, one is not
```

## References

- PostgreSQL: Common subexpression elimination for subqueries
- Oracle: Subquery result caching
- mssql: Repeated subquery execution plan sharing
