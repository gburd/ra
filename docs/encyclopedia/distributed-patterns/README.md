# Distributed Query Patterns

Multi-node query execution strategies for distributed databases.

## Patterns

### [Shuffle Joins](shuffle-joins.md)
Repartition both tables by join key. Required for large-to-large joins.

### [Broadcast Joins](broadcast-joins.md)
Replicate small table to all nodes. Optimal for large-to-small joins.

### [Co-located Joins](co-located-joins.md)
Join locally when data already partitioned on join key.

### [Partition Pruning](partition-pruning.md)
Eliminate partitions not matching query predicates.

### [Pushdown Aggregation](pushdown-aggregation.md)
Pre-aggregate locally before network shuffle.

### [Union Over Partitions](union-over-partitions.md)
Parallel scan across partitions with result union.

## Strategy Selection

| Left Size | Right Size | Partitioning | Strategy | Network Cost |
|-----------|-----------|-------------|----------|--------------|
| Large | Large | None | Shuffle both | $O(L + R)$ |
| Large | Small | Any | Broadcast right | $O(R \times P)$ |
| Large | Large | Aligned | Co-located | $O(0)$ |
| Large | Large | Skewed | Salted shuffle | $O((L + R) \times s)$ |

## Cost Formulas

**Broadcast:**
$$
\text{Cost} = |S| \times P \times C_{\text{network}} + |R| \times C_{\text{scan}}
$$

**Shuffle:**
$$
\text{Cost} = (|R| + |S|) \times C_{\text{network}} + \frac{|R| + |S|}{P} \times C_{\text{hash}}
$$

**Co-located:**
$$
\text{Cost} = \frac{|R| + |S|}{P} \times C_{\text{local join}}
$$
