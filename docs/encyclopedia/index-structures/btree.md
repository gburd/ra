# B-tree Indexes

## Description

Balanced tree index structure supporting efficient range queries, point lookups, and sorted access. Default index type in most databases.

## Structure

```
                 [50 | 100]
                /     |     \
         [20|30]   [70|80]   [120]
         /  |  \    /  |  \    /  \
    [10] [25] [35] [60] [75] [90] [110] [130]
     |    |    |    |    |    |     |     |
   [data][...][...]...
```

**Properties:**
- All leaf nodes at same level
- Internal nodes store keys + pointers
- Leaf nodes store keys + data (or row pointers)
- Order $B$: Min $\lceil B/2 \rceil$, max $B$ keys per node

## Mathematical Model

### Tree Height

$$
h = \lceil \log_B(N) \rceil
$$

Where:
- $N$ = number of index entries
- $B$ = branching factor (node capacity)

Typical $B = 100$-$200$ for disk-based indexes.

### Point Lookup Cost

$$
\text{Cost}_{\text{point}} = h \times C_{\text{io}} + C_{\text{heap}}
$$

$$
= \lceil \log_B(N) \rceil \times C_{\text{io}} + C_{\text{heap}}
$$

**Example:** 1M rows, $B=100$:

$$
h = \lceil \log_{100}(1{,}000{,}000) \rceil = 3
$$

Cost: 3 I/Os (index) + 1 I/O (heap) = 4 I/Os.

### Range Scan Cost

$$
\text{Cost}_{\text{range}} = h \times C_{\text{io}} + n_{\text{leaf}} \times C_{\text{io}} + n_{\text{rows}} \times C_{\text{heap}}
$$

Where:
- $h$ = tree height
- $n_{\text{leaf}}$ = leaf pages scanned
- $n_{\text{rows}}$ = matching rows

**Clustered index optimization:**

$$
\text{Cost}_{\text{range, clustered}} = h \times C_{\text{io}} + n_{\text{rows}}/\text{rows\_per\_page} \times C_{\text{io}}
$$

## How Ra Optimizes

### 1. Index vs Sequential Scan

**Rule:** `physical/index-vs-seqscan-selection`

Ra chooses index when:

$$
\text{Cost}_{\text{index}} < \text{Cost}_{\text{seq}}
$$

$$
h + \text{sel} \times N < N \times C_{\text{seq\_ratio}}
$$

Where $C_{\text{seq\_ratio}} \approx 0.1$ (sequential reads 10x cheaper than random).

**Threshold:** Typically index used when $\text{sel} < 0.05$ (5%).

### 2. Index-Only Scan

**Rule:** `physical/index-only-scan`

If all query columns in index (covering index):

$$
\text{Cost}_{\text{index\_only}} = h + n_{\text{leaf}}
$$

No heap access needed.

### 3. Skip Scan

**Rule:** `physical/skip-scan-optimization`

For composite index $(A, B)$ with query on $B$ only:

```sql
-- Index: (category, price)
SELECT * FROM products WHERE price > 100;
```

Ra performs **skip scan**: iterate distinct categories, range scan each.

$$
\text{Cost}_{\text{skip}} = h \times |\text{distinct}(A)| + n_{\text{matches}}
$$

Used when $|\text{distinct}(A)|$ is small.

### 4. Backward Scan

**Rule:** `physical/backward-index-scan`

```sql
SELECT * FROM orders ORDER BY created_at DESC LIMIT 10;
```

Ra scans index backward, avoiding sort.

## Statistics API

```rust
use ra_optimizer::{Index, IndexType, IndexStatistics};

// B-tree index on single column
optimizer.add_index("users", Index {
    name: "users_email_idx",
    columns: vec!["email"],
    index_type: IndexType::BTree,
    unique: true,
    size_pages: 1000,  // Index size
    tree_height: 3,
    clustering_factor: 0.2,  // 0=perfect clustering, 1=random
});

// Composite B-tree index
optimizer.add_index("orders", Index {
    name: "orders_customer_date_idx",
    columns: vec!["customer_id", "order_date"],
    index_type: IndexType::BTree,
    unique: false,
    tree_height: 4,
});

// Index statistics for selectivity
optimizer.add_index_stats("users_email_idx", IndexStatistics {
    distinct_keys: 10_000_000,
    avg_rows_per_key: 1.0,  // Unique index
    null_keys: 0,
});
```

## Examples

### Point Lookup

```sql
SELECT name, email FROM users WHERE id = 12345;
```

**Ra Plan:**

```
IndexScan [users.id_pkey]
  Filter: id = 12345
  -> HeapFetch [name, email]
```

**Cost:**
- Index traversal: 3 I/Os
- Heap fetch: 1 I/O
- Total: 4 I/Os

### Range Query

```sql
SELECT * FROM orders
WHERE order_date BETWEEN '2024-01-01' AND '2024-01-31'
ORDER BY order_date;
```

**Ra Plan:**

```
IndexRangeScan [orders.order_date_idx]
  Filter: order_date BETWEEN '2024-01-01' AND '2024-01-31'
  (returns sorted, no Sort operator needed)
```

**Cost Estimate:**
- Tree height: 4 I/Os
- Leaf pages: ~100 I/Os (for 50K matching rows)
- Heap fetches: 50K I/Os (if non-clustered)
- Total: ~50K I/Os

**With clustered index:**
- Tree height: 4 I/Os
- Sequential heap read: 1K I/Os (50 rows/page)
- Total: ~1K I/Os (50x faster)

### Index-Only Scan

```sql
-- Index: CREATE INDEX orders_date_total_idx ON orders(order_date, total)
SELECT order_date, total
FROM orders
WHERE order_date >= '2024-01-01';
```

**Ra Plan:**

```
IndexOnlyScan [orders.date_total_idx]
  Filter: order_date >= '2024-01-01'
```

**Cost:**
- Tree height: 4 I/Os
- Leaf pages: 100 I/Os
- **No heap access** (saves 50K I/Os)

### Composite Index Usage

```sql
-- Index: CREATE INDEX orders_cust_date_idx ON orders(customer_id, order_date)

-- Query 1: Uses index efficiently
SELECT * FROM orders
WHERE customer_id = 123 AND order_date >= '2024-01-01';

-- Query 2: Uses index partially
SELECT * FROM orders WHERE customer_id = 123;

-- Query 3: Cannot use index efficiently
SELECT * FROM orders WHERE order_date >= '2024-01-01';
```

**Query 1 Plan:**

```
IndexRangeScan [orders.cust_date_idx]
  Filter: customer_id = 123 AND order_date >= '2024-01-01'
```

**Query 2 Plan:**

```
IndexRangeScan [orders.cust_date_idx]
  Filter: customer_id = 123
  (scans all dates for customer 123)
```

**Query 3 Plan:**

```
SeqScan [orders]  -- Index not used (or skip scan if distinct customers is small)
  Filter: order_date >= '2024-01-01'
```

**Reason:** Index organized by $(customer\_id, order\_date)$. Can't efficiently find all rows with specific $order\_date$ without scanning all customers.

### Skip Scan Optimization

```sql
-- Index: CREATE INDEX products_cat_price_idx ON products(category, price)
SELECT * FROM products WHERE price > 100;
```

**Without Skip Scan:**

```
SeqScan [products]
  Filter: price > 100
```

**With Skip Scan (Ra optimization):**

```
SkipScan [products.cat_price_idx]
  Skip: category (10 distinct values)
  Filter: price > 100
```

**Cost:**
- Without: Full table scan ($N$ rows)
- With: 10 index seeks + matching rows
- **Useful when:** $|\text{distinct}(\text{category})| \times h < N \times C_{\text{seq}}$

## Clustered vs Non-Clustered

### Clustered Index (Heap Organization)

Data physically sorted by index key.

**Advantages:**
- Range queries are sequential reads
- Index-organized table (no separate heap)

**Disadvantages:**
- Only one per table
- Inserts may require reorg

**Cost Model:**

$$
\text{Cost}_{\text{range, clustered}} = h + \frac{n_{\text{rows}}}{\text{rows\_per\_page}}
$$

### Non-Clustered Index (Secondary Index)

Separate structure pointing to heap.

**Advantages:**
- Multiple per table
- Fast inserts

**Disadvantages:**
- Random heap access for each match

**Cost Model:**

$$
\text{Cost}_{\text{range, nonclustered}} = h + n_{\text{leaf}} + n_{\text{rows}} \times C_{\text{heap}}
$$

**Clustering Factor:**

$$
CF = \frac{\text{heap pages accessed}}{\text{index entries scanned}}
$$

- $CF \to 0$: Perfect clustering (sequential)
- $CF \to 1$: Random (worst case)

Ra uses $CF$ in cost model:

$$
\text{Cost}_{\text{heap}} = n_{\text{rows}} \times (1 - CF + CF \times N_{\text{pages}}/n_{\text{rows}})
$$

## Performance Characteristics

| Operation | Cost | Notes |
|-----------|------|-------|
| Point lookup | $O(\log N)$ | 3-4 I/Os typical |
| Range scan (clustered) | $O(\log N + k)$ | $k$ = result size |
| Range scan (non-clustered) | $O(\log N + k \times \text{pages})$ | Random heap access |
| Insert | $O(\log N)$ | May cause page split |
| Delete | $O(\log N)$ | May cause rebalance |
| Sorted access | $O(\log N + k)$ | No sort needed |

## Index Maintenance

### Page Splits

When inserting into full node:

$$
\text{Cost}_{\text{split}} = 2 \times C_{\text{io\_write}} + \text{Cost}_{\text{parent\_update}}
$$

**Fill Factor:** Ra recommends 70-90% to reduce splits.

### Fragmentation

Over time, logical order ≠ physical order.

**Metric:**

$$
\text{Frag} = 1 - \frac{\text{sequential page pairs}}{\text{total page pairs}}
$$

**Solution:** Periodic REINDEX.

## See Also

- [Hash Indexes](hash.md) - Equality-only indexes
- [Bitmap Indexes](bitmap.md) - Low-cardinality optimization
- [Covering Indexes](covering.md) - Index-only scans
- [Point Lookup](../query-patterns/oltp/point-lookup.md) - Usage pattern
- [Range Scan](../query-patterns/oltp/range-scan.md) - Range queries
- [Rule: Index Selection](../../rules/physical/index-scan-selection.md)

## References

- Bayer & McCreight, "Organization and Maintenance of Large Ordered Indexes", *Acta Informatica 1972*
- Comer, "The Ubiquitous B-Tree", *ACM Computing Surveys 1979*
- Graefe, "Modern B-Tree Techniques", *Foundations and Trends in Databases 2011*
