# Rule: Unique Key Join Elimination

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/unique-key-join-elimination.rra`

## Metadata

- **ID:** `unique-key-join-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** join, elimination, unique-key
- **Authors:** "RA Contributors"


# Unique Key Join Elimination

## Description

Eliminates joins when joining on unique key and only using unique-side columns.

**When to apply**: JOIN on unique/primary key where only unique-side columns are referenced.

**Why it works**: Unique key ensures 1:1 or N:1 relationship; if selecting from many-side only, join is redundant.

## Relational Algebra

```algebra
project[A.*](join[A.key = B.unique_key](A, B))
  -> project[A.*](A)
  where unique(B.unique_key) && !references_columns(B)
```

## Implementation

```rust
rw!("eliminate-unique-key-join";
    "(project ?cols (join (= ?key ?unique_key) ?left ?right))" =>
    "(project ?cols ?left)"
    if is_unique("?unique_key") && !uses_right_columns("?cols", "?right")
),
```

## Cost Model

```rust
fn benefit(left_size: u64, right_size: u64) -> f64 {
    let with_join = left_size as f64 + right_size as f64; // Hash join cost
    let without = left_size as f64;
    (with_join - without) / with_join
}
```

**Typical benefit**: 30-50% by eliminating join lookup

## Test Cases

### Positive: Join on primary key, only left columns

```sql
SELECT order_items.* FROM order_items
JOIN orders ON order_items.order_id = orders.id;

-- Eliminate: orders.id is unique, not selecting orders columns
```

### Positive: Unique constraint verification

```sql
SELECT products.* FROM products
JOIN product_metadata ON products.id = product_metadata.product_id;

-- Eliminate if product_metadata.product_id is unique
```

### Negative: Selecting unique-side columns

```sql
SELECT order_items.*, orders.created_at FROM order_items
JOIN orders ON order_items.order_id = orders.id;

-- Cannot eliminate: need orders.created_at
```

### Negative: Non-unique join key

```sql
SELECT order_items.* FROM order_items
JOIN orders ON order_items.customer_id = orders.customer_id;

-- Cannot eliminate: customer_id not unique in orders
```

## References

- PostgreSQL: Unique join elimination in pg_plan
- Oracle: Join cardinality-based elimination
- MySQL: Unique index join optimization
