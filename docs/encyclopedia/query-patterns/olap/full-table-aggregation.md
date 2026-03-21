# Full Table Aggregation

## Description

Aggregation over entire table or large partitions using GROUP BY. Core OLAP pattern for summary statistics, reports, and dashboards.

## Use Cases

- Sales by region/product/time period
- Daily active users by country
- Revenue summaries
- Inventory levels by warehouse
- Customer segmentation

## Relational Algebra

Basic aggregation:

$$
\gamma_{G; \text{AGG}_1(A_1), \ldots, \text{AGG}_n(A_n)}(R)
$$

Where:
- $G = \{g_1, \ldots, g_k\}$ are grouping columns
- $\text{AGG}_i$ are aggregate functions (SUM, COUNT, AVG, MIN, MAX)

With selection:

$$
\gamma_{G; F}(\sigma_{\theta}(R))
$$

## How Ra Optimizes

### 1. Hash Aggregation vs Sort Aggregation

**Rule:** `physical/aggregation-method-selection`

**Hash Aggregation:**
$$
\text{Cost}_{\text{hash}} = |R| \times (C_{\text{cpu}} + C_{\text{hash}}) + |G| \times C_{\text{output}}
$$

Used when:
- Output cardinality $|G|$ fits in memory
- No subsequent ORDER BY needed

**Sort Aggregation:**
$$
\text{Cost}_{\text{sort}} = |R| \times \log(|R|) \times C_{\text{sort}} + |R| \times C_{\text{scan}}
$$

Used when:
- Output is large (doesn't fit in memory)
- Query has ORDER BY on grouping columns
- Streaming aggregation needed

### 2. Partial Aggregation Pushdown

**Rule:** `logical/pushdown/aggregate-through-union`

For partitioned tables:

$$
\gamma_{G; F}(R_1 \cup R_2 \cup \cdots \cup R_n) \equiv \gamma_{G; F}(\gamma_{G; F}(R_1) \cup \cdots \cup \gamma_{G; F}(R_n))
$$

Ra performs local aggregation per partition, then final aggregation.

### 3. Predicate Pushdown

**Rule:** `logical/pushdown/filter-through-aggregate`

Push non-aggregate predicates below aggregation:

$$
\sigma_{\theta}(\gamma_{G; F}(R)) \rightarrow \gamma_{G; F}(\sigma_{\theta}(R)) \quad \text{if } \theta \text{ only references } G
$$

### 4. Projection Pushdown

**Rule:** `logical/pushdown/project-through-aggregate`

Eliminate unused columns before aggregation:

$$
\pi_{G \cup A}(\gamma_{G; F}(R)) \equiv \gamma_{G; F}(\pi_{G \cup A}(R))
$$

Where $A$ are columns needed for aggregates.

### 5. Index-Based MIN/MAX

**Rule:** `physical/min-max-index-scan`

For queries like:

$$
\gamma_{\emptyset; \text{MIN}(A), \text{MAX}(A)}(R)
$$

If index exists on $A$:
- $\text{MIN}(A)$: Read first index entry
- $\text{MAX}(A)$: Read last index entry

Cost: $O(1)$ instead of $O(|R|)$.

## Statistics API

```rust
use ra_optimizer::{Statistics, ColumnStatistics};

// Table stats
optimizer.add_table_stats("sales", Statistics {
    row_count: 100_000_000,
    block_count: 1_000_000,
    average_row_width: 100,
});

// Grouping column stats
optimizer.add_column_stats("sales", "region", ColumnStatistics {
    distinct_count: 50,  // Number of distinct groups
    null_fraction: 0.0,
    most_common_values: vec![
        ("North America", 0.35),
        ("Europe", 0.30),
        ("Asia", 0.25),
        ("South America", 0.10),
    ],
});

optimizer.add_column_stats("sales", "product_id", ColumnStatistics {
    distinct_count: 10_000,
    null_fraction: 0.01,
});

// Aggregate column
optimizer.add_column_stats("sales", "amount", ColumnStatistics {
    distinct_count: 500_000,
    null_fraction: 0.0,
    min_value: Some(0.01),
    max_value: Some(999999.99),
});
```

### Cardinality Estimation

Output cardinality for GROUP BY:

$$
|\gamma_{G}(R)| = \prod_{g \in G} \text{distinct}(g)
$$

With correlation adjustment if columns are dependent.

## Examples

### Single-Column Grouping

```sql
SELECT region, COUNT(*) as order_count, SUM(amount) as total_sales
FROM sales
GROUP BY region;
```

**Relational Algebra:**

$$
\gamma_{\text{region}; \text{COUNT}(*), \text{SUM}(\text{amount})}(\text{sales})
$$

**Ra Plan:**

```
HashAggregate [region]
  Aggregates: COUNT(*), SUM(amount)
  SeqScan [sales]
```

**Cost:** $O(|R|)$ with hash table of size $|\text{regions}| = 50$.

### Multi-Column Grouping

```sql
SELECT region, product_category, DATE_TRUNC('month', sale_date) as month,
       COUNT(*) as sales_count, AVG(amount) as avg_amount
FROM sales
WHERE sale_date >= '2024-01-01'
GROUP BY region, product_category, DATE_TRUNC('month', sale_date);
```

**Relational Algebra:**

$$
\gamma_{G; \text{COUNT}(*), \text{AVG}(\text{amount})}(\sigma_{\text{sale\_date} \geq \text{'2024-01-01'}}(\text{sales}))
$$

Where $G = \{\text{region}, \text{product\_category}, \text{month}\}$.

**Ra Plan:**

```
HashAggregate [region, product_category, month]
  Aggregates: COUNT(*), AVG(amount)
  Project [region, product_category, DATE_TRUNC('month', sale_date) AS month, amount]
    SeqScan [sales]
      Filter: sale_date >= '2024-01-01'
```

**Output Cardinality:** $50 \times 20 \times 12 = 12{,}000$ groups (approx).

### Sort-Based Aggregation

```sql
SELECT customer_id, SUM(amount) as total_spent
FROM orders
GROUP BY customer_id
ORDER BY total_spent DESC
LIMIT 10;
```

**Ra Plan:**

```
Limit (10)
  Sort [total_spent DESC]
    HashAggregate [customer_id]
      Aggregates: SUM(amount) AS total_spent
      SeqScan [orders]
```

Alternative with sort aggregation:

```
Limit (10)
  SortAggregate [customer_id]
    Aggregates: SUM(amount) AS total_spent
    Sort [customer_id]  -- Pre-sort for streaming aggregation
      SeqScan [orders]
```

Ra chooses based on:
- If $|\text{customers}|$ fits in memory: Hash aggregation + sort
- Otherwise: Sort aggregation (streaming)

### Partitioned Table Optimization

```sql
-- Table partitioned by year: sales_2022, sales_2023, sales_2024
SELECT product_id, SUM(amount) as total_sales
FROM sales
WHERE sale_date >= '2024-01-01'
GROUP BY product_id;
```

**Ra Plan:**

```
FinalHashAggregate [product_id]
  Aggregates: SUM(partial_sum) AS total_sales
  Append
    PartialHashAggregate [product_id]
      Aggregates: SUM(amount) AS partial_sum
      SeqScan [sales_2024]
        Filter: sale_date >= '2024-01-01'
```

**Optimization:** Partition pruning eliminates sales_2022, sales_2023.

### MIN/MAX Index Optimization

```sql
SELECT MIN(created_at), MAX(created_at) FROM orders;
```

**Ra Plan:**

```
Result
  InitPlan 1:
    Limit (1)
      IndexScan [orders.created_at_idx] (Forward)
  InitPlan 2:
    Limit (1)
      IndexScan [orders.created_at_idx] (Backward)
```

**Cost:** $O(1)$ instead of scanning all rows.

### Covering Index for Aggregation

```sql
-- Index: CREATE INDEX sales_region_amount_idx ON sales(region, amount)
SELECT region, SUM(amount), COUNT(*)
FROM sales
GROUP BY region;
```

**Ra Plan:**

```
HashAggregate [region]
  Aggregates: SUM(amount), COUNT(*)
  IndexOnlyScan [sales.region_amount_idx]
```

**Cost Reduction:** No heap access needed, ~10x faster.

## Advanced Optimizations

### Distinct Aggregation

**Rule:** `logical/distinct-aggregate-decomposition`

```sql
SELECT region, COUNT(DISTINCT customer_id), SUM(amount)
FROM sales
GROUP BY region;
```

Decomposed to:

$$
\gamma_{\text{region}; \text{COUNT}(\text{customer\_id}), \text{SUM}(\text{sum\_amount})}(
  \gamma_{\text{region}, \text{customer\_id}; \text{SUM}(\text{amount})}(\text{sales})
)
$$

Two-phase aggregation to handle DISTINCT efficiently.

### Filter Aggregates (SQL:2003)

```sql
SELECT region,
       COUNT(*) FILTER (WHERE amount > 1000) as high_value_count,
       SUM(amount) as total_sales
FROM sales
GROUP BY region;
```

**Ra Plan:** Single pass with conditional aggregation.

## Performance Characteristics

| Scenario | Method | Time Complexity | Memory |
|----------|--------|----------------|--------|
| Low cardinality (< 10K groups) | Hash aggregation | $O(n)$ | $O(g)$ |
| High cardinality (> 10K groups) | Sort aggregation | $O(n \log n)$ | $O(1)$ streaming |
| Partitioned table | Parallel partial agg | $O(n/p)$ | $O(g)$ per partition |
| MIN/MAX only | Index scan | $O(1)$ | $O(1)$ |

## See Also

- [Multi-level Grouping](multi-level-grouping.md) - ROLLUP, CUBE
- [Distinct Aggregation](distinct-aggregation.md) - COUNT(DISTINCT) optimization
- [Top-N](top-n.md) - GROUP BY with LIMIT
- [Distributed Patterns: Pushdown Aggregation](../../distributed-patterns/pushdown-aggregation.md)
- [Index Structures: Covering](../../index-structures/covering.md)
- [Rule: Aggregate Pushdown](../../../rules/logical/pushdown/aggregate-pushdown.md)

## References

- Graefe, "Query Evaluation Techniques for Large Databases", *ACM Computing Surveys*, 1993
- Larson et al., "Hashing and Sorting for SQL Server 2005", *SIGMOD 2005*
