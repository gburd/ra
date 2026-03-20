# Phase 19: Distributed Query Optimization

Phase 19 extends the optimizer to handle distributed query execution with
network-aware cost modeling, data movement strategies, federated query
planning, and distributed aggregation.

## Overview

Phases 1-18 delivered production-ready single-node optimization. Phase 19
adds distributed execution strategies needed for:

- Cluster environments (Spark, Presto, Trino)
- Data warehouses (Snowflake, BigQuery, Redshift)
- Federated queries across multiple databases
- Cloud-native architectures with network boundaries

## Components

### 1. Network Cost Modeling

Models network topology and estimates data transfer costs between nodes,
datacenters, and regions. Includes 5 predefined profiles covering common
deployment patterns.

**Crates:** `ra-hardware` (network module), `ra-engine` (network_cost module)

**Documentation:** [Network Modeling](network-modeling.md)

**Key types:**
- `NetworkTopology` -- Graph of nodes connected by links with bandwidth,
  latency, and billing cost
- `NetworkCostModel` -- Combines topology with table placement to estimate
  transfer costs
- `DistributionStrategy` -- Broadcast, Shuffle, or CoLocated strategies

### 2. Distribution Strategies

Selects optimal data distribution strategies for joins, including broadcast
(for small tables), shuffle (for large-large joins), and partition-wise
execution. Includes network-locality-aware placement and skew detection.

**Crates:** `ra-core` (distribution module), `ra-engine` (distributed_optimizer module)

**Key types:**
- `DataDistribution` -- How data is partitioned across nodes
- `DistributedOptimizer` -- Rewrites plans with distribution-aware operators
- `ClusterTopology` -- Cluster configuration for the optimizer

**Rules:** 34 rules in `rules/distributed/join-distribution/` and
`rules/distributed/filter-pushdown-distributed/`

### 3. Distributed Aggregation

Implements two-phase and three-phase aggregation strategies for distributed
execution. Detects and handles skewed data distributions.

**Crates:** `ra-core` (distributed_agg module), `ra-stats` (skew module)

**Key types:**
- `TwoPhaseAggregation` -- Local pre-aggregation + global merge
- `ThreePhaseAggregation` -- Adds redistribution phase for high cardinality
- `SkewDetector` -- Identifies skewed key distributions using histograms

**Rules:** 25 rules in `rules/distributed/aggregation/`

### 4. Federated Queries

Optimizes queries spanning multiple database systems. Pushes operations to
source databases when profitable and manages cross-system data movement.

**Crates:** `ra-core` (federated module), `ra-engine` (federated_cost and
federated_optimizer modules)

**Key types:**
- `FederatedCostModel` -- Estimates cost of pushing vs pulling operations
- `FederatedOptimizer` -- Rewrites plans for multi-database execution
- `FederatedAnalysis` -- Tracks capabilities per database

**Rules:** 24 rules in `rules/distributed/` (federated pushdown categories)

## Metrics

| Component                | Lines of Code | Tests | Rules |
|--------------------------|---------------|-------|-------|
| Network Cost Modeling    | 2,770         | 118   | --    |
| Distribution Strategies  | 2,462         | 84    | 34    |
| Distributed Aggregation  | 2,059         | 170   | 25    |
| Federated Queries        | 2,480         | 89    | 24    |
| **Total**                | **9,771**     | **461** | **83** |

Note: Integration testing added Phase 19 total to 12,492 lines of Rust and
406+ passing tests across all modules.

## Architecture

```
                    +-------------------+
                    | SQL Query / Plan  |
                    +--------+----------+
                             |
                    +--------v----------+
                    |  Query Optimizer  |
                    |  (egg e-graph)    |
                    +--------+----------+
                             |
              +--------------+--------------+
              |              |              |
   +----------v---+  +------v------+  +----v---------+
   | Distribution  |  | Aggregation |  |  Federated   |
   | Strategies    |  | Strategies  |  |  Optimizer   |
   +---------+----+  +------+------+  +----+---------+
              |              |              |
              +--------------+--------------+
                             |
                    +--------v----------+
                    |  Network Cost     |
                    |  Model            |
                    +--------+----------+
                             |
                    +--------v----------+
                    |  Network Topology |
                    |  (ra-hardware)    |
                    +-------------------+
```

The network cost model sits at the foundation. Distribution strategies,
aggregation strategies, and federated query planning all use the network
cost model to make decisions about data placement and movement.

## Examples

### Broadcast vs Shuffle Join

```rust
use ra_engine::{NetworkCostModel, JoinSides};
use ra_hardware::{NetworkTopology, NodeId};
use std::collections::HashMap;

let topo = NetworkTopology::multi_datacenter();
let mut assignments = HashMap::new();
assignments.insert("dim_product".into(), NodeId(0));  // small dimension
assignments.insert("fact_sales".into(), NodeId(2));    // large fact table

let model = NetworkCostModel::new(topo, assignments);
let sides = JoinSides {
    left_node: NodeId(0),
    right_node: NodeId(2),
    left_rows: 10_000,       // small dimension table
    right_rows: 100_000_000, // large fact table
    row_width: 200,
};

let strategy = model.recommend_join_strategy(
    &sides,
    &[NodeId(0), NodeId(2)],
    10_000_000, // 10MB broadcast threshold
);
// Returns Broadcast since dim_product (2MB) fits under threshold
```

### Cross-Region Transfer Cost

```rust
use ra_hardware::{NetworkTopology, NodeId};

let topo = NetworkTopology::multi_datacenter();
let one_gb = 1_073_741_824_u64;

// US-East to EU-West: ~80ms latency, $0.01/GB
let time = topo.transfer_time(NodeId(0), NodeId(4), one_gb);
let cost = topo.transfer_cost(NodeId(0), NodeId(4), one_gb);
assert!(time.as_millis() > 8_000); // bandwidth-limited for 1GB
assert!(cost > 0.009);
```

## Related Documentation

- [Network Modeling](network-modeling.md) -- Network topology and cost details
- [Cost Models](cost-models.md) -- Core cost estimation framework
- [Hardware Acceleration](hardware-acceleration.md) -- GPU/FPGA cost models
- [Architecture](architecture.md) -- Overall system design
