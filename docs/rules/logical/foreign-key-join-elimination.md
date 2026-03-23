# Rule: Foreign Key Join Elimination

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/foreign-key-join-elimination.rra`

## Metadata

- **ID:** `foreign-key-join-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** join, elimination, foreign-key
- **Authors:** "RA Contributors"


# Foreign Key Join Elimination

## Description

Eliminates joins to tables when foreign key guarantees existence and no columns are needed.

**When to apply**: JOIN on foreign key where joined table columns are not referenced.

**Why it works**: Foreign key constraint ensures referenced rows exist; if not selecting them, join is unnecessary.

## Relational Algebra

```algebra
project[A.*](join[A.fk = B.pk](A, B))
  -> project[A.*](A)
  where foreign_key(A.fk -> B.pk) && !references_columns(B)
```

## Implementation

```rust
rw!("eliminate-fk-join";
    "(project ?cols (join (= ?fk ?pk) ?left ?right))" =>
    "(project ?cols ?left)"
    if is_foreign_key("?fk", "?pk") && !uses_right_columns("?cols", "?right")
),
```

## Cost Model

```rust
fn benefit(left_size: u64, right_size: u64, selectivity: f64) -> f64 {
    let with_join = left_size as f64 * right_size as f64 * selectivity;
    let without = left_size as f64;
    (with_join - without) / with_join
}
```

**Typical benefit**: 40-70% by eliminating join operation

## Test Cases

### Positive: Join not selecting foreign table

```sql
SELECT orders.* FROM orders
JOIN customers ON orders.customer_id = customers.id;

-- Eliminate: FK ensures customer exists, not selecting customer columns
```

### Positive: Aggregation without foreign columns

```sql
SELECT COUNT(*) FROM order_items oi
JOIN orders o ON oi.order_id = o.id;

-- Eliminate: only counting order_items
```

### Negative: Selecting foreign table columns

```sql
SELECT orders.*, customers.name FROM orders
JOIN customers ON orders.customer_id = customers.id;

-- Cannot eliminate: need customer.name
```

### Negative: Foreign key with WHERE on foreign table

```sql
SELECT orders.* FROM orders
JOIN customers ON orders.customer_id = customers.id
WHERE customers.country = 'US';

-- Cannot eliminate: filtering by customer column
```

## References

- PostgreSQL: Join removal optimization in pg_rewrite
- MySQL: Table elimination for foreign keys
- Oracle: Join elimination transformation
