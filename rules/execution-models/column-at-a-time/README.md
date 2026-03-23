# Column-at-a-Time Execution Model (X100)

Rules specific to column-at-a-time / X100 execution as implemented in **MonetDB** and **VectorWise (Actian Vector)**.

## Overview

The X100 execution model processes entire columns at once with late materialization, pioneered by MonetDB. Unlike traditional tuple-at-a-time or even vectorized (batch) processing, it works with column positions and defers tuple reconstruction until absolutely necessary.

## Core Concepts

### 1. Column-Oriented Storage

Data stored in columns (vertical partitioning):
```
orders table:
  order_id:    [100, 101, 102, 103, 104]
  customer_id: [1,   2,   1,   3,   2]
  amount:      [100, 1500, 200, 2000, 300]
  region:      ['US','EU','US','EU','US']
```

### 2. Late Materialization

Don't construct tuples until needed. Work with positions (offsets):

```rust
// Traditional (early materialization):
for tuple in scan(orders) {
    if tuple.amount > 1000 {
        result.push(tuple)  // Full tuple copied
    }
}

// Column-at-a-time (late materialization):
positions = select_gt(amount_col, 1000)  // [1, 3]
// Only now materialize needed columns:
result_amount = gather(amount_col, positions)     // [1500, 2000]
result_customer = gather(customer_col, positions) // [2, 3]
// Other columns never touched!
```

### 3. Cache-Conscious Operators

Operators process entire columns that fit in CPU cache:
- L1 cache: 32-64 KB -> ~4K-8K i32 values
- L2 cache: 256-512 KB -> ~32K-64K i32 values
- L3 cache: 8-32 MB -> ~1M-4M i32 values

Sequential access patterns enable excellent prefetching.

### 4. SIMD Vectorization

Process 4-16 values per instruction:
```c
// Select values > 1000 using SIMD
__m256i threshold = _mm256_set1_epi32(1000);
__m256i values = _mm256_loadu_si256(&amount[i]);
__m256i mask = _mm256_cmpgt_epi32(values, threshold);
// Process 8 values in one instruction!
```

## Optimization Rules for Column-at-a-Time

### 1. Late Materialization

**Rule:** `late-materialization.rra`

Defer tuple reconstruction as long as possible:

```sql
SELECT customer_name, order_total
FROM orders
WHERE amount > 1000 AND region = 'US'

Execution Plan:
1. pos₁ = select(amount_col, > 1000)      // positions [1,3,7]
2. pos₂ = select(region_col, = 'US')      // positions [0,2,3,4]
3. positions = intersect(pos₁, pos₂)      // positions [3]
4. names  = gather(customer_name_col, positions)  // Late!
5. totals = gather(order_total_col, positions)    // Late!
```

**Benefit:** Only materialize 1 row instead of scanning all columns for all rows.

### 2. Column Pruning (Aggressive)

**Rule:** `column-pruning-aggressive.rra`

Eliminate unused columns early - critical for columnar:

```sql
-- Query only needs 2 columns
SELECT customer_id, amount FROM orders

-- Bad: Read all columns (8 columns $\times$ 1M rows)
-- Good: Read only customer_id, amount (2 columns $\times$ 1M rows)
-- Savings: 6M column reads avoided
```

**Benefit:** 4x reduction in I/O for this example.

### 3. Filter Ordering by Selectivity

**Rule:** `filter-ordering-by-selectivity.rra`

Apply most selective filters first to minimize subsequent processing:

```sql
WHERE region = 'US'      -- selectivity 0.3 (30% match)
  AND amount > 1000      -- selectivity 0.01 (1% match)
  AND status = 'SHIPPED' -- selectivity 0.5 (50% match)

Optimal order:
1. amount > 1000         -- Eliminates 99%, cheapest to check
2. region = 'US'         -- Eliminates 70% of remaining
3. status = 'SHIPPED'    -- Final 50% check
```

**Benefit:** Process fewer rows in each subsequent filter.

### 4. Position-Based Join

**Rule:** `position-based-join.rra`

Join on positions before materializing tuples:

```sql
SELECT o.order_id, c.name
FROM orders o JOIN customers c ON o.customer_id = c.id

Column-at-a-time execution:
1. pos_orders = scan(orders.customer_id)      // positions [0,1,2,...]
2. pos_customers = lookup(customers, customer_id)  // matched positions
3. joined_positions = match(pos_orders, pos_customers)
4. order_ids = gather(orders.order_id, joined_positions)  // Late!
5. names = gather(customers.name, joined_positions)       // Late!
```

**Benefit:** Join operates on positions (integers), not full tuples.

### 5. Column Cracking

**Rule:** `column-cracking.rra`

Adaptive indexing: partition data during first query execution:

```sql
-- First query with filter:
SELECT * FROM orders WHERE region = 'US'

-- MonetDB cracks the region column:
Before: [EU, US, EU, US, EU, US, EU, US]
After:  [US, US, US, US | EU, EU, EU, EU]
         up US partition | EU partition up

-- Remembers crack boundaries for future queries
-- Next query on region='US' only scans US partition
```

**Benefit:** Incremental indexing without upfront cost.

### 6. Column Imprints

**Rule:** `column-imprints.rra`

Lightweight bit vector indexes per cache line:

```
Cache line (64 bytes = 16 i32 values):
  values: [100, 150, 1500, 200, 2000, 300, 400, 500, ...]
  imprint: [min=100, max=2000, bit_vector=0b1010...]

Query: amount > 1000
  -> Check imprint: max >= 1000? Yes, scan this cache line
  -> Check next imprint: max < 1000? No, skip entire cache line
```

**Benefit:** Fast elimination of irrelevant cache lines.

### 7. Sideways Cracking

**Rule:** `sideways-cracking.rra`

Reuse cracks from one column for filters on other columns:

```sql
-- Query 1 cracks on region
SELECT * FROM orders WHERE region = 'US'
-- Creates: [US partition | non-US partition]

-- Query 2 uses existing crack
SELECT * FROM orders WHERE amount > 1000 AND region = 'US'
-- Apply amount filter only to US partition (smaller scan)
```

**Benefit:** Compound benefits from multiple cracks.

### 8. Positional Updates

**Rule:** `positional-updates.rra`

Updates using positions for efficient bulk modifications:

```sql
-- Update 1% of rows
UPDATE orders SET status = 'CANCELLED'
WHERE amount > 10000

Column-at-a-time:
1. positions = select(amount_col, > 10000)  // Find affected rows
2. update_at_positions(status_col, positions, 'CANCELLED')
// Other columns untouched
```

**Benefit:** Only update affected column, minimize I/O.

## MonetDB-Specific Features

### Database Cracking

Automatic incremental indexing:

```c
// Crack partitions data on first access
void crack(Column* col, int pivot) {
    // Partition: [values <= pivot | values > pivot]
    // O(n) first time, O(log n) subsequently
}

// Multiple cracks create multi-way partitioning
// [< 1000 | 1000-5000 | > 5000]
```

### Column Imprints

Cache-line aware bit vectors:

```c
struct Imprint {
    int min, max;           // Value range
    uint64_t bitvector;     // Presence bits
};

// Check imprint before scanning cache line
if (imprint.max < query_min || imprint.min > query_max) {
    skip_cache_line();  // Fast elimination
}
```

### Cache-Conscious Algorithms

All operators designed for CPU cache:

```c
// Process column in cache-sized chunks
#define CHUNK_SIZE 8192  // Fits in L2 cache

for (int i = 0; i < n; i += CHUNK_SIZE) {
    process_chunk(&column[i], CHUNK_SIZE);
    // Each chunk processes sequentially
    // Excellent cache hit rate
}
```

## Performance Characteristics

**Scan Performance:**
```
Tuple-at-a-time (Volcano): 12,000 ms
Vectorized (1K batches):    3,500 ms
Column-at-a-time:             800 ms

Speedup: 15x over tuple-at-a-time
Speedup: 4.4x over vectorized
```

**Memory Bandwidth:**
```
10-column table, 100M rows, query projects 2 columns:

Row-store: Read all 10 columns = 4 GB
Column-store: Read 2 columns = 800 MB

Bandwidth saved: 80%
```

**Cache Hit Rates:**
```
Random access (row-store): ~60% L3 miss
Sequential access (column-store): ~95% L3 hit
```

## When to Use Column-at-a-Time Rules

**Best for:**
- OLAP / Analytics workloads
- Large table scans
- Aggregations over few columns
- Columnar storage (Parquet, ORC, Arrow)
- Read-heavy workloads

**Avoid for:**
- OLTP (point queries, updates)
- Queries needing many columns
- Small data (overhead exceeds benefit)
- Write-heavy workloads

## Example Rules to Implement

1. **late-materialization.rra** - Defer tuple reconstruction
2. **column-pruning-aggressive.rra** - Eliminate unused columns
3. **filter-ordering-by-selectivity.rra** - Order filters optimally
4. **position-based-join.rra** - Join on positions
5. **column-cracking.rra** - Adaptive indexing
6. **sideways-cracking.rra** - Reuse cracks across columns
7. **column-imprints.rra** - Cache-line bit vectors
8. **positional-updates.rra** - Bulk updates via positions
9. **cache-conscious-scanning.rra** - Chunk size optimization
10. **column-group-selection.rra** - Choose column groups to store together

## References

**MonetDB Documentation:**
- https://www.monetdb.org/documentation/
- https://www.monetdb.org/documentation-Jan2022/user-guide/optimization/
- https://www.monetdb.org/papers/

**Academic Papers:**
- Boncz, Peter A., et al. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
- Idreos, Stratos, et al. "Database Cracking." CIDR 2007.
- Sidirourgos, Lefteris, et al. "Column Imprints: A Secondary Index Structure." SIGMOD 2013.
- Kersten, Martin L., et al. "The Researcher's Guide to the Data Deluge: Querying a Scientific Database in Just a Few Seconds." VLDB 2011.

**Source Code:**
- MonetDB: https://github.com/MonetDB/MonetDB
- MonetDB/X100: https://homepages.cwi.nl/~boncz/

**Related Systems:**
- VectorWise (Actian Vector): Commercial implementation of X100
- Apache Arrow: Columnar in-memory format with similar principles
