# Range Scan

## Description

A range scan retrieves multiple rows within a bounded interval on an indexed column. Common for date ranges, numeric intervals, and alphabetic ranges.

## Use Cases

- Orders in a date range
- Products in a price range
- Users created since date X
- Paginated result sets (OFFSET/LIMIT)
- Time-series data retrieval

## Relational Algebra

$$
\sigma_{\text{low} \leq A \leq \text{high}}(R)
$$

With ordering (common case):

$$
\tau_A(\sigma_{\text{low} \leq A \leq \text{high}}(R))
$$

General range predicate:

$$
\sigma_{\theta}(R) \quad \text{where } \theta \in \{<, \leq, \geq, >, \text{BETWEEN}\}
$$

## How Ra Optimizes

### Index Range Scan

**Rule:** `physical/index-range-scan`

Ra chooses index range scan when:

$$
\text{Cost}_{\text{index}} < \text{Cost}_{\text{seq}}
$$

Where:

$$
\text{Cost}_{\text{index}} = \log_B(|R|) + \text{sel}(\theta) \times |R| \times C_{\text{heap}}
$$

$$
\text{Cost}_{\text{seq}} = |R| \times C_{\text{seq}}
$$

**Selectivity threshold:** Typically index is used when $\text{sel}(\theta) < 0.05$ (5% of table).

### Sort Elimination

**Rule:** `logical/sort-elimination-via-index`

If query has `ORDER BY` on the range column and index is sorted:

$$
\tau_A(\sigma_{A \in [l, h]}(R)) \equiv \sigma_{A \in [l, h]}(R) \quad \text{with index on } A
$$

The sort operator is eliminated because index scan returns rows in order.

### Bitmap Index Scan

**Rule:** `physical/bitmap-index-scan`

For low selectivity or multiple predicates:

$$
\sigma_{\theta_1 \land \theta_2}(R) \rightarrow \text{BitmapAnd}(\text{BitmapScan}(\theta_1), \text{BitmapScan}(\theta_2))
$$

Ra uses bitmap scan when:
- Selectivity is 5-20% (between index and seq scan thresholds)
- Multiple predicates can be combined
- Reduces random I/O

## Statistics API

```rust
use ra_optimizer::{Statistics, ColumnStatistics, Histogram};

// Table stats
optimizer.add_table_stats("orders", Statistics {
    row_count: 10_000_000,
    block_count: 100_000,
    average_row_width: 150,
});

// Column with range predicate
optimizer.add_column_stats("orders", "created_at", ColumnStatistics {
    distinct_count: 500_000,  // One order per ~20 seconds
    null_fraction: 0.0,
    min_value: Some("2020-01-01"),
    max_value: Some("2024-12-31"),
    histogram: Some(Histogram {
        bounds: vec![
            "2020-01-01", "2021-01-01", "2022-01-01",
            "2023-01-01", "2024-01-01", "2024-12-31"
        ],
        frequencies: vec![0.15, 0.20, 0.25, 0.25, 0.15],
    }),
});

// Index for range scan
optimizer.add_index("orders", Index {
    name: "orders_created_at_idx",
    columns: vec!["created_at"],
    index_type: IndexType::BTree,
    unique: false,
});
```

### Histogram-Based Selectivity

Ra estimates selectivity using histogram:

$$
\text{sel}(\text{date BETWEEN } l \text{ AND } h) = \sum_{i: b_i \in [l,h]} f_i
$$

Where $b_i$ are histogram bounds and $f_i$ are frequencies.

## Examples

### Date Range Query

```sql
SELECT order_id, total, customer_id
FROM orders
WHERE created_at BETWEEN '2024-01-01' AND '2024-01-31'
ORDER BY created_at;
```

**Relational Algebra:**

$$
\tau_{\text{created\_at}}(\pi_{\text{order\_id}, \text{total}, \text{customer\_id}}(\sigma_{\text{created\_at} \in [\text{'2024-01-01'}, \text{'2024-01-31'}]}(\text{orders})))
$$

**Ra Plan:**

```
Project [order_id, total, customer_id]
  IndexRangeScan [orders.created_at_idx]
    Filter: created_at BETWEEN '2024-01-01' AND '2024-01-31'
    (returns sorted by created_at, sort eliminated)
```

**Cost Estimate:**
- Rows: 10M × (31 days / 1826 days) = ~170K rows
- Selectivity: 1.7%
- I/Os: log(10M) + 170K ≈ 170K (mostly heap fetches)

### Numeric Range

```sql
SELECT product_name, price
FROM products
WHERE price BETWEEN 10.00 AND 50.00;
```

**Ra Decision:**
- If selectivity < 5%: Index range scan
- If selectivity 5-20%: Bitmap index scan
- If selectivity > 20%: Sequential scan

### Multiple Range Predicates

```sql
SELECT *
FROM events
WHERE created_at BETWEEN '2024-01-01' AND '2024-12-31'
  AND user_id BETWEEN 1000 AND 2000;
```

**Ra Plan (if both columns indexed):**

```
BitmapHeapScan [events]
  BitmapAnd
    BitmapIndexScan [events.created_at_idx]
      (created_at BETWEEN '2024-01-01' AND '2024-12-31')
    BitmapIndexScan [events.user_id_idx]
      (user_id BETWEEN 1000 AND 2000)
```

**Selectivity Calculation:**

$$
\text{sel}_{\text{combined}} = \text{sel}(\text{date}) \times \text{sel}(\text{user\_id})
$$

Assuming independence. Ra's correlation-aware estimator adjusts if columns are correlated.

### Open-Ended Range

```sql
-- Recent orders
SELECT * FROM orders
WHERE created_at > NOW() - INTERVAL '7 days';
```

**Ra Plan:**

```
IndexRangeScan [orders.created_at_idx]
  Filter: created_at > '2024-03-14'  -- Constant folded
```

## Index-Only Scan Optimization

If index covers all needed columns:

```sql
-- Index: CREATE INDEX orders_date_total_idx ON orders(created_at, total)
SELECT created_at, total
FROM orders
WHERE created_at BETWEEN '2024-01-01' AND '2024-01-31';
```

**Ra Plan:**

```
IndexOnlyScan [orders.date_total_idx]
  Filter: created_at BETWEEN '2024-01-01' AND '2024-01-31'
```

**Cost Improvement:** Eliminates heap fetches, reducing cost by ~50%.

## Pagination Pattern

```sql
SELECT order_id, total
FROM orders
WHERE created_at >= '2024-01-01'
ORDER BY created_at
LIMIT 20 OFFSET 100;
```

**Ra Plan:**

```
Limit (20) Offset (100)
  IndexRangeScan [orders.created_at_idx]
    Filter: created_at >= '2024-01-01'
    (returns sorted, sort eliminated)
```

**Optimization:** Ra pushes `LIMIT` down to scan, stopping early.

## Anti-Patterns

### 1. Implicit Type Conversion

❌ **Bad:**
```sql
-- If created_at is DATE
SELECT * FROM orders WHERE created_at BETWEEN '2024-01-01 00:00:00' AND '2024-01-31 23:59:59';
```

Timestamp strings on DATE column may prevent index usage.

✅ **Good:**
```sql
SELECT * FROM orders WHERE created_at BETWEEN '2024-01-01' AND '2024-01-31';
```

### 2. Function on Indexed Column

❌ **Bad:**
```sql
SELECT * FROM orders WHERE DATE(created_at) = '2024-01-15';
```

✅ **Good:**
```sql
SELECT * FROM orders
WHERE created_at >= '2024-01-15' AND created_at < '2024-01-16';
```

### 3. Inefficient Pagination

❌ **Bad:**
```sql
-- For large offsets (e.g., OFFSET 100000)
SELECT * FROM orders ORDER BY created_at LIMIT 20 OFFSET 100000;
```

Cost grows linearly with offset.

✅ **Good:**
```sql
-- Keyset pagination
SELECT * FROM orders
WHERE created_at > '2024-01-15 10:30:00'  -- Last seen value
ORDER BY created_at
LIMIT 20;
```

## Performance Characteristics

| Selectivity | Preferred Method | Expected Cost |
|-------------|-----------------|---------------|
| < 0.01% | Index scan | ~log(N) |
| 0.01% - 5% | Index range scan | log(N) + sel×N |
| 5% - 20% | Bitmap scan | log(N) + sel×N (clustered) |
| > 20% | Sequential scan | N |

## See Also

- [Point Lookup](point-lookup.md) - Single-value retrieval
- [Top-N Queries](../olap/top-n.md) - LIMIT optimization
- [Index Structures: B-tree](../../index-structures/btree.md) - Range scan mechanics
- [Index Structures: Bitmap](../../index-structures/bitmap.md) - Bitmap index scans
- [Date Range Filters](../temporal/date-range-filters.md) - Temporal range patterns
- [Rule: Bitmap Index Scan](../../../rules/physical/bitmap-index-scan.md)

## References

- PostgreSQL: [Index Scanning](https://www.postgresql.org/docs/current/indexes-bitmap-scans.html)
- MySQL: [Range Optimization](https://dev.mysql.com/doc/refman/8.0/en/range-optimization.html)
- Silberschatz et al., *Database System Concepts*, Ch. 11
