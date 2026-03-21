# Rule: EXCEPT to Anti-Join

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/except-to-anti-join.rra`

## Metadata

- **ID:** `except-to-anti-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite
- **Tags:** set-operations, except, anti-join
- **Authors:** "RA Contributors"


# EXCEPT to Anti-Join

## Description

Converts EXCEPT (set difference) operations to anti-joins for better optimization opportunities.

**When to apply**: R EXCEPT S where both have compatible schemas.

**Why it works**: Anti-join can use indexes and join algorithms; EXCEPT typically requires materialization.

## Relational Algebra

```algebra
except(R, S)
  -> anti_join[R.* = S.*](R, S)
  where schema_compatible(R, S)
```

## Implementation

```rust
rw!("except-to-anti-join";
    "(except ?left ?right)" =>
    "(anti-join (all-cols-equal ?left ?right) ?left ?right)"
    if same_schema("?left", "?right")
),
```

## Cost Model

```rust
fn benefit(left_size: u64, right_size: u64) -> f64 {
    let except_cost = left_size as f64 + right_size as f64 +
                      (left_size as f64 * (right_size as f64).log2());
    let anti_join = left_size as f64 + right_size as f64; // Hash anti-join
    (except_cost - anti_join) / except_cost
}
```

**Typical benefit**: 30-60% with hash anti-join

## Test Cases

### Positive: Set difference

```sql
SELECT id, name FROM customers
EXCEPT
SELECT id, name FROM deleted_customers;

-- Convert to anti-join on (id, name)
```

### Positive: Filtering by absence

```sql
SELECT product_id FROM inventory
EXCEPT
SELECT product_id FROM discontinued_products;

-- Anti-join can use index on product_id
```

### Negative: EXCEPT ALL (bag semantics)

```sql
SELECT category FROM products
EXCEPT ALL
SELECT category FROM archived_products;

-- Cannot convert: EXCEPT ALL preserves duplicates
```

### Negative: Different schemas

```sql
(SELECT id, name FROM users)
EXCEPT
(SELECT user_id, email FROM deleted_accounts);

-- Cannot convert: incompatible column types
```

## References

- PostgreSQL: EXCEPT implementation via hashing
- DuckDB: EXCEPT to anti-join transformation
- MySQL: Set operation optimization
