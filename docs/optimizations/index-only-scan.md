# Index-Only Scan Optimization

## Overview

Index-only scans (also called covering index scans) provide 5-10x speedup by reading all needed data directly from an index, eliminating expensive heap table accesses.

## How It Works

When a query needs only columns that are present in an index (either as key columns or INCLUDE columns), the optimizer can rewrite the plan to scan only the index:

```
Before:  Scan(table) → Filter(pred) → Project(cols)
After:   IndexOnlyScan(table, index, cols, pred)
```

### Performance Benefits

1. **Eliminates heap access**: Read directly from B-tree leaf pages
2. **Reduces I/O**: Index pages are ~40% the size of heap pages
3. **Improves cache locality**: Sequential index page access
4. **Skips visibility checks**: No MVCC overhead for covering indexes

Typical speedup ranges:
- **Warm cache**: 5-10x faster than heap scan
- **Cold cache**: 2-5x faster (fewer pages to read)
- **Point queries**: 20x+ faster

## Requirements

All conditions must be satisfied:

1. ✅ All projected columns are in the index (key or INCLUDE)
2. ✅ All filter predicate columns are in the index
3. ✅ Index is not partial, or query satisfies the partial predicate
4. ✅ No NULL visibility issues (PostgreSQL-specific)

## Example

### Schema

```sql
CREATE TABLE orders (
    order_id BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL,
    order_date DATE NOT NULL,
    amount DECIMAL(10, 2) NOT NULL,
    status VARCHAR(20) NOT NULL,
    notes TEXT
);

-- Covering index for customer queries
CREATE INDEX idx_orders_customer
    ON orders(customer_id, order_date)
    INCLUDE (order_id, amount, status);
```

### Query Using Index-Only Scan

```sql
SELECT order_id, customer_id, order_date, amount, status
FROM orders
WHERE customer_id = 12345
  AND order_date >= '2024-01-01'
ORDER BY order_date DESC;
```

**Plan:**
```
IndexOnlyScan(orders, idx_orders_customer,
              [order_id, customer_id, order_date, amount, status],
              customer_id = 12345 AND order_date >= '2024-01-01')
```

**Why it works:**
- All SELECT columns: ✅ in index (key + INCLUDE)
- All WHERE columns: ✅ in index (customer_id, order_date are keys)
- All ORDER BY columns: ✅ in index (order_date is a key)

### Query That Cannot Use Index-Only Scan

```sql
SELECT order_id, notes
FROM orders
WHERE customer_id = 12345;
```

**Plan:**
```
Project([order_id, notes],
    Filter(customer_id = 12345,
        Scan(orders)))
```

**Why it doesn't work:**
- `notes` column is NOT in the index
- Must access heap to fetch `notes`

## Cost Model

The index-only scan cost is calculated as:

```rust
cost = base_cost + btree_descent + filter_evaluation

where:
  base_cost = heap_size * 0.4 * 0.5 * seq_page_cost * confidence
  btree_descent = log2(index_pages) * rand_page_cost * 0.001
  filter_evaluation = row_count * selectivity * tuple_cost * 0.0001

  0.4 = index size factor (indexes are 40% of heap)
  0.5 = cache hit factor (indexes are well-cached)
  confidence = 1.0 for fresh stats, up to 2.0 for stale stats
```

### Example Cost Calculation

For a 1M row table (100 bytes/row):

```
Heap size: 1,000,000 * 100 / 1MB = 95.4 MB

Full Scan Cost:
  = 95.4 * seq_page_cost * confidence
  = 95.4 * 0.5 * 1.0
  = 47.7

Index-Only Scan Cost:
  = 95.4 * 0.4 * 0.5 * 0.5 * 1.0 + log2(4867) * 1.0 * 0.001
  = 9.54 + 0.012
  = 9.55

Speedup: 47.7 / 9.55 = 5.0x
```

## Implementation Details

### Algebra Representation

The `IndexOnlyScan` operator in `ra-core/algebra.rs`:

```rust
pub enum RelExpr {
    // ...
    IndexOnlyScan {
        table: String,
        index: String,         // Index name or "auto" for deferred resolution
        columns: Vec<ProjectionColumn>,
        predicate: Expr,
    },
}
```

### Rewrite Rules

In `ra-engine/covering_index.rs`:

```rust
// Forward rule
rewrite!("project-filter-scan-to-index-only";
    "(project ?cols (filter ?pred (scan ?table)))" =>
    "(index-only-scan ?table auto ?cols ?pred)"
);

// Reverse rule (bidirectional search)
rewrite!("index-only-to-project-filter-scan";
    "(index-only-scan ?table auto ?cols ?pred)" =>
    "(project ?cols (filter ?pred (scan ?table)))"
);
```

### Cost Function

In `ra-engine/cost.rs`, two methods handle costing:

1. **`IntegratedCostModel::index_only_scan_cost`**: Detailed cost calculation used by the high-level optimizer
2. **`IntegratedCostFn::cost` (IndexOnlyScan case)**: Simplified cost used by the egg extractor

## Index Design Best Practices

### 1. Order Key Columns by Selectivity

```sql
-- Good: most selective column first
CREATE INDEX idx_orders_date_status
    ON orders(order_date, status);

-- Less optimal: less selective column first
CREATE INDEX idx_orders_status_date
    ON orders(status, order_date);
```

### 2. Use INCLUDE for Non-Key Columns

```sql
-- Efficient covering index
CREATE INDEX idx_orders_customer
    ON orders(customer_id)
    INCLUDE (order_id, amount, status);
```

### 3. Avoid Over-Including

```sql
-- Bad: including rarely-queried large columns
CREATE INDEX idx_orders_customer_bad
    ON orders(customer_id)
    INCLUDE (notes, description, metadata);  -- Bloats index!
```

### 4. Consider Query Patterns

Analyze your workload and create indexes that cover common access patterns:

```sql
-- For: SELECT * FROM orders WHERE customer_id = ? AND status = 'pending'
CREATE INDEX idx_orders_customer_pending
    ON orders(customer_id, status)
    INCLUDE (order_id, order_date, amount);
```

## Verification

### Check if Index-Only Scan is Used

Use the RA optimizer explain:

```rust
use ra_engine::{Optimizer, IntegratedCostModel};
use ra_parser::parse;

let sql = "SELECT id, name FROM users WHERE status = 'active'";
let plan = parse(sql)?;
let optimizer = Optimizer::new(cost_model);
let optimized = optimizer.optimize(plan)?;

println!("{}", optimized.explain());
// Expected output:
// IndexOnlyScan(users, idx_status_name, [id, name], status = 'active')
```

### Benchmark

Compare execution times:

```rust
use std::time::Instant;

// Without covering index
let start = Instant::now();
let result = db.execute("SELECT id, name FROM users WHERE status = 'active'")?;
let without_index = start.elapsed();

// With covering index
db.execute("CREATE INDEX idx_users_status_name ON users(status) INCLUDE (id, name)")?;
let start = Instant::now();
let result = db.execute("SELECT id, name FROM users WHERE status = 'active'")?;
let with_index = start.elapsed();

println!("Without covering index: {:?}", without_index);
println!("With covering index: {:?}", with_index);
println!("Speedup: {:.1}x", without_index.as_secs_f64() / with_index.as_secs_f64());
```

## Limitations

### PostgreSQL NULL Visibility

In PostgreSQL, index-only scans require the visibility map to be fully updated. Tables with frequent updates may fall back to heap scans if visibility information is not available.

**Workaround**: Run `VACUUM` regularly to update the visibility map.

### Partial Indexes

Queries must satisfy the partial index predicate:

```sql
CREATE INDEX idx_active_orders
    ON orders(order_date)
    WHERE status = 'active';

-- This can use index-only scan
SELECT order_date FROM orders WHERE status = 'active';

-- This CANNOT use the index
SELECT order_date FROM orders WHERE status = 'pending';
```

### Expression Indexes

Function-based indexes can support covering scans:

```sql
CREATE INDEX idx_orders_year
    ON orders(EXTRACT(YEAR FROM order_date))
    INCLUDE (order_id, amount);

-- Can use index-only scan
SELECT order_id, amount
FROM orders
WHERE EXTRACT(YEAR FROM order_date) = 2024;
```

## References

- [PostgreSQL Index-Only Scans](https://www.postgresql.org/docs/current/indexes-index-only-scans.html)
- [SQL Server Covering Indexes](https://learn.microsoft.com/en-us/sql/relational-databases/indexes/create-indexes-with-included-columns)
- RFC 0068: Calibrated Cost Model
- `ra-engine/covering_index.rs`: Implementation
- `ra-engine/cost.rs`: Cost model
