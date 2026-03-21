# Point Lookup

## Description

A point lookup retrieves a single row (or small set of rows) by primary key or unique index. This is the most fundamental OLTP pattern, optimized for sub-millisecond latency.

## Use Cases

- Fetching user profile by ID
- Order details lookup
- Session retrieval
- Cache fills
- REST API GET endpoints

## Relational Algebra

$$
\sigma_{\text{id} = k}(R)
$$

Where:
- $R$ is the base relation (table)
- $k$ is a constant value (the lookup key)
- The predicate is an equality on a unique column

With projection (typical case):

$$
\pi_{A_1, \ldots, A_n}(\sigma_{\text{id} = k}(R))
$$

## How Ra Optimizes

### Automatic Transformations

1. **Index Selection**
   - Rule: `physical/index-scan-on-point-lookup`
   - Chooses index scan over sequential scan
   - Prefers covering index if all columns are in SELECT

2. **Projection Pushdown**
   - Rule: `logical/pushdown/project-through-select`
   - Pushes projection below selection
   - Reduces data movement

3. **Constant Folding**
   - Rule: `logical/constant-folding`
   - Evaluates constant expressions at optimization time
   - Example: `WHERE id = 42 + 1` becomes `WHERE id = 43`

### Cost Model Considerations

Ra's cost model for point lookups:

$$
\text{Cost}_{\text{point}} = \begin{cases}
C_{\text{index}} + C_{\text{heap}} & \text{if non-covering index} \\
C_{\text{index}} & \text{if covering index} \\
C_{\text{seq}} \times |R| & \text{if no index}
\end{cases}
$$

Where:
- $C_{\text{index}} \approx \log_B(|R|) \times C_{\text{io}}$ (B-tree depth)
- $C_{\text{heap}} = C_{\text{io}}$ (single page fetch)
- $C_{\text{seq}}$ is sequential scan cost per row

**Threshold:** Ra uses index scan when:

$$
\log_B(|R|) + 1 < \text{sel}(\theta) \times |R|
$$

For point lookups, $\text{sel}(\theta) = \frac{1}{|R|}$, so index is always chosen.

### Index Type Selection

| Index Type | When Ra Uses It | Cost Formula |
|------------|----------------|--------------|
| B-tree | General case, sortable keys | $O(\log_B N)$ |
| Hash | Exact equality only, high cardinality | $O(1)$ average |
| Covering | All SELECT columns in index | Eliminates heap fetch |

## Statistics API

Ra needs these statistics for accurate optimization:

```rust
use ra_optimizer::{Statistics, ColumnStatistics};

// Table-level stats
optimizer.add_table_stats("users", Statistics {
    row_count: 1_000_000,
    block_count: 10_000,
    average_row_width: 200,
});

// Column stats for lookup column
optimizer.add_column_stats("users", "id", ColumnStatistics {
    distinct_count: 1_000_000,  // Unique column
    null_fraction: 0.0,
    min_value: Some(1),
    max_value: Some(1_000_000),
    most_common_values: vec![],
});

// Index metadata
optimizer.add_index("users", Index {
    name: "users_pkey",
    columns: vec!["id"],
    index_type: IndexType::BTree,
    unique: true,
    covering_columns: vec!["id", "name", "email"],  // Optional
});
```

## Examples

### Basic Point Lookup

```sql
SELECT name, email, created_at
FROM users
WHERE id = 12345;
```

**Relational Algebra:**

$$
\pi_{\text{name}, \text{email}, \text{created\_at}}(\sigma_{\text{id} = 12345}(\text{users}))
$$

**Ra Plan:**

```
Project [name, email, created_at]
  IndexScan [users.pkey] (id = 12345)
```

**Cost:** 3-4 I/Os (B-tree lookup + heap fetch)

### Covering Index Optimization

```sql
-- If index exists: CREATE INDEX users_covering_idx ON users(id, name, email, created_at)
SELECT name, email, created_at
FROM users
WHERE id = 12345;
```

**Ra Plan:**

```
IndexOnlyScan [users.covering_idx] (id = 12345)
```

**Cost:** 3 I/Os (B-tree lookup only, no heap fetch)

### Multi-column Point Lookup

```sql
SELECT product_name, price
FROM order_items
WHERE order_id = 5000 AND item_id = 3;
```

**Relational Algebra:**

$$
\pi_{\text{product\_name}, \text{price}}(\sigma_{\text{order\_id} = 5000 \land \text{item\_id} = 3}(\text{order\_items}))
$$

**Ra Plan:**

```
Project [product_name, price]
  IndexScan [order_items.pkey] (order_id = 5000 AND item_id = 3)
```

### With Secondary Index

```sql
-- Index: CREATE INDEX users_email_idx ON users(email)
SELECT id, name
FROM users
WHERE email = 'user@example.com';
```

**Ra Decision Tree:**

1. If `email` is unique: Use `users_email_idx` (one row expected)
2. If `email` is non-unique but selective: Use index
3. If `email` has low cardinality: Sequential scan

**Plan (unique email):**

```
Project [id, name]
  IndexScan [users.email_idx] (email = 'user@example.com')
    -> HeapFetch [users]
```

## Anti-Patterns

### 1. Function on Indexed Column

❌ **Bad:**
```sql
SELECT * FROM users WHERE UPPER(email) = 'USER@EXAMPLE.COM';
```

The function prevents index usage.

✅ **Good:**
```sql
-- Create functional index
CREATE INDEX users_email_upper_idx ON users(UPPER(email));
-- Or normalize at insert time
SELECT * FROM users WHERE email = 'user@example.com';
```

### 2. Type Mismatch

❌ **Bad:**
```sql
-- If id is integer
SELECT * FROM users WHERE id = '12345';  -- String literal
```

Implicit cast may prevent index usage.

✅ **Good:**
```sql
SELECT * FROM users WHERE id = 12345;  -- Correct type
```

### 3. OR with Non-indexed Columns

❌ **Bad:**
```sql
SELECT * FROM users WHERE id = 123 OR name = 'Alice';
```

Forces sequential scan if `name` isn't indexed.

✅ **Good:**
```sql
-- Use UNION ALL with two separate lookups
SELECT * FROM users WHERE id = 123
UNION ALL
SELECT * FROM users WHERE name = 'Alice' AND id != 123;
```

## Performance Characteristics

| Scenario | Expected Performance | Notes |
|----------|---------------------|-------|
| Primary key lookup | 1-3 ms | 3-4 I/Os typical |
| Covering index | < 1 ms | No heap fetch |
| Secondary index | 2-5 ms | Extra indirection |
| No index (1M rows) | 100+ ms | Sequential scan |

## See Also

- [Range Scans](range-scan.md) - Bounded index scans
- [Index Structures: B-tree](../../index-structures/btree.md) - B-tree mechanics
- [Index Structures: Covering](../../index-structures/covering.md) - Index-only scans
- [Schema Patterns: Normalized](../../schema-patterns/normalized.md) - Primary key design
- [Rule: Index Selection](../../../rules/physical/index-scan-selection.md)
- [Example: Index Selection](../../../examples/index-selection.md)

## References

- Ramakrishnan & Gehrke, *Database Management Systems*, Ch. 8
- Garcia-Molina et al., *Database Systems: The Complete Book*, Ch. 13
- PostgreSQL documentation: [Index Types](https://www.postgresql.org/docs/current/indexes-types.html)
