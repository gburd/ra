# Index Selection Example

This example demonstrates how RA automatically identifies and uses the best indexes for query optimization.

## The Query

```sql
SELECT o.order_id, o.order_date, c.name, c.email
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.status = 'pending'
  AND o.order_date >= '2024-01-01'
  AND c.country = 'USA'
ORDER BY o.order_date DESC
LIMIT 100;
```

## Available Indexes

```sql
-- Existing indexes in the database
CREATE INDEX idx_orders_status ON orders(status);
CREATE INDEX idx_orders_date ON orders(order_date);
CREATE INDEX idx_orders_customer ON orders(customer_id);
CREATE INDEX idx_orders_covering ON orders(status, order_date)
  INCLUDE (order_id, customer_id);
CREATE INDEX idx_customers_country ON customers(country);
CREATE INDEX idx_customers_pk ON customers(id);
```

## Index Selection Process

### 1. Index Matching

RA identifies applicable indexes for each predicate:

```yaml
Predicate Analysis:
  o.status = 'pending':
    - idx_orders_status (exact match)
    - idx_orders_covering (prefix match)

  o.order_date >= '2024-01-01':
    - idx_orders_date (range scan)
    - idx_orders_covering (second column, requires status equality)

  c.country = 'USA':
    - idx_customers_country (exact match)

  o.customer_id = c.id (join):
    - idx_orders_customer (join index)
    - idx_customers_pk (primary key)
```

### 2. Cost Estimation

RA estimates cost for different index strategies:

```yaml
Strategy 1: Individual indexes
  1. idx_orders_status -> 10K rows
  2. idx_orders_date -> 50K rows
  3. Intersect results -> 500 rows
  4. Fetch from heap -> 500 seeks
  Total Cost: 525 units

Strategy 2: Covering index
  1. idx_orders_covering (status, order_date) -> 500 rows
  2. Index-only scan (no heap access needed)
  Total Cost: 50 units  <- WINNER

Strategy 3: Table scan with filter
  1. Scan orders -> 1M rows
  2. Apply filters -> 500 rows
  Total Cost: 10,000 units
```

### 3. Join Method Selection

Based on available indexes:

```yaml
Nested Loop Join (with index):
  - Scan filtered orders (500 rows)
  - For each: probe idx_customers_pk
  - Cost: 500 * 1 = 500 lookups

Hash Join:
  - Scan filtered customers (50K rows)
  - Build hash table
  - Probe with 500 orders
  - Cost: 50K + 500 = 50,500 operations

RA chooses: Nested Loop with index
```

## Optimized Execution Plan

```
Limit(100)
  `---- Sort(o.order_date DESC)
      `---- NestedLoopJoin(o.customer_id = c.id)
          |---- IndexOnlyScan(idx_orders_covering)
          |   `---- Conditions: status='pending' AND date>='2024-01-01'
          `---- IndexSeek(idx_customers_pk)
              `---- Filter(country = 'USA')
```

## Running the Example

```bash
# Analyze index usage
ra-cli analyze-indexes \
  "SELECT o.order_id, o.order_date, c.name, c.email \
   FROM orders o JOIN customers c ON o.customer_id = c.id \
   WHERE o.status = 'pending' AND o.order_date >= '2024-01-01' \
   AND c.country = 'USA' \
   ORDER BY o.order_date DESC LIMIT 100"

# Suggest missing indexes
ra-cli suggest-indexes \
  --workload queries.sql \
  --output suggested_indexes.sql

# Compare plans with/without indexes
ra-cli compare \
  --with-indexes \
  --without-indexes \
  "YOUR_QUERY"
```

## Index Selection Strategies

### Covering Index Detection

```sql
-- Query only needs columns in the index
SELECT status, COUNT(*)
FROM orders
WHERE order_date >= '2024-01-01'
GROUP BY status;

-- RA detects idx_orders_covering covers all needed columns
-- Result: Index-only scan, no table access needed
```

### Bitmap Index Combination

```sql
-- Multiple equality predicates
SELECT * FROM products
WHERE color = 'red'
  AND size = 'large'
  AND category = 'clothing';

-- RA combines bitmap indexes:
BitmapOr
  |---- BitmapIndexScan(idx_color)
  |---- BitmapIndexScan(idx_size)
  `---- BitmapIndexScan(idx_category)
```

### Partial Index Usage

```sql
-- Partial index on hot data
CREATE INDEX idx_recent_orders
ON orders(customer_id)
WHERE order_date >= '2024-01-01';

-- Query matches partial index condition
SELECT * FROM orders
WHERE customer_id = 123
  AND order_date >= '2024-03-01';

-- RA uses partial index (smaller, more efficient)
```

## Performance Impact

Real-world improvements from proper index selection:

| Scenario | Without Indexes | With RA Selection | Improvement |
|----------|----------------|-------------------|-------------|
| Point lookup | 1000ms | 1ms | 1000x |
| Range scan | 5000ms | 50ms | 100x |
| Covering index | 500ms | 5ms | 100x |
| Join with index | 10000ms | 100ms | 100x |
| Bitmap combination | 2000ms | 20ms | 100x |

## Advanced Features

### Index Advisor

RA can suggest new indexes based on workload:

```bash
# Analyze query workload
ra-cli advisor \
  --workload production_queries.log \
  --constraints "max_indexes=10,max_space=1GB"

# Output:
# Recommended indexes (ranked by benefit):
# 1. CREATE INDEX idx_orders_opt1 ON orders(status, order_date)
#    INCLUDE (order_id, customer_id);
#    Benefit: 45% workload improvement
#    Space: 120MB
```

### Index Intersection

```sql
-- No perfect index exists
SELECT * FROM orders
WHERE status = 'pending'
  AND priority = 'high'
  AND region = 'EMEA';

-- RA intersects multiple indexes
IndexIntersection
  |---- IndexScan(idx_status)
  |---- IndexScan(idx_priority)
  `---- IndexScan(idx_region)
```

### Function-Based Indexes

```sql
-- Index on expression
CREATE INDEX idx_year ON orders((EXTRACT(YEAR FROM order_date)));

-- Query uses same expression
SELECT * FROM orders
WHERE EXTRACT(YEAR FROM order_date) = 2024;

-- RA recognizes and uses function-based index
```

## Related Topics

- **[Covering Indexes](../features/covering-index.md)** - Index-only scans
- **[Bitmap Indexes](../features/bitmap-index-scan.md)** - Set operations on indexes
- **[Cost Models](../guides/cost-models.md)** - How index costs are estimated
- **[Statistics](../features/statistics-timeline-format.md)** - Cardinality estimation