# Rule: Correlated ANY to Semi-Join

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/correlated-any-to-semi-join.rra`

## Metadata

- **ID:** `correlated-any-to-semi-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle
- **Tags:** subquery, unnesting, any, semi-join
- **Authors:** "RA Contributors"


# Correlated ANY to Semi-Join

## Description

Converts correlated ANY/SOME subqueries to semi-joins for efficient execution.

**When to apply**: WHERE col op ANY (subquery) with correlation.

**Why it works**: Semi-join can use hash tables or indexes; avoids repeated subquery evaluation.

## Relational Algebra

```algebra
filter[A.x > ANY (select S.y from S where S.key = A.key)](A)
  -> semi_join[A.x > S.y AND S.key = A.key](A, S)
```

## Implementation

```rust
rw!("any-to-semi-join";
    "(filter (any ?op ?col ?subquery) ?input)" =>
    "(semi-join (and (apply ?op ?col ?subquery.col) ?subquery.cond) ?input ?subquery.from)"
),
```

## Cost Model

```rust
fn benefit(outer_size: u64, inner_size: u64) -> f64 {
    let any_subquery = outer_size as f64 * inner_size as f64; // Execute per row
    let semi_join = outer_size as f64 + inner_size as f64; // Hash semi-join
    (any_subquery - semi_join) / any_subquery
}
```

**Typical benefit**: 50-80% for large outer tables

## Test Cases

### Positive: ANY with correlation

```sql
SELECT * FROM employees e
WHERE salary > ANY (
    SELECT salary FROM employees WHERE dept_id = e.dept_id
);

-- Convert to semi-join with inequality
```

### Positive: SOME equivalent

```sql
SELECT * FROM products p
WHERE price >= SOME (
    SELECT price FROM competitor_prices WHERE product_name = p.name
);

-- Convert to semi-join (ANY and SOME are equivalent)
```

### Negative: Uncorrelated ANY

```sql
SELECT * FROM orders
WHERE total > ANY (SELECT 100, 200, 300);

-- Keep as simple comparison: total > 100
```

### Negative: Complex expression in ANY

```sql
SELECT * FROM sales s
WHERE amount > ANY (
    SELECT avg_amount * 1.5 FROM (
        SELECT AVG(amount) as avg_amount FROM sales WHERE region = s.region
        UNION ALL SELECT 0
    )
);

-- Cannot convert: complex nested subquery
```

## References

- PostgreSQL: ANY subquery to semi-join transformation
- Oracle: Subquery unnesting with ANY/SOME
- MySQL: Semi-join optimization for ANY
