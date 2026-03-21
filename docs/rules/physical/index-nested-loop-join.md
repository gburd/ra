# Rule: Index Nested Loop Join

**Category:** physical/join-algorithms
**File:** `rules/physical/join-algorithms/index-nested-loop-join.rra`

## Metadata

- **ID:** `index-nested-loop-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** join, index, nested-loop
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner (= ?lcol ?rcol) ?left ?right)"
    description: "Equi-join using index on inner table"
  - type: "predicate"
    condition: "has_index(?right, ?rcol)"
    description: "Index must exist on the inner (right) table's join column"
  - type: "fact"
    fact_type: "statistics.cardinality"
    table: "?left"
    comparator: "<"
    threshold: 100000
    optional: true
    description: "Outer table should be small for efficient index lookups"
```


# Index Nested Loop Join

## Description

Uses index on inner table to efficiently lookup matching rows for each outer row.

**When to apply**: Join with index on inner table's join key and small outer table.

**Why it works**: Index lookup is O(log m) instead of O(m) scan; optimal for small outer tables.

## Relational Algebra

```algebra
join[R.key = S.indexed_key](R, S)
  -> for each r in R:
       index_lookup(S, r.key)

Cost: |R| * (log|S| + matches)
```

## Implementation

```rust
rw!("use-index-nested-loop";
    "(join (= ?outer_key ?inner_key) ?outer ?inner)" =>
    "(index-nested-loop ?outer_key ?inner_key ?outer ?inner)"
    if has_index("?inner", "?inner_key") && is_small("?outer")
),
```

## Cost Model

```rust
fn cost(outer_size: u64, inner_size: u64, selectivity: f64) -> f64 {
    let lookups = outer_size as f64;
    let index_cost = (inner_size as f64).log2();
    let matches = outer_size as f64 * selectivity;
    lookups * index_cost + matches
}

fn benefit_over_nested_loop(outer: u64, inner: u64, sel: f64) -> f64 {
    let nested = outer as f64 * inner as f64;
    let indexed = cost(outer, inner, sel);
    (nested - indexed) / nested
}
```

**Typical benefit**: 50-90% when outer table is small

## Test Cases

### Positive: Small outer, indexed inner

```sql
SELECT * FROM recent_orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.date > CURRENT_DATE - 7;

-- 100 recent orders, index on customers(id)
-- Cost: 100 * log(1M) ≈ 2000 operations
```

### Positive: Parameterized query

```sql
SELECT * FROM orders
JOIN products ON orders.product_id = products.id
WHERE orders.id = ?;

-- Single order lookup: 1 * log(products)
```

### Negative: Large outer table

```sql
SELECT * FROM all_orders o
JOIN customers c ON o.customer_id = c.id;

-- 10M orders: index nested loop too expensive
-- Better: hash join
```

### Negative: No index on inner

```sql
SELECT * FROM orders o
JOIN order_notes n ON o.id = n.order_id;

-- No index on order_notes.order_id
-- Must use nested loop or hash join
```

## References

- PostgreSQL: Nested loop with inner index scan
- MySQL: Index nested-loop join optimization
- Oracle: Nested loops join with index access
