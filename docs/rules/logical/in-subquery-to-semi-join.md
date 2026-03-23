# Rule: IN Subquery to Semi-Join

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/in-subquery-to-semi-join.rra`

## Metadata

- **ID:** `in-subquery-to-semi-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, oracle
- **Tags:** subquery, unnesting, semi-join, in-predicate
- **Authors:** "RA Contributors"


# IN Subquery to Semi-Join

## Description

Transforms IN subqueries into semi-joins, eliminating nested loop execution
and enabling hash-based or merge-based semi-join algorithms. This is one of
the most impactful subquery unnesting transformations.

**When to apply**: Any IN subquery that is not correlated or can be decorrelated.

**Why it works**: Semi-joins can use hash tables or sorted inputs for efficient
lookups, whereas naive subquery execution repeats the subquery for each outer row.

## Relational Algebra

```algebra
filter[col IN (subquery)](R)
  -> semi_join[R.col = S.col](R, subquery as S)

Example:
filter[R.id IN (select S.id from S where S.active)](R)
  -> semi_join[R.id = S.id](R, filter[S.active](S))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("in-subquery-to-semi-join";
    "(filter (in ?col (subquery ?inner)) ?outer)" =>
    "(semi-join (= ?col ?inner_col) ?outer (project [?inner_col] ?inner))"
    if is_uncorrelated("?inner", "?outer")
),

// Semi-join returns left rows that have matching right rows
// Key difference from inner join: no duplication of left rows
```

**Restrictions:**
- Subquery must return single column
- For correlated subqueries, requires decorrelation first
- NULL handling: IN with NULL requires special semantics

## Cost Model

```rust
fn estimated_benefit(
    outer_rows: u64,
    inner_rows: u64,
    inner_distinct: u64,
) -> f64 {
    // Nested loop execution: O(outer $\times$ inner)
    let nested_cost = outer_rows * inner_rows;

    // Hash semi-join: Build hash table on inner, probe with outer
    // Cost: O(inner + outer)
    let semi_join_cost = inner_rows + outer_rows;

    (nested_cost as f64 - semi_join_cost as f64) / nested_cost as f64
}
```

**Assumptions:**
- Inner query has high selectivity or small result
- Hash table fits in memory
- Typical speedup: 10x-1000x for large outer tables

**Typical benefit**: 60-95% cost reduction

## Test Cases

### Positive: Simple IN subquery

```sql
SELECT *
FROM customers
WHERE country_id IN (SELECT id FROM countries WHERE region = 'EU');

-- Before: For each customer, scan countries table
-- After: Hash semi-join (build hash on countries, probe with customers)
-- Benefit: O(N$\times$M) -> O(N+M)
```

### Positive: IN with filtered subquery

```sql
SELECT *
FROM orders
WHERE product_id IN (
  SELECT product_id
  FROM products
  WHERE category = 'electronics' AND price > 100
);

-- Unnest to semi-join with filtered inner
-- Enables index usage on products.category
```

### Positive: Multiple IN predicates

```sql
SELECT *
FROM employees e
WHERE e.dept_id IN (SELECT id FROM departments WHERE budget > 1000000)
  AND e.manager_id IN (SELECT id FROM managers WHERE level >= 5);

-- Each IN becomes a semi-join
-- Can execute semi-joins in optimal order
```

### Negative: Correlated IN subquery (needs decorrelation first)

```sql
SELECT *
FROM customers c
WHERE c.id IN (
  SELECT customer_id
  FROM orders o
  WHERE o.date > c.registration_date  -- Correlated!
);

-- Requires decorrelation transformation first
-- This rule does not apply directly to correlated queries
```

## References

**Academic papers:**
- Kim, "On Optimizing an SQL-like Nested Query", ACM TODS 1982
- Pirahesh et al., "Extensible/Rule Based Query Rewrite Optimization in Starburst", SIGMOD 1992
- Galindo-Legaria & Joshi, "Orthogonal Optimization of Subqueries and Aggregation", SIGMOD 2001

**Implementation:**
- PostgreSQL: pull_up_subqueries() in optimizer
- MySQL: IN-to-EXISTS to semi-join transformation
- Oracle: Complex View Merging with semi-join conversion
- mssql: Subquery unnesting with Apply operator elimination
