# GROUP BY Aggregation

**Category:** OLAP Query Patterns
**Impact:** High - Core analytical query pattern
**Complexity:** Medium

## Overview

GROUP BY aggregation queries summarize data by grouping rows with common values and computing aggregate functions (COUNT, SUM, AVG, MIN, MAX) over each group. This is the foundation of OLAP and analytical workloads.

## SQL Pattern

```sql
SELECT
    product_category,
    region,
    COUNT(*) as sale_count,
    SUM(amount) as total_revenue,
    AVG(amount) as avg_sale,
    MIN(amount) as min_sale,
    MAX(amount) as max_sale
FROM sales
GROUP BY product_category, region;
```

## Relational Algebra

$$
\gamma_{G, F}(R)
$$

Where:
- $G$ = grouping attributes (`product_category`, `region`)
- $F$ = aggregate functions (`COUNT(*)`, `SUM(amount)`, etc.)
- $R$ = input relation (`sales`)

Formally:
$$
\gamma_{\text{product\_category}, \text{region}, \text{COUNT}(*), \text{SUM(amount)}, \text{AVG(amount)}}(\text{sales})
$$

## Execution Strategies

### Hash Aggregation (Default)

Build hash table keyed by group columns:

$$
\text{Cost}_{\text{hash}} = |R| \times (C_{\text{cpu}} + C_{\text{hash}}) + |\text{groups}| \times C_{\text{output}}
$$

**Pros:**
- Fast for low to moderate cardinality
- Single pass over data
- Parallelizable

**Cons:**
- Requires memory for hash table
- Poor performance if groups don't fit in memory

**Best for:** $|\text{groups}| < 10^6$ and fits in memory

### Sort-Based Aggregation

Sort by group keys, then aggregate sorted runs:

$$
\text{Cost}_{\text{sort}} = |R| \times \log |R| \times C_{\text{cpu}} + |R| \times C_{\text{output}}
$$

**Pros:**
- Works for unlimited cardinality
- Produces sorted output
- Predictable memory usage

**Cons:**
- Slower than hash aggregation
- Requires full sort

**Best for:** High cardinality or memory-constrained environments

### Index-Based Aggregation

Use index on group columns to avoid full scan:

$$
\text{Cost}_{\text{index}} = |\text{groups}| \times C_{\text{index}} + |R_{\text{needed}}| \times C_{\text{cpu}}
$$

**Pros:**
- Very fast for selective queries
- Avoids reading unnecessary rows

**Cons:**
- Requires appropriate index
- Only beneficial with high selectivity

**Best for:** $|\text{groups}| \ll |R|$ with index on group columns

## Ra Optimization Rules

1. **[hash-aggregate](../../rules/logical/aggregation/hash-aggregate.rra)** - Use hash-based aggregation
2. **[sort-aggregate](../../rules/logical/aggregation/sort-aggregate.rra)** - Use sort-based aggregation
3. **[push-aggregate-down](../../rules/logical/aggregation/push-aggregate-down.rra)** - Push aggregation below joins
4. **[two-phase-aggregate](../../rules/distributed/two-phase-aggregate.rra)** - Distributed aggregation
5. **[eliminate-redundant-group-by](../../rules/logical/aggregation/eliminate-redundant-group-by.rra)** - Remove unnecessary grouping

## Providing Statistics to Ra

```rust
use ra_core::{ColumnStatistics, TableStatistics};

optimizer.set_statistics("sales", TableStatistics {
    row_count: 100_000_000,
    distinct_values: hashmap! {
        "product_category" => 50,     // Low cardinality
        "region" => 10,                // Low cardinality
        "amount" => 50_000,            // High cardinality
    },
});

// Ra estimates group count: 50 $\times$ 10 = 500 groups
// Chooses hash aggregation (fits easily in memory)
```

## Examples

### Simple Aggregation

```sql
SELECT region, COUNT(*) as customer_count
FROM customers
GROUP BY region;
```

**Optimization:**
- Hash aggregation (10 groups)
- Index scan on `region` if index exists
- Single pass over table

**Cost:** $O(n)$ where $n = |\text{customers}|$

### Multi-Column Grouping

```sql
SELECT
    product_category,
    product_subcategory,
    brand,
    COUNT(*) as product_count,
    AVG(price) as avg_price
FROM products
GROUP BY product_category, product_subcategory, brand;
```

**Cardinality estimation:**
$$
|\text{groups}| \approx \frac{|\text{distinct}(c_1)| \times |\text{distinct}(c_2)| \times |\text{distinct}(c_3)|}{\text{correlation\_factor}}
$$

With 20 categories $\times$ 100 subcategories $\times$ 500 brands = 1M potential groups.
With correlation factor (not all combinations exist) -> ~50K actual groups.

### Aggregation with Complex Expressions

```sql
SELECT
    EXTRACT(YEAR FROM order_date) as year,
    EXTRACT(QUARTER FROM order_date) as quarter,
    SUM(quantity * unit_price) as revenue,
    COUNT(DISTINCT customer_id) as unique_customers
FROM orders
GROUP BY EXTRACT(YEAR FROM order_date), EXTRACT(QUARTER FROM order_date);
```

**Ra optimizations:**
- Compute expressions once (common subexpression elimination)
- Use HyperLogLog for approximate COUNT(DISTINCT)
- Hash aggregation on computed columns

### Nested Aggregations

```sql
-- Top 10 products by revenue
SELECT product_id, SUM(amount) as total_revenue
FROM sales
GROUP BY product_id
ORDER BY total_revenue DESC
LIMIT 10;
```

**Optimization:**
- Hash aggregation to compute sums
- Top-K algorithm (heap) instead of full sort
- Avoids sorting all groups

**Cost:** $O(n + k \log k)$ instead of $O(n \log n)$ where $k = 10$

## Aggregate Function Characteristics

### Algebraic (Decomposable)

Can be computed incrementally:

| Function | Incremental Formula | Memory |
|----------|---------------------|--------|
| `COUNT(*)` | $\text{count} + 1$ | O(1) |
| `SUM(x)` | $\text{sum} + x$ | O(1) |
| `MIN(x)` | $\min(\text{current}, x)$ | O(1) |
| `MAX(x)` | $\max(\text{current}, x)$ | O(1) |
| `AVG(x)` | $\frac{\text{sum}}{ {\text{count}} }$ | O(1) - store sum+count |

### Holistic (Non-Decomposable)

Require seeing all values:

| Function | Approach | Memory |
|----------|----------|--------|
| `MEDIAN(x)` | Sort or quantile sketch | O(n) or O(log n) |
| `PERCENTILE(x, p)` | Quantile sketch | O(log n) |
| `MODE(x)` | Frequency map | O(d) where d = distinct values |
| `LISTAGG(x)` | Accumulate strings | O(n) |

## Push-down Aggregation

Ra pushes aggregation below joins when possible:

### Before

$$
\gamma_{G, F}(R \bowtie S)
$$

Must join all rows, then aggregate.

### After (with push-down)

$$
\gamma_{G, F}(R) \bowtie S
$$

Aggregate $R$ first, then join with $S$ (fewer rows).

**Requirement:** Grouping columns $G \subseteq \text{attrs}(R)$

**Example:**
```sql
-- Before: Join 100M orders $\times$ 10K customers, then aggregate
SELECT c.customer_name, COUNT(*) as order_count
FROM orders o
JOIN customers c ON o.customer_id = c.customer_id
GROUP BY c.customer_name;

-- After: Aggregate orders (100M -> 10K), then join
SELECT c.customer_name, o.order_count
FROM (
    SELECT customer_id, COUNT(*) as order_count
    FROM orders
    GROUP BY customer_id
) o
JOIN customers c ON o.customer_id = c.customer_id;
```

**Speedup:** Join 10K rows instead of 100M = **10,000x reduction**

## Distributed Aggregation

For distributed systems, use two-phase aggregation:

### Local Phase

Each node aggregates its partition:
$$
\text{partial\_results}_i = \gamma_{G, F}(R_i)
$$

### Global Phase

Coordinator merges partial results:
$$
\text{final\_result} = \gamma_{G, \text{merge}(F)}(\bigcup_{i} \text{partial\_results}_i)
$$

**Example:**
```
Node 1: {region: 'US', count: 30M, sum: 500M}
Node 2: {region: 'US', count: 25M, sum: 420M}
Final:  {region: 'US', count: 55M, sum: 920M, avg: 16.7}
```

See [Push-down Aggregation](../../distributed-patterns/pushdown-aggregation.md) for details.

## Performance Tuning

### Small Cardinality (< 1000 groups)

```sql
-- 10 regions, 50 categories = 500 groups
SELECT region, category, COUNT(*)
FROM sales
GROUP BY region, category;
```

**Optimal:** Hash aggregation, fits in L3 cache.

**Cost:** ~1 second for 100M rows.

### Medium Cardinality (1K - 1M groups)

```sql
-- 50K products, aggregating sales
SELECT product_id, SUM(amount)
FROM sales
GROUP BY product_id;
```

**Optimal:** Hash aggregation with larger hash table.

**Cost:** ~5-10 seconds for 100M rows.

### High Cardinality (> 1M groups)

```sql
-- 10M unique customers
SELECT customer_id, SUM(amount)
FROM sales
GROUP BY customer_id;
```

**Optimal:** Sort-based aggregation or spill to disk.

**Cost:** ~30-60 seconds for 100M rows.

**Alternative:** Use approximate aggregation if exact not needed.

## Common Pitfalls

### [FAIL] High Cardinality GROUP BY

```sql
-- user_agent has millions of distinct values
SELECT user_agent, COUNT(*)
FROM access_logs
GROUP BY user_agent;
```

**Problem:** Hash table doesn't fit in memory, causes spilling/swapping.

**Fix:** Use approximate aggregation, sampling, or pre-aggregate.

### [FAIL] Unnecessary GROUP BY

```sql
-- All rows have same customer_id (filtered)
SELECT customer_id, COUNT(*)
FROM orders
WHERE customer_id = 12345
GROUP BY customer_id;
```

**Optimization:** Ra eliminates GROUP BY if grouping column is constant:
```sql
SELECT 12345 as customer_id, COUNT(*) FROM orders WHERE customer_id = 12345;
```

### [FAIL] Expensive Aggregate Functions

```sql
-- STRING_AGG concatenates all values (memory intensive)
SELECT category, STRING_AGG(product_name, ', ')
FROM products
GROUP BY category;
```

**Problem:** Accumulates large strings in memory.

**Fix:** Limit result size or use specialized aggregation.

## Testing Aggregation Queries

```rust
#[test]
fn test_hash_aggregation_selection() {
    let sql = "SELECT region, COUNT(*) FROM sales GROUP BY region";

    let plan = optimize(sql)
        .with_statistics("sales", TableStatistics {
            row_count: 100_000_000,
            distinct_values: hashmap! { "region" => 10 },
        })
        .build();

    // Verify hash aggregation chosen
    assert!(plan.contains_node_type("HashAggregate"));
    assert!(!plan.contains_node_type("SortAggregate"));

    // Verify single-pass execution
    assert_eq!(plan.passes_over_data(), 1);
}

#[test]
fn test_push_aggregate_below_join() {
    let sql = "
        SELECT c.name, COUNT(*)
        FROM orders o
        JOIN customers c ON o.customer_id = c.customer_id
        GROUP BY c.name
    ";

    let plan = optimize(sql).build();

    // Verify aggregation happens before join
    let join_node = plan.find_node("Join");
    let agg_node = plan.find_node("Aggregate");
    assert!(agg_node.is_ancestor_of(join_node));
}
```

## Performance Characteristics

| Rows | Groups | Strategy | Time | Memory |
|------|--------|----------|------|--------|
| 1M | 10 | Hash | 0.1s | 1KB |
| 10M | 100 | Hash | 0.5s | 10KB |
| 100M | 1K | Hash | 3s | 100KB |
| 100M | 100K | Hash | 10s | 10MB |
| 100M | 10M | Sort | 60s | 100MB (disk) |

## References

- [Hash Aggregate Rule](../../rules/logical/aggregation/hash-aggregate.rra)
- [Two-Phase Aggregation](../../distributed-patterns/pushdown-aggregation.md)
- [Window Functions](../analytical/window-functions.md) - Related to running aggregates
- [Approximate Aggregation](../../features/approximate-aggregation.md)

## Related Patterns

- [Window Functions](../analytical/window-functions.md) - Aggregation over windows
- [ROLLUP/CUBE](rollup-cube.md) - Hierarchical aggregation
- [Push-down Aggregation](../../distributed-patterns/pushdown-aggregation.md) - Distributed case
