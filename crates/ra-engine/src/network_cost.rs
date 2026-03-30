//! Network-aware cost model for distributed query optimization.
//!
//! Integrates [`NetworkTopology`] with query planning to estimate
//! data transfer costs between nodes. Supports broadcast, shuffle,
//! and co-located distribution strategies.

use std::collections::HashMap;
use std::time::Duration;

use ra_core::Cost;
use ra_hardware::network::{NetworkTopology, NodeId};

/// Strategy for distributing data across nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistributionStrategy {
    /// Send the full dataset from source to all target nodes.
    Broadcast {
        /// Node that holds the source data.
        source: NodeId,
        /// Nodes that need a copy of the data.
        targets: Vec<NodeId>,
    },
    /// Hash-partition data and redistribute across target nodes.
    Shuffle {
        /// Node that holds the source data.
        source: NodeId,
        /// Nodes that receive partitions.
        targets: Vec<NodeId>,
    },
    /// Data is already co-located; no network transfer needed.
    CoLocated,
}

/// Describes both sides of a distributed join for strategy selection.
#[derive(Debug, Clone)]
pub struct JoinSides {
    /// Node holding the left input.
    pub left_node: NodeId,
    /// Node holding the right input.
    pub right_node: NodeId,
    /// Row count of the left input.
    pub left_rows: u64,
    /// Row count of the right input.
    pub right_rows: u64,
    /// Average row width in bytes.
    pub row_width: usize,
}

/// Result of a network cost estimation.
#[derive(Debug, Clone)]
pub struct NetworkCostEstimate {
    /// Core cost components (cpu=0, io=0, network=transfer ms).
    pub cost: Cost,
    /// Estimated wall-clock transfer time.
    pub transfer_time: Duration,
    /// Cloud billing cost in dollars.
    pub monetary_cost: f64,
    /// Total bytes transferred.
    pub bytes_transferred: u64,
}

impl NetworkCostEstimate {
    /// Create a zero-cost estimate (no transfer needed).
    #[must_use]
    pub fn zero() -> Self {
        Self {
            cost: Cost::ZERO,
            transfer_time: Duration::ZERO,
            monetary_cost: 0.0,
            bytes_transferred: 0,
        }
    }
}

/// Network-aware cost model for distributed query plans.
///
/// Combines a [`NetworkTopology`] with table-to-node assignments to
/// estimate the cost of data movement between operators executing
/// on different nodes.
#[derive(Debug, Clone)]
pub struct NetworkCostModel {
    topology: NetworkTopology,
    /// Maps table names to the node where they are stored.
    node_assignment: HashMap<String, NodeId>,
}

impl NetworkCostModel {
    /// Create a new network cost model.
    #[must_use]
    pub fn new(
        topology: NetworkTopology,
        node_assignment: HashMap<String, NodeId>,
    ) -> Self {
        Self {
            topology,
            node_assignment,
        }
    }

    /// Get a reference to the underlying topology.
    #[must_use]
    pub fn topology(&self) -> &NetworkTopology {
        &self.topology
    }

    /// Get the node assignment for a table, if known.
    #[must_use]
    pub fn node_for_table(&self, table: &str) -> Option<NodeId> {
        self.node_assignment.get(table).copied()
    }

    /// Assign a table to a node.
    pub fn assign_table(
        &mut self,
        table: impl Into<String>,
        node: NodeId,
    ) {
        self.node_assignment.insert(table.into(), node);
    }

    /// Estimate the cost of transferring a table's data to a target
    /// node.
    ///
    /// Returns a zero cost if the table is already on the target node
    /// or if the table has no known node assignment.
    #[must_use]
    pub fn transfer_cost(
        &self,
        from_table: &str,
        to_node: NodeId,
        rows: u64,
        row_width: usize,
    ) -> NetworkCostEstimate {
        let Some(&source_node) = self.node_assignment.get(from_table)
        else {
            return NetworkCostEstimate::zero();
        };

        if source_node == to_node {
            return NetworkCostEstimate::zero();
        }

        let bytes = rows * row_width as u64;
        let time = self
            .topology
            .transfer_time(source_node, to_node, bytes);
        let billing = self
            .topology
            .transfer_cost(source_node, to_node, bytes);

        NetworkCostEstimate {
            cost: Cost::new(
                0.0,
                0.0,
                time.as_secs_f64() * 1000.0,
                0,
            ),
            transfer_time: time,
            monetary_cost: billing,
            bytes_transferred: bytes,
        }
    }

    /// Estimate the cost of transferring data between two specific
    /// nodes.
    #[must_use]
    pub fn node_transfer_cost(
        &self,
        from: NodeId,
        to: NodeId,
        rows: u64,
        row_width: usize,
    ) -> NetworkCostEstimate {
        if from == to {
            return NetworkCostEstimate::zero();
        }

        let bytes = rows * row_width as u64;
        let time = self.topology.transfer_time(from, to, bytes);
        let billing = self.topology.transfer_cost(from, to, bytes);

        NetworkCostEstimate {
            cost: Cost::new(
                0.0,
                0.0,
                time.as_secs_f64() * 1000.0,
                0,
            ),
            transfer_time: time,
            monetary_cost: billing,
            bytes_transferred: bytes,
        }
    }

    /// Total cost for a distribution strategy.
    ///
    /// - **Broadcast**: sends the full dataset to each target.
    /// - **Shuffle**: hash-partitions and sends `rows / N` to each of
    ///   N targets.
    /// - `CoLocated`: zero cost.
    #[must_use]
    pub fn distribution_cost(
        &self,
        strategy: &DistributionStrategy,
        input_rows: u64,
        row_width: usize,
    ) -> NetworkCostEstimate {
        match strategy {
            DistributionStrategy::Broadcast { source, targets } => {
                self.broadcast_cost(*source, targets, input_rows, row_width)
            }
            DistributionStrategy::Shuffle { source, targets } => {
                self.shuffle_cost(*source, targets, input_rows, row_width)
            }
            DistributionStrategy::CoLocated => {
                NetworkCostEstimate::zero()
            }
        }
    }

    /// Cost of broadcasting all rows from source to each target.
    fn broadcast_cost(
        &self,
        source: NodeId,
        targets: &[NodeId],
        rows: u64,
        row_width: usize,
    ) -> NetworkCostEstimate {
        let bytes_per_target = rows * row_width as u64;
        let mut total_time = Duration::ZERO;
        let mut max_time = Duration::ZERO;
        let mut total_billing = 0.0;
        let mut total_bytes = 0_u64;

        for &target in targets {
            if target == source {
                continue;
            }
            let time = self
                .topology
                .transfer_time(source, target, bytes_per_target);
            let billing = self
                .topology
                .transfer_cost(source, target, bytes_per_target);

            total_time += time;
            if time > max_time {
                max_time = time;
            }
            total_billing += billing;
            total_bytes = total_bytes.saturating_add(bytes_per_target);
        }

        // Use max_time for the network cost since broadcasts can be
        // parallelized; the bottleneck is the slowest target.
        NetworkCostEstimate {
            cost: Cost::new(
                0.0,
                0.0,
                max_time.as_secs_f64() * 1000.0,
                0,
            ),
            transfer_time: max_time,
            monetary_cost: total_billing,
            bytes_transferred: total_bytes,
        }
    }

    /// Cost of hash-partitioning and shuffling rows across targets.
    fn shuffle_cost(
        &self,
        source: NodeId,
        targets: &[NodeId],
        rows: u64,
        row_width: usize,
    ) -> NetworkCostEstimate {
        if targets.is_empty() {
            return NetworkCostEstimate::zero();
        }

        let target_count = targets.len() as u64;
        let rows_per_target = rows / target_count;
        let bytes_per_target = rows_per_target * row_width as u64;

        let mut max_time = Duration::ZERO;
        let mut total_billing = 0.0;
        let mut total_bytes = 0_u64;

        for &target in targets {
            if target == source {
                continue;
            }
            let time = self
                .topology
                .transfer_time(source, target, bytes_per_target);
            let billing = self
                .topology
                .transfer_cost(source, target, bytes_per_target);

            if time > max_time {
                max_time = time;
            }
            total_billing += billing;
            total_bytes = total_bytes.saturating_add(bytes_per_target);
        }

        NetworkCostEstimate {
            cost: Cost::new(
                0.0,
                0.0,
                max_time.as_secs_f64() * 1000.0,
                0,
            ),
            transfer_time: max_time,
            monetary_cost: total_billing,
            bytes_transferred: total_bytes,
        }
    }

    /// Compare two distribution strategies and return the cheaper one.
    ///
    /// Uses weighted total cost (network time + monetary).
    #[must_use]
    pub fn cheaper_strategy<'a>(
        &self,
        a: &'a DistributionStrategy,
        b: &'a DistributionStrategy,
        rows: u64,
        row_width: usize,
    ) -> &'a DistributionStrategy {
        let cost_a = self.distribution_cost(a, rows, row_width);
        let cost_b = self.distribution_cost(b, rows, row_width);
        if cost_a.cost.total() <= cost_b.cost.total() {
            a
        } else {
            b
        }
    }

    /// Recommend whether to broadcast or shuffle for a join.
    ///
    /// Broadcasts the smaller side when it is below the threshold
    /// (in bytes); otherwise shuffles both sides.
    #[must_use]
    pub fn recommend_join_strategy(
        &self,
        sides: &JoinSides,
        targets: &[NodeId],
        broadcast_threshold_bytes: u64,
    ) -> DistributionStrategy {
        if sides.left_node == sides.right_node {
            return DistributionStrategy::CoLocated;
        }

        let left_bytes = sides.left_rows * sides.row_width as u64;
        let right_bytes =
            sides.right_rows * sides.row_width as u64;

        // Broadcast the smaller side if it fits under threshold
        if left_bytes <= broadcast_threshold_bytes
            && left_bytes <= right_bytes
        {
            return DistributionStrategy::Broadcast {
                source: sides.left_node,
                targets: targets.to_vec(),
            };
        }

        if right_bytes <= broadcast_threshold_bytes
            && right_bytes < left_bytes
        {
            return DistributionStrategy::Broadcast {
                source: sides.right_node,
                targets: targets.to_vec(),
            };
        }

        // Default to shuffle
        DistributionStrategy::Shuffle {
            source: sides.left_node,
            targets: targets.to_vec(),
        }
    }

    /// Check if two tables are co-located on the same node.
    #[must_use]
    pub fn tables_colocated(
        &self,
        table_a: &str,
        table_b: &str,
    ) -> bool {
        match (
            self.node_assignment.get(table_a),
            self.node_assignment.get(table_b),
        ) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// Check if two tables are in the same datacenter.
    #[must_use]
    pub fn tables_same_datacenter(
        &self,
        table_a: &str,
        table_b: &str,
    ) -> bool {
        match (
            self.node_assignment.get(table_a),
            self.node_assignment.get(table_b),
        ) {
            (Some(&a), Some(&b)) => {
                self.topology.same_datacenter(a, b)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use ra_hardware::network::{
        LinkType, Location, NetworkLink, NetworkTopology,
    };

    fn simple_model() -> NetworkCostModel {
        let mut topo = NetworkTopology::new();
        let n0 = NodeId(0);
        let n1 = NodeId(1);
        topo.add_node(
            n0,
            Location::new("us-east-1", "us-east-1a"),
        );
        topo.add_node(
            n1,
            Location::new("us-west-2", "us-west-2a"),
        );
        topo.add_link(
            n0,
            n1,
            NetworkLink::new(
                125_000_000, // 1 Gbps
                60_000,      // 60ms
                0.02,        // $0.02/GB
                LinkType::CrossRegion,
            ),
        );

        let mut assignments = HashMap::new();
        assignments.insert("orders".into(), n0);
        assignments.insert("customers".into(), n1);
        assignments.insert("products".into(), n0);

        NetworkCostModel::new(topo, assignments)
    }

    fn colocated_model() -> NetworkCostModel {
        let mut topo = NetworkTopology::new();
        let n0 = NodeId(0);
        topo.add_node(
            n0,
            Location::new("us-east-1", "us-east-1a"),
        );

        let mut assignments = HashMap::new();
        assignments.insert("orders".into(), n0);
        assignments.insert("products".into(), n0);

        NetworkCostModel::new(topo, assignments)
    }

    #[test]
    fn transfer_cost_same_node_is_zero() {
        let model = colocated_model();
        let est = model.transfer_cost("orders", NodeId(0), 1_000_000, 100);
        assert_eq!(est.cost.network, 0.0);
        assert_eq!(est.monetary_cost, 0.0);
        assert_eq!(est.bytes_transferred, 0);
    }

    #[test]
    fn transfer_cost_unknown_table_is_zero() {
        let model = simple_model();
        let est =
            model.transfer_cost("unknown_table", NodeId(0), 1000, 100);
        assert_eq!(est.cost.network, 0.0);
    }

    #[test]
    fn transfer_cost_cross_region() {
        let model = simple_model();
        let est =
            model.transfer_cost("orders", NodeId(1), 1_000_000, 100);
        assert!(est.cost.network > 0.0);
        assert!(est.monetary_cost > 0.0);
        assert_eq!(est.bytes_transferred, 100_000_000);
    }

    #[test]
    fn transfer_cost_network_ms_is_positive() {
        let model = simple_model();
        let est =
            model.transfer_cost("orders", NodeId(1), 10_000, 100);
        assert!(est.cost.network > 0.0);
        assert!(est.transfer_time > Duration::ZERO);
    }

    #[test]
    fn node_transfer_cost_same_node() {
        let model = simple_model();
        let est =
            model.node_transfer_cost(NodeId(0), NodeId(0), 1000, 100);
        assert_eq!(est.cost.network, 0.0);
    }

    #[test]
    fn node_transfer_cost_cross_region() {
        let model = simple_model();
        let est =
            model.node_transfer_cost(NodeId(0), NodeId(1), 1000, 100);
        assert!(est.cost.network > 0.0);
    }

    #[test]
    fn distribution_cost_colocated_is_zero() {
        let model = simple_model();
        let est = model.distribution_cost(
            &DistributionStrategy::CoLocated,
            1_000_000,
            100,
        );
        assert_eq!(est.cost.network, 0.0);
        assert_eq!(est.monetary_cost, 0.0);
    }

    #[test]
    fn distribution_cost_broadcast() {
        let model = simple_model();
        let est = model.distribution_cost(
            &DistributionStrategy::Broadcast {
                source: NodeId(0),
                targets: vec![NodeId(1)],
            },
            100_000,
            100,
        );
        assert!(est.cost.network > 0.0);
        assert!(est.monetary_cost > 0.0);
        assert_eq!(est.bytes_transferred, 10_000_000);
    }

    #[test]
    fn distribution_cost_broadcast_skips_self() {
        let model = simple_model();
        let est = model.distribution_cost(
            &DistributionStrategy::Broadcast {
                source: NodeId(0),
                targets: vec![NodeId(0), NodeId(1)],
            },
            100_000,
            100,
        );
        // Only transfers to node 1, not to self
        assert_eq!(est.bytes_transferred, 10_000_000);
    }

    #[test]
    fn distribution_cost_shuffle() {
        let model = simple_model();
        let est = model.distribution_cost(
            &DistributionStrategy::Shuffle {
                source: NodeId(0),
                targets: vec![NodeId(0), NodeId(1)],
            },
            100_000,
            100,
        );
        // Shuffle: 50k rows to node 1 (self is skipped)
        assert!(est.cost.network > 0.0);
        assert_eq!(est.bytes_transferred, 5_000_000);
    }

    #[test]
    fn distribution_cost_shuffle_empty_targets() {
        let model = simple_model();
        let est = model.distribution_cost(
            &DistributionStrategy::Shuffle {
                source: NodeId(0),
                targets: vec![],
            },
            100_000,
            100,
        );
        assert_eq!(est.cost.network, 0.0);
    }

    #[test]
    fn broadcast_more_expensive_than_shuffle_for_large_data() {
        let model = simple_model();
        let rows = 10_000_000;
        let width = 100;
        let targets = vec![NodeId(0), NodeId(1)];

        let broadcast = model.distribution_cost(
            &DistributionStrategy::Broadcast {
                source: NodeId(0),
                targets: targets.clone(),
            },
            rows,
            width,
        );
        let shuffle = model.distribution_cost(
            &DistributionStrategy::Shuffle {
                source: NodeId(0),
                targets,
            },
            rows,
            width,
        );

        assert!(broadcast.bytes_transferred > shuffle.bytes_transferred);
    }

    #[test]
    fn cheaper_strategy_prefers_colocated() {
        let model = simple_model();
        let broadcast = DistributionStrategy::Broadcast {
            source: NodeId(0),
            targets: vec![NodeId(1)],
        };
        let colocated = DistributionStrategy::CoLocated;

        let result = model.cheaper_strategy(
            &broadcast,
            &colocated,
            1_000_000,
            100,
        );
        assert_eq!(*result, DistributionStrategy::CoLocated);
    }

    #[test]
    fn recommend_join_strategy_colocated() {
        let model = simple_model();
        let sides = JoinSides {
            left_node: NodeId(0),
            right_node: NodeId(0),
            left_rows: 1_000_000,
            right_rows: 1_000_000,
            row_width: 100,
        };
        let strategy = model.recommend_join_strategy(
            &sides,
            &[NodeId(0)],
            100_000_000,
        );
        assert_eq!(strategy, DistributionStrategy::CoLocated);
    }

    #[test]
    fn recommend_join_strategy_broadcast_small_left() {
        let model = simple_model();
        let sides = JoinSides {
            left_node: NodeId(0),
            right_node: NodeId(1),
            left_rows: 100,
            right_rows: 1_000_000,
            row_width: 100,
        };
        let strategy = model.recommend_join_strategy(
            &sides,
            &[NodeId(0), NodeId(1)],
            1_000_000,
        );
        match &strategy {
            DistributionStrategy::Broadcast { source, .. } => {
                assert_eq!(*source, NodeId(0));
            }
            other => panic!("expected Broadcast, got {other:?}"),
        }
    }

    #[test]
    fn recommend_join_strategy_broadcast_small_right() {
        let model = simple_model();
        let sides = JoinSides {
            left_node: NodeId(0),
            right_node: NodeId(1),
            left_rows: 1_000_000,
            right_rows: 100,
            row_width: 100,
        };
        let strategy = model.recommend_join_strategy(
            &sides,
            &[NodeId(0), NodeId(1)],
            1_000_000,
        );
        match &strategy {
            DistributionStrategy::Broadcast { source, .. } => {
                assert_eq!(*source, NodeId(1));
            }
            other => panic!("expected Broadcast, got {other:?}"),
        }
    }

    #[test]
    fn recommend_join_strategy_shuffle_when_both_large() {
        let model = simple_model();
        let sides = JoinSides {
            left_node: NodeId(0),
            right_node: NodeId(1),
            left_rows: 10_000_000,
            right_rows: 10_000_000,
            row_width: 100,
        };
        let strategy = model.recommend_join_strategy(
            &sides,
            &[NodeId(0), NodeId(1)],
            1_000_000,
        );
        assert!(matches!(
            strategy,
            DistributionStrategy::Shuffle { .. }
        ));
    }

    #[test]
    fn tables_colocated_same_node() {
        let model = simple_model();
        assert!(model.tables_colocated("orders", "products"));
    }

    #[test]
    fn tables_colocated_different_nodes() {
        let model = simple_model();
        assert!(!model.tables_colocated("orders", "customers"));
    }

    #[test]
    fn tables_colocated_unknown_table() {
        let model = simple_model();
        assert!(!model.tables_colocated("orders", "unknown"));
    }

    #[test]
    fn tables_same_datacenter_true() {
        let model = colocated_model();
        assert!(
            model.tables_same_datacenter("orders", "products")
        );
    }

    #[test]
    fn tables_same_datacenter_false() {
        let model = simple_model();
        assert!(
            !model.tables_same_datacenter("orders", "customers")
        );
    }

    #[test]
    fn tables_same_datacenter_unknown() {
        let model = simple_model();
        assert!(
            !model.tables_same_datacenter("orders", "unknown")
        );
    }

    #[test]
    fn topology_accessor() {
        let model = simple_model();
        assert_eq!(model.topology().node_count(), 2);
    }

    #[test]
    fn node_for_table_exists() {
        let model = simple_model();
        assert_eq!(model.node_for_table("orders"), Some(NodeId(0)));
    }

    #[test]
    fn node_for_table_missing() {
        let model = simple_model();
        assert_eq!(model.node_for_table("unknown"), None);
    }

    #[test]
    fn assign_table_adds_mapping() {
        let mut model = simple_model();
        model.assign_table("inventory", NodeId(1));
        assert_eq!(
            model.node_for_table("inventory"),
            Some(NodeId(1))
        );
    }

    #[test]
    fn assign_table_overwrites() {
        let mut model = simple_model();
        model.assign_table("orders", NodeId(1));
        assert_eq!(model.node_for_table("orders"), Some(NodeId(1)));
    }

    #[test]
    fn monetary_cost_scales_with_data_size() {
        let model = simple_model();
        let small = model.transfer_cost("orders", NodeId(1), 1_000, 100);
        let large =
            model.transfer_cost("orders", NodeId(1), 1_000_000, 100);
        assert!(large.monetary_cost > small.monetary_cost);
    }

    #[test]
    fn transfer_time_scales_with_data_size() {
        let model = simple_model();
        let small = model.transfer_cost("orders", NodeId(1), 1_000, 100);
        let large =
            model.transfer_cost("orders", NodeId(1), 1_000_000, 100);
        assert!(large.transfer_time > small.transfer_time);
    }

    #[test]
    fn distribution_cost_broadcast_to_multiple() {
        let mut topo = NetworkTopology::new();
        for i in 0..4_u32 {
            topo.add_node(
                NodeId(i),
                Location::new("us-east-1", "us-east-1a"),
            );
        }
        for i in 1..4 {
            topo.add_link(
                NodeId(0),
                NodeId(i),
                NetworkLink::from_type(LinkType::IntraDatacenter),
            );
        }

        let mut assignments = HashMap::new();
        assignments.insert("data".into(), NodeId(0));
        let model = NetworkCostModel::new(topo, assignments);

        let est = model.distribution_cost(
            &DistributionStrategy::Broadcast {
                source: NodeId(0),
                targets: vec![NodeId(1), NodeId(2), NodeId(3)],
            },
            100_000,
            100,
        );
        // 3 targets, each gets 10MB
        assert_eq!(est.bytes_transferred, 30_000_000);
    }

    #[test]
    fn shuffle_divides_rows_evenly() {
        let model = simple_model();
        let est = model.distribution_cost(
            &DistributionStrategy::Shuffle {
                source: NodeId(0),
                targets: vec![NodeId(0), NodeId(1)],
            },
            200_000,
            100,
        );
        // 200k rows / 2 targets = 100k rows per target
        // Only node 1 gets data (self skipped), 100k * 100 = 10MB
        assert_eq!(est.bytes_transferred, 10_000_000);
    }

    #[test]
    fn with_single_dc_profile() {
        let topo = NetworkTopology::single_datacenter_cluster();
        let mut assignments = HashMap::new();
        assignments.insert("t1".into(), NodeId(0));
        assignments.insert("t2".into(), NodeId(2));
        let model = NetworkCostModel::new(topo, assignments);

        let est = model.transfer_cost("t1", NodeId(2), 1_000_000, 100);
        // Intra-datacenter: fast, free
        assert!(est.cost.network > 0.0);
        assert_eq!(est.monetary_cost, 0.0);
    }

    #[test]
    fn with_multi_dc_profile() {
        let topo = NetworkTopology::multi_datacenter();
        let mut assignments = HashMap::new();
        assignments.insert("us_data".into(), NodeId(0));
        assignments.insert("eu_data".into(), NodeId(4));
        let model = NetworkCostModel::new(topo, assignments);

        let est =
            model.transfer_cost("us_data", NodeId(4), 1_000_000, 100);
        assert!(est.cost.network > 0.0);
        assert!(est.monetary_cost > 0.0);
    }

    #[test]
    fn with_cloud_federation_profile() {
        let topo = NetworkTopology::cloud_federation();
        let mut assignments = HashMap::new();
        assignments.insert("aws_table".into(), NodeId(0));
        assignments.insert("gcp_table".into(), NodeId(2));
        let model = NetworkCostModel::new(topo, assignments);

        let est =
            model.transfer_cost("aws_table", NodeId(2), 100_000, 100);
        assert!(est.cost.network > 0.0);
        // Cross-cloud has high monetary cost
        assert!(est.monetary_cost > 0.0);
    }

    #[test]
    fn network_cost_zero_rows() {
        let model = simple_model();
        let est = model.transfer_cost("orders", NodeId(1), 0, 100);
        // Zero rows = only latency
        assert!(est.bytes_transferred == 0);
    }

    #[test]
    fn broadcast_cost_parallel_time() {
        // Broadcast uses max_time (parallel), not total_time
        let mut topo = NetworkTopology::new();
        for i in 0..3_u32 {
            topo.add_node(
                NodeId(i),
                Location::new("us-east-1", "us-east-1a"),
            );
        }
        // Link to node 1: fast
        topo.add_link(
            NodeId(0),
            NodeId(1),
            NetworkLink::from_type(LinkType::IntraRack),
        );
        // Link to node 2: slow
        topo.add_link(
            NodeId(0),
            NodeId(2),
            NetworkLink::new(
                1_000_000, // 1 MBps
                100_000,   // 100ms
                0.0,
                LinkType::Internet,
            ),
        );

        let mut assignments = HashMap::new();
        assignments.insert("data".into(), NodeId(0));
        let model = NetworkCostModel::new(topo, assignments);

        let est = model.distribution_cost(
            &DistributionStrategy::Broadcast {
                source: NodeId(0),
                targets: vec![NodeId(1), NodeId(2)],
            },
            10_000,
            100,
        );

        // Transfer time should be the max (slow link), not the sum
        let slow_est =
            model.node_transfer_cost(NodeId(0), NodeId(2), 10_000, 100);
        assert!(
            (est.transfer_time.as_secs_f64()
                - slow_est.transfer_time.as_secs_f64())
            .abs()
                < 0.001
        );
    }
}
