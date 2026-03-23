# Rule: "Distributed Query Routing and Data Movement Cost"

**Category:** cost-models
**File:** `rules/cost-models/distributed-query-routing-cost.rra`

## Metadata

- **ID:** `distributed-query-routing-cost`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, tidb, yugabytedb, spanner, trino, presto
- **Tags:** cost, distributed, routing, shuffle, broadcast, colocation, partition
- **Authors:** "Ozsu & Valduriez 2020 - Distributed Databases", "Corbett et al. 2013 - Spanner"


# Distributed Query Routing and Data Movement Cost

## Description

Models the full cost of data movement strategies in distributed query
plans: broadcast, hash shuffle, range shuffle, and co-located access.
Extends the basic network cost model with decision logic for choosing
between movement strategies based on data sizes, partition alignment,
and cluster topology.

**When to apply**: Every join and aggregation in a distributed query
requires a data movement decision. The wrong choice (e.g., shuffling
a large table when a small table could be broadcast) can increase
query time by 10-100x.

**Why it works**: Broadcasting a 10 MB dimension table to 100 nodes
costs 1 GB total transfer. Shuffling a 100 GB fact table costs 100 GB
transfer. If the dimension table is the build side, broadcast avoids
99 GB of unnecessary data movement.

## Relational Algebra

```algebra
-- Data movement strategies:
broadcast_cost(R, nodes) =
  BYTES(R) * (nodes - 1) / NETWORK_BW + nodes * LATENCY

shuffle_cost(R, nodes) =
  BYTES(R) * (1 - 1/nodes) / NETWORK_BW + nodes * LATENCY
  -- Each tuple goes to exactly one node; 1/nodes stays local

colocated_cost(R) = 0  -- data already on correct node

-- Strategy selection for join R $\bowtie$ S:
best_strategy(R, S, nodes) =
  if partition_key(R) == join_key AND partition_key(S) == join_key:
    colocated_join                    -- no movement
  elif BYTES(R) < BROADCAST_THRESHOLD:
    broadcast(R) + local_join(S)      -- broadcast smaller
  elif BYTES(S) < BROADCAST_THRESHOLD:
    broadcast(S) + local_join(R)
  else:
    shuffle(R) + shuffle(S) + local_join -- both shuffle

BROADCAST_THRESHOLD = NETWORK_BW * MAX_ACCEPTABLE_DELAY / nodes
```

## Implementation

```rust
use egg::{rewrite as rw, *};

struct DistributedCostModel {
    num_nodes: usize,
    network_bw_gbps: f64,
    network_latency_ms: f64,
    local_scan_rate_gbps: f64,
    serialization_overhead: f64, // multiplier >1.0
}

impl DistributedCostModel {
    fn broadcast_cost(&self, data_bytes: f64) -> f64 {
        let transfer = data_bytes
            * self.serialization_overhead
            * (self.num_nodes - 1) as f64;
        let time_s = transfer / (self.network_bw_gbps * 1e9);
        let latency_s =
            self.network_latency_ms / 1000.0
                * self.num_nodes as f64;
        time_s + latency_s
    }

    fn shuffle_cost(&self, data_bytes: f64) -> f64 {
        let fraction_moved =
            1.0 - 1.0 / self.num_nodes as f64;
        let transfer = data_bytes
            * self.serialization_overhead
            * fraction_moved;
        let time_s = transfer / (self.network_bw_gbps * 1e9);
        let latency_s = self.network_latency_ms / 1000.0;
        time_s + latency_s
    }

    fn join_movement_cost(
        &self,
        left: &DistRelStats,
        right: &DistRelStats,
        join_key: &[Column],
    ) -> (MovementStrategy, f64) {
        // Check co-location
        if left.partition_matches(join_key)
            && right.partition_matches(join_key)
        {
            return (MovementStrategy::Colocated, 0.0);
        }

        let left_bytes = left.total_bytes();
        let right_bytes = right.total_bytes();

        // Broadcast smaller side
        let broadcast_left = self.broadcast_cost(left_bytes)
            + right_bytes / (self.local_scan_rate_gbps * 1e9);
        let broadcast_right = self.broadcast_cost(right_bytes)
            + left_bytes / (self.local_scan_rate_gbps * 1e9);

        // Shuffle both sides
        let shuffle_both = self.shuffle_cost(left_bytes)
            + self.shuffle_cost(right_bytes);

        // One-sided shuffle: repartition only one side
        let shuffle_left = if right.partition_matches(join_key)
        {
            self.shuffle_cost(left_bytes)
        } else {
            f64::MAX
        };
        let shuffle_right = if left.partition_matches(join_key)
        {
            self.shuffle_cost(right_bytes)
        } else {
            f64::MAX
        };

        let options = [
            (MovementStrategy::BroadcastLeft, broadcast_left),
            (MovementStrategy::BroadcastRight, broadcast_right),
            (MovementStrategy::ShuffleBoth, shuffle_both),
            (MovementStrategy::ShuffleLeft, shuffle_left),
            (MovementStrategy::ShuffleRight, shuffle_right),
        ];

        options
            .iter()
            .min_by(|a, b| {
                a.1.partial_cmp(&b.1).unwrap_or(
                    std::cmp::Ordering::Equal,
                )
            })
            .cloned()
            .unwrap_or((MovementStrategy::ShuffleBoth, shuffle_both))
    }

    fn aggregation_strategy(
        &self,
        input: &DistRelStats,
        group_by: &[Column],
        num_groups: usize,
    ) -> AggStrategy {
        let group_data_bytes =
            num_groups as f64 * input.tuple_width();

        if input.partition_matches(group_by) {
            // Already partitioned on GROUP BY: local agg
            AggStrategy::LocalOnly
        } else if group_data_bytes < input.total_bytes() * 0.1 {
            // Few groups: partial agg then gather
            AggStrategy::PartialThenGather {
                network_bytes: group_data_bytes
                    * self.num_nodes as f64,
            }
        } else {
            // Many groups: shuffle then local agg
            AggStrategy::ShuffleThenAggregate {
                network_bytes: input.total_bytes()
                    * (1.0 - 1.0 / self.num_nodes as f64),
            }
        }
    }
}
```

## Preconditions

```rust
fn applicable(query: &Query, cluster: &ClusterConfig) -> bool {
    cluster.num_nodes() > 1
        && query.involves_multiple_tables()
}
```

**Restrictions:**
- Network bandwidth shared across concurrent queries
- Serialization format affects transfer size (Protobuf vs Arrow)
- Skewed partitions cause stragglers (one node gets majority)
- Cross-datacenter queries have 10-100x higher latency
- Cloud networking is often unpredictable (noisy neighbors)

## Cost Model

```rust
fn broadcast_vs_shuffle_threshold(
    num_nodes: usize,
    network_bw: f64,
) -> f64 {
    // Broadcast cost: R * (N-1)
    // Shuffle cost for other side: S * (1 - 1/N)
    // Broadcast wins when: R * (N-1) < S * (1 - 1/N)
    // Simplifies to: R < S / N (approximately)
    // Threshold: broadcast side should be < 1/N of other side
    //
    // Practical: broadcast up to ~100 MB in most clusters
    100.0 * 1024.0 * 1024.0 // 100 MB default threshold
}
```

**Decision rules of thumb:**
- Broadcast: smaller side < 100 MB (or 1/N of larger side)
- Co-located: both partitioned on join key (0 network cost)
- One-sided shuffle: one input already partitioned correctly
- Full shuffle: both large, neither co-located

## Test Cases

### Positive: Broadcast small dimension table

```sql
-- 100-node cluster, 10 Gbps network
-- products: 100K rows, 10 MB
-- sales: 10B rows, 1 TB, partitioned by sale_id
SELECT p.name, SUM(s.amount)
FROM sales s JOIN products p ON s.product_id = p.id
GROUP BY p.name;

-- Broadcast products: 10 MB * 99 = 990 MB, ~0.8s
-- Shuffle sales: 1 TB * 0.99 = 990 GB, ~800s
-- Broadcast is 1000x cheaper
```

### Positive: Co-located join

```sql
-- Both tables partitioned by customer_id
-- orders: 1B rows, customer_id partition
-- payments: 500M rows, customer_id partition
SELECT * FROM orders o
JOIN payments p ON o.customer_id = p.customer_id;

-- Co-located: 0 network transfer
-- Each node joins its local partition independently
```

### Negative: Both sides large, no co-location

```sql
-- orders: 1B rows, partitioned by order_id
-- lineitem: 6B rows, partitioned by lineitem_id
-- Join on order_id: neither co-located
SELECT * FROM orders o JOIN lineitem l ON o.orderkey = l.orderkey;

-- Must shuffle: both sides repartitioned on orderkey
-- Total network: ~7 TB * 0.99 = ~7 TB transfer
-- This is the worst case for distributed joins
```

## References

**Distributed query optimization:**
- Ozsu & Valduriez, "Principles of Distributed Database Systems", 4th ed., 2020
  - Chapter 8: Distributed query processing
- Corbett et al., "Spanner: Google's Globally-Distributed Database", OSDI 2012
  - Co-located joins via directory-based sharding

**Data movement optimization:**
- Zhu et al., "Looking Ahead Makes Query Plans Better", VLDB 2017
  - Lookahead information passing for distributed optimization
- Vitorovic et al., "Squall: Fine-Grained Live Reconfiguration for Partitioned Main Memory Databases", SIGMOD 2016
  - Dynamic repartitioning for workload changes
