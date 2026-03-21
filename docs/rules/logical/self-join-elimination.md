# Rule: Self-Join Elimination

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/self-join-elimination.rra`

## Metadata

- **ID:** `self-join-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** join, elimination, self-join
- **Authors:** "RA Contributors"


# Self-Join Elimination

## Description

Eliminates self-joins when table is joined to itself with equivalent conditions.

**When to apply**: SELECT ... FROM T t1 JOIN T t2 ON t1.key = t2.key

**Why it works**: Self-join is redundant when both sides reference same table with same key.

## Relational Algebra

```algebra
join[t1.key = t2.key](scan[T as t1], scan[T as t2])
  -> scan[T]
  where equivalent_columns(t1.key, t2.key)
```

## Implementation

```rust
rw!("eliminate-self-join";
    "(join (= ?key1 ?key2) (scan ?table ?alias1) (scan ?table ?alias2))" =>
    "(scan ?table ?alias1)"
    if same_table("?table") && equivalent_keys("?key1", "?key2")
),
```

## Cost Model

```rust
fn benefit(table_size: u64, join_cost: f64) -> f64 {
    let with_join = table_size as f64 * table_size as f64 * join_cost;
    let without = table_size as f64;
    (with_join - without) / with_join
}
```

**Typical benefit**: 30-60% by avoiding cartesian product

## Test Cases

### Positive: Simple self-join

```sql
SELECT * FROM users u1
JOIN users u2 ON u1.id = u2.id;

-- Eliminate: just scan users once
```

### Positive: Self-join with additional filters

```sql
SELECT * FROM products p1
JOIN products p2 ON p1.sku = p2.sku
WHERE p1.price > 100;

-- Eliminate join, keep filter
```

### Negative: Different join conditions

```sql
SELECT * FROM orders o1
JOIN orders o2 ON o1.customer_id = o2.customer_id
WHERE o1.id <> o2.id;

-- Cannot eliminate: comparing different rows
```

## References

- PostgreSQL: Self-join removal in pg_rewrite
- Oracle: Query transformation self-join elimination
