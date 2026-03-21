# Shuffle Joins

## Description

Distributed join strategy where both input relations are repartitioned (shuffled) across nodes based on the join key. Required when neither relation is partitioned appropriately for a co-located join.

## Use Cases

- Large-to-large table joins
- No existing partitioning on join keys
- M:N relationships with high cardinality
- Ad-hoc analytical queries

## Relational Algebra

Distributed shuffle join:

$$
R \bowtie_{\theta} S = \bigcup_{i=1}^{P} (R_i \bowtie_{\theta} S_i)
$$

Where $R_i$ and $S_i$ are partitions created by hash partitioning on join key:

$$
R_i = \{r \in R \mid h(\text{join\_key}(r)) \mod P = i\}
$$

$$
S_i = \{s \in S \mid h(\text{join\_key}(s)) \mod P = i\}
$$

## How Ra Optimizes

### 1. Partition Count Selection

**Rule:** `distributed/shuffle-partition-count`

Optimal partition count:

$$
P = \min\left(\text{nodes}, \left\lceil \frac{|R| + |S|}{\text{partition\_size\_target}} \right\rceil\right)
$$

Typical target: 128 MB - 256 MB per partition.

### 2. Shuffle Strategy Selection

**Rule:** `distributed/shuffle-strategy-selection`

Choose shuffle strategy based on table sizes:

| Left Size | Right Size | Strategy |
|-----------|-----------|----------|
| Large | Large | **Symmetric Shuffle** - Both tables shuffled |
| Large | Small | **Broadcast Join** - Right broadcasted |
| Large | Medium | **Asymmetric Shuffle** - Right shuffled, left remains |

**Cost Comparison:**

$$
\text{Cost}_{\text{shuffle}} = C_{\text{network}} \times (|R| + |S|) + C_{\text{hash}}(R, S)
$$

$$
\text{Cost}_{\text{broadcast}} = C_{\text{network}} \times |S| \times P + C_{\text{nested\_loop}}(R, S)
$$

### 3. Skew Handling

**Rule:** `distributed/shuffle-skew-mitigation`

For skewed keys, Ra uses **salted shuffle**:

$$
h'(\text{key}) = h(\text{key} \oplus \text{salt}) \mod P
$$

Where salt is random value added to split hot keys across partitions.

### 4. Predicate Pushdown Before Shuffle

**Rule:** `logical/pushdown/filter-before-shuffle`

Push filters before network transfer:

$$
\sigma_{\theta}(R) \bowtie S \equiv \sigma_{\theta}(R) \bowtie S
$$

Reduces shuffle volume.

## Cost Model

### Network Cost

$$
\text{Cost}_{\text{network}} = \frac{|R| + |S|}{B} \times C_{\text{byte}} \times (1 + \epsilon)
$$

Where:
- $B$ = network bandwidth (bytes/sec)
- $\epsilon$ = network overhead (0.1-0.3 typical)

### Shuffle Hash Join Cost

$$
\text{Cost}_{\text{total}} = \text{Cost}_{\text{shuffle}} + \text{Cost}_{\text{local\_join}}
$$

$$
= \left(\frac{|R| + |S|}{B} \times C_{\text{network}}\right) + \left(\frac{|R| + |S|}{P} \times C_{\text{hash}}\right)
$$

### Comparison with Broadcast

**Broadcast:** $O(|S| \times P)$ network, $O(|R| \times |S|/P)$ computation

**Shuffle:** $O(|R| + |S|)$ network, $O((|R| + |S|)/P)$ computation

**Threshold:** Broadcast when $|S| < \frac{|R|}{P}$.

## Statistics API

```rust
use ra_optimizer::{DistributedStatistics, NetworkProfile};

// Cluster configuration
optimizer.set_cluster_config(ClusterConfig {
    node_count: 10,
    network_bandwidth_mbps: 10_000,  // 10 Gbps
    shuffle_partition_size_mb: 128,
});

// Table stats
optimizer.add_table_stats("orders", Statistics {
    row_count: 100_000_000,
    size_bytes: 10_000_000_000,  // 10 GB
});

optimizer.add_table_stats("customers", Statistics {
    row_count: 10_000_000,
    size_bytes: 2_000_000_000,  // 2 GB
});

// Join column stats
optimizer.add_column_stats("orders", "customer_id", ColumnStatistics {
    distinct_count: 10_000_000,
    null_fraction: 0.0,
    skew_factor: 1.5,  // Moderate skew
    top_keys: vec![
        (123, 0.01),  // Customer 123 has 1% of orders (hot key)
    ],
});

// Network profile (optional, for accurate cost modeling)
optimizer.set_network_profile(NetworkProfile {
    latency_ms: 1.0,
    bandwidth_mbps: 10_000,
    overhead_factor: 0.15,
});
```

## Examples

### Symmetric Shuffle Join

```sql
-- Large fact tables: 100M rows each
SELECT o.order_id, r.return_id, o.amount, r.refund_amount
FROM orders o
JOIN returns r ON o.order_id = r.order_id
WHERE o.order_date >= '2024-01-01'
  AND r.return_date >= '2024-01-01';
```

**Ra Distributed Plan:**

```
HashJoin [o.order_id = r.order_id]
  Exchange [Hash(o.order_id), 100 partitions]
    SeqScan [orders o]
      Filter: order_date >= '2024-01-01'
      (40M rows after filter)
  Exchange [Hash(r.order_id), 100 partitions]
    SeqScan [returns r]
      Filter: return_date >= '2024-01-01'
      (5M rows after filter)
```

**Network Transfer:**
- Orders: 4 GB (40M rows × 100 bytes avg)
- Returns: 500 MB (5M rows × 100 bytes avg)
- Total: 4.5 GB shuffled

**Parallelism:** 100 partitions × 10 nodes = 10 partitions/node.

### Skewed Shuffle Join

```sql
-- High skew: top 10 customers have 50% of orders
SELECT c.name, COUNT(*) as order_count
FROM customers c
JOIN orders o ON o.customer_id = c.id
GROUP BY c.name;
```

**Naive Plan (causes stragglers):**

```
HashAggregate [name]
  HashJoin [o.customer_id = c.id]
    Exchange [Hash(c.id), 100 partitions]  -- Skewed partitions!
      SeqScan [customers c]
    Exchange [Hash(o.customer_id), 100 partitions]  -- Hot keys
      SeqScan [orders o]
```

**Problem:** Partitions with hot keys process 1000x more data than others.

**Ra Optimized Plan (salted shuffle):**

```
HashAggregate [name]
  Aggregates: SUM(partial_count)
  Exchange [Hash(name), 100 partitions]
    HashAggregate [name]
      Aggregates: COUNT(*) as partial_count
      HashJoin [o.customer_id = c.id AND o.salt = c.salt]
        Exchange [Hash(c.id, salt), 100 partitions]
          Generate [salt = random(0, 9)]  -- 10 replicas
            SeqScan [customers c]
        Exchange [Hash(o.customer_id, salt), 100 partitions]
          Generate [salt = random(0, 9)]  -- Split hot keys
            SeqScan [orders o]
```

**Skew Mitigation:**
- Hot customers replicated 10x
- Their orders split across 10 partitions
- Balanced workload distribution

### Shuffle Join with Aggregation Pushdown

```sql
SELECT c.country, SUM(o.amount) as total_sales
FROM customers c
JOIN orders o ON o.customer_id = c.id
GROUP BY c.country;
```

**Ra Plan (two-phase aggregation):**

```
FinalHashAggregate [country]
  Aggregates: SUM(partial_sum)
  Exchange [Hash(country), 20 partitions]
    PartialHashAggregate [country]
      Aggregates: SUM(amount) as partial_sum
      HashJoin [o.customer_id = c.id]
        Exchange [Hash(c.id), 100 partitions]
          SeqScan [customers c]
        Exchange [Hash(o.customer_id), 100 partitions]
          SeqScan [orders o]
```

**Optimization:** Partial aggregation reduces data before second shuffle.

**Network Traffic:**
- First shuffle: Orders (10 GB) + Customers (2 GB) = 12 GB
- Second shuffle: ~1 MB (100 countries × ~10 KB per country)

### Cascading Shuffles

```sql
SELECT p.product_name, SUM(oi.quantity) as total_sold
FROM order_items oi
JOIN orders o ON oi.order_id = o.id
JOIN products p ON oi.product_id = p.id
WHERE o.status = 'completed';
```

**Ra Plan:**

```
HashAggregate [product_name]
  Aggregates: SUM(quantity)
  HashJoin [oi.product_id = p.id]
    Exchange [Hash(oi.product_id), 100 partitions]
      HashJoin [oi.order_id = o.id]
        Exchange [Hash(oi.order_id), 100 partitions]
          SeqScan [order_items oi]
        Exchange [Hash(o.id), 100 partitions]
          SeqScan [orders o]
            Filter: status = 'completed'
    SeqScan [products p]  -- Small table, broadcast
```

**Optimization:** Products table broadcasted (not shuffled) because it's small.

## Advanced Techniques

### Range Shuffle

For range joins or sorted output:

$$
R_i = \{r \in R \mid \text{key}_{\min,i} \leq \text{key}(r) < \text{key}_{\max,i}\}
$$

Used for sort-merge joins:

```sql
SELECT * FROM large_table1 t1
JOIN large_table2 t2 ON t1.key = t2.key
ORDER BY t1.key;
```

**Ra Plan:**

```
MergeJoin [key]
  Exchange [Range(key), 100 partitions]  -- Range shuffle
    SeqScan [large_table1]
  Exchange [Range(key), 100 partitions]  -- Same ranges
    SeqScan [large_table2]
```

**Advantage:** Output already sorted, no final sort needed.

### Adaptive Shuffle

Ra monitors shuffle progress and adjusts:

```rust
// Runtime adjustment
if partition_size_variance > threshold {
    // Detected skew mid-execution
    repartition_with_salt();
}
```

## Performance Characteristics

| Scenario | Network Cost | Compute Cost | Stragglers |
|----------|-------------|-------------|-----------|
| Balanced data | $O(n/P)$ per node | $O(n/P)$ | None |
| Skewed data (naive) | $O(n/P)$ per node | $O(n)$ on hotspot | Severe |
| Skewed data (salted) | $O(n \times s/P)$ | $O(n \times s/P)$ | Minimal |

Where $s$ is salt factor (e.g., 10).

## See Also

- [Broadcast Joins](broadcast-joins.md) - Small table replication
- [Co-located Joins](co-located-joins.md) - Partition-aligned joins
- [Partition Pruning](partition-pruning.md) - Eliminating partitions
- [Pushdown Aggregation](pushdown-aggregation.md) - Pre-aggregation
- [Skew](../dataset-characteristics/skew.md) - Data imbalance
- [Rule: Shuffle Join Optimization](../../rules/distributed/shuffle-join-optimization.md)
- [Example: Distributed Join Strategies](../../examples/distributed-join-strategies.md)

## References

- DeWitt et al., "Gamma - A High Performance Dataflow Database Machine", *VLDB 1986*
- Graefe, "Encapsulation of Parallelism in the Volcano Query Processing System", *SIGMOD 1990*
- Xu et al., "Leen: Locality/Fairness-Aware Key Partitioning for MapReduce in the Cloud", *CloudCom 2010*
- Kwon et al., "SkewTune: Mitigating Skew in MapReduce Applications", *SIGMOD 2012*
