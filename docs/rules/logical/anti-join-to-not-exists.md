# Rule: Anti-Join to NOT EXISTS

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/anti-join-to-not-exists.rra`

## Metadata

- **ID:** `anti-join-to-not-exists`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** join, anti-join, not-exists
- **Authors:** "RA Contributors"


# Anti-Join to NOT EXISTS

## Description

Converts anti-join to NOT EXISTS check for efficient filtering.

**When to apply**: Anti-join filtering left table by absence of matching right rows.

**Why it works**: NOT EXISTS can short-circuit on first match; avoids building full anti-join result.

## Relational Algebra

```algebra
anti_join[cond](L, R)
  -> filter[!exists(R where cond)](L)
  where simple_condition(cond)
```

## Implementation

```rust
rw!("anti-join-to-not-exists";
    "(anti-join ?cond ?left ?right)" =>
    "(filter (not (exists (select ?right (where ?cond)))) ?left)"
    if can_short_circuit("?right", "?cond")
),
```

## Cost Model

```rust
fn benefit(left_size: u64, right_size: u64, match_rate: f64) -> f64 {
    let anti_join = left_size as f64 * right_size as f64;
    let not_exists = left_size as f64 * right_size as f64 * match_rate * 0.5; // Short circuit
    (anti_join - not_exists) / anti_join
}
```

**Typical benefit**: 20-40% with early termination

## Test Cases

### Positive: NOT EXISTS filter

```sql
SELECT * FROM customers c
WHERE NOT EXISTS (
    SELECT 1 FROM orders o WHERE o.customer_id = c.id
);

-- Short-circuit on first matching order
```

### Positive: NOT IN subquery

```sql
SELECT * FROM products
WHERE category_id NOT IN (SELECT id FROM archived_categories);

-- Fast lookup with early exit
```

### Negative: Complex correlation

```sql
SELECT * FROM orders o
WHERE NOT EXISTS (
    SELECT 1 FROM refunds r
    WHERE r.order_id = o.id AND r.amount >= o.total * 0.5
);

-- Cannot simplify: complex correlated condition
```

## References

- PostgreSQL: Anti-join optimization with hash table
- MySQL: NOT EXISTS subquery execution
- DuckDB: Anti-join with bloom filter rejection
