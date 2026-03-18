# Network Cost Modeling

Network-aware cost modeling for distributed query optimization. This module
estimates data transfer costs between cluster nodes, factoring in bandwidth,
latency, and cloud billing.

## Architecture

The network modeling system has three layers:

1. **Network Topology** (`ra-hardware::network`) -- Physical connectivity
   between nodes: bandwidth, latency, billing cost per link.
2. **Network Profiles** (`ra-hardware::network_profiles`) -- Predefined
   topologies for common deployment patterns.
3. **Network Cost Model** (`ra-engine::network_cost`) -- Query-level cost
   estimation integrating topology with table placement.

## Core Types

### NodeId

Unique identifier for a cluster node. Wraps a `u32`.

```rust
use ra_hardware::NodeId;
let node = NodeId(0);
```

### Location

Physical or logical location of a node, used to infer link types.

```rust
use ra_hardware::Location;

// Datacenter without rack info
let loc = Location::new("us-east-1", "us-east-1a");

// With rack assignment
let loc = Location::with_rack("us-east-1", "us-east-1a", "rack-1");
```

### LinkType

Classification of network links by physical topology:

| Type              | Bandwidth    | Latency   | Cost/GB |
|-------------------|-------------|-----------|---------|
| IntraRack         | 100 Gbps    | <1 us     | $0.00   |
| IntraDatacenter   | 10 Gbps     | 5 us      | $0.00   |
| CrossDatacenter   | 1 Gbps      | 5 ms      | $0.01   |
| CrossRegion       | 100 Mbps    | 100 ms    | $0.02   |
| Internet          | 50 Mbps     | 150 ms    | $0.09   |

### NetworkLink

A connection between two nodes with specific characteristics.

```rust
use ra_hardware::{NetworkLink, LinkType};

// From defaults
let link = NetworkLink::from_type(LinkType::CrossRegion);

// Custom
let link = NetworkLink::new(
    125_000_000,  // 1 Gbps
    60_000,       // 60ms latency
    0.02,         // $0.02/GB
    LinkType::CrossRegion,
);
```

### NetworkTopology

Graph of nodes connected by links.

```rust
use ra_hardware::{NetworkTopology, NodeId, Location, NetworkLink, LinkType};

let mut topo = NetworkTopology::new();
topo.add_node(NodeId(0), Location::new("us-east-1", "us-east-1a"));
topo.add_node(NodeId(1), Location::new("eu-west-1", "eu-west-1a"));
topo.add_link(
    NodeId(0),
    NodeId(1),
    NetworkLink::from_type(LinkType::CrossRegion),
);

// Query the topology
let time = topo.transfer_time(NodeId(0), NodeId(1), 1_073_741_824);
let cost = topo.transfer_cost(NodeId(0), NodeId(1), 1_073_741_824);
let same_dc = topo.same_datacenter(NodeId(0), NodeId(1));
```

## Predefined Profiles

Five profiles cover common deployment patterns:

### single_datacenter_cluster

4 nodes across 2 racks. 100 Gbps intra-rack, 10 Gbps cross-rack. No billing
cost. Models on-premises Hadoop/Spark clusters.

### multi_datacenter

6 nodes across 3 datacenters (US-East, US-West, EU-West). 10 Gbps intra-DC,
1 Gbps cross-DC. $0.01/GB cross-DC transfer. Models geo-replicated databases
like CockroachDB.

### cloud_federation

6 nodes across 3 clouds (AWS, GCP, Azure). 10 Gbps intra-cloud, 50 Mbps
cross-cloud. $0.09/GB internet egress. Models federated query engines.

### edge_cloud

2 cloud nodes + 4 edge nodes with asymmetric bandwidth (upload slower than
download). 1-10 Mbps uplinks with 20-200ms latency. Models IoT and CDN
architectures.

### data_warehouse

4 compute nodes + 2 storage nodes (S3-style). 25 Gbps compute-to-storage,
10 Gbps compute-to-compute. $0.0025/GB storage access. Models Snowflake-style
compute-storage separation.

```rust
use ra_hardware::NetworkTopology;

let topo = NetworkTopology::multi_datacenter();
assert_eq!(topo.node_count(), 6);
assert_eq!(topo.datacenters().len(), 3);
```

## Network Cost Model

The `NetworkCostModel` combines a topology with table-to-node assignments to
estimate data movement costs.

```rust
use std::collections::HashMap;
use ra_hardware::{NetworkTopology, NodeId};
use ra_engine::NetworkCostModel;

let topo = NetworkTopology::multi_datacenter();
let mut assignments = HashMap::new();
assignments.insert("orders".into(), NodeId(0));
assignments.insert("customers".into(), NodeId(4));

let model = NetworkCostModel::new(topo, assignments);

// Estimate transfer cost
let est = model.transfer_cost("orders", NodeId(4), 1_000_000, 100);
println!("Network time: {} ms", est.cost.network);
println!("Billing cost: ${:.4}", est.monetary_cost);
println!("Bytes transferred: {}", est.bytes_transferred);
```

## Distribution Strategies

Three strategies for moving data in distributed joins:

- **Broadcast**: Send the full dataset to all target nodes. Best for small
  dimension tables.
- **Shuffle**: Hash-partition data across targets. Each target receives
  `rows / N` rows. Best for large-large joins.
- **CoLocated**: No data movement. Both inputs already on the same node.

```rust
use ra_engine::{NetworkCostModel, DistributionStrategy, JoinSides};
use ra_hardware::NodeId;

// The model recommends a strategy based on data sizes
let sides = JoinSides {
    left_node: NodeId(0),
    right_node: NodeId(4),
    left_rows: 100,          // small dimension table
    right_rows: 10_000_000,  // large fact table
    row_width: 100,
};
let strategy = model.recommend_join_strategy(
    &sides,
    &[NodeId(0), NodeId(4)],
    1_000_000,  // broadcast threshold: 1 MB
);
// Returns Broadcast { source: NodeId(0), ... } since left side is small
```

## Cost Estimation Details

Transfer time is computed as: `latency + (bytes / bandwidth)`

Cloud billing is computed as: `(bytes / 1 GB) * cost_per_gb`

For broadcast operations, the model uses **parallel time** (max across all
targets) rather than sequential time, since transfers happen concurrently.

For shuffle operations, each target receives `rows / N` rows, and only
non-local targets incur transfer cost.
