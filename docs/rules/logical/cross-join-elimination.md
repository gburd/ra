# Rule: Cross Join Elimination

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/cross-join-elimination.rra`

## Metadata

- **ID:** `cross-join-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** join, elimination, cross-join, cartesian
- **Authors:** "RA Contributors"


# Cross Join Elimination

## Description

Eliminates cross joins (cartesian products) when one table returns exactly one row.

**When to apply**: CROSS JOIN where one side is guaranteed to return single row.

**Why it works**: Joining with single-row table is equivalent to adding columns without multiplication.

## Relational Algebra

```algebra
cross_join(R, single_row(S))
  -> extend[S.cols](R)
  where cardinality(S) = 1
```

## Implementation

```rust
rw!("eliminate-cross-join-with-single-row";
    "(cross-join ?left ?right)" =>
    "(extend ?left (project-cols ?right))"
    if is_single_row("?right")
),
```

## Cost Model

```rust
fn benefit(left_size: u64, right_size: u64) -> f64 {
    assert_eq!(right_size, 1);
    let with_join = left_size as f64 * right_size as f64;
    let without = left_size as f64;
    (with_join - without) / with_join
}
```

**Typical benefit**: 50-90% when avoiding cartesian product

## Test Cases

### Positive: Cross join with aggregation result

```sql
SELECT * FROM orders
CROSS JOIN (SELECT MAX(price) as max_price FROM products);

-- Eliminate: subquery returns 1 row, just extend orders
```

### Positive: Cross join with scalar subquery

```sql
SELECT *, (SELECT COUNT(*) FROM customers) as total
FROM products
CROSS JOIN (SELECT COUNT(*) FROM customers);

-- Simplify to scalar subquery in SELECT
```

### Negative: Both tables have multiple rows

```sql
SELECT * FROM users CROSS JOIN roles;

-- Cannot eliminate: true cartesian product needed
```

### Negative: Subquery might return 0 or multiple rows

```sql
SELECT * FROM orders
CROSS JOIN (SELECT price FROM products WHERE id = ?);

-- Cannot eliminate: cardinality not guaranteed
```

## References

- PostgreSQL: Cross join optimization with single-row tables
- MySQL: Cartesian product elimination
- mssql: Cross apply simplification
