//! Distribution strategies for distributed query execution.
//!
//! This module defines data distribution models and strategies for
//! moving data between nodes in a distributed cluster. The optimizer
//! uses these to select the cheapest way to execute joins,
//! aggregations, and other operators across multiple nodes.

use serde::{Deserialize, Serialize};

use crate::expr::Expr;

/// Unique identifier for a node in the cluster.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
    Serialize, Deserialize,
)]
pub struct NodeId(pub u32);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "node-{}", self.0)
    }
}

/// How data is physically distributed across cluster nodes.
///
/// This describes the current distribution of a relation, not a
/// redistribution action. The optimizer inspects this to decide
/// whether data movement is needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataDistribution {
    /// No guarantees on data placement.
    Arbitrary,

    /// Fully replicated on every node.
    Replicated,

    /// Hash-partitioned by the given key expressions.
    HashPartitioned {
        /// Expressions whose hash determines the partition.
        keys: Vec<Expr>,
        /// Number of partitions (may differ from node count).
        partition_count: u32,
    },

    /// Range-partitioned by a single key with explicit boundaries.
    RangePartitioned {
        /// The partitioning key expression.
        key: Expr,
        /// Ordered range boundaries (N boundaries = N+1 partitions).
        boundaries: Vec<RangeBoundary>,
    },

    /// All data resides on a single node.
    SinglePartition {
        /// The node holding the data.
        node: NodeId,
    },
}

/// A boundary value in a range partition scheme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeBoundary {
    /// Boundary value as a sortable string representation.
    pub value: String,
    /// Whether this boundary is inclusive.
    pub inclusive: bool,
}

/// A strategy for redistributing data before an operator executes.
///
/// Each variant describes a specific data movement pattern.
/// The optimizer enumerates candidate strategies and picks the
/// cheapest one according to the network cost model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DistributionStrategy {
    /// Send the entire dataset to all target nodes.
    ///
    /// Best when one side of a join is small enough to fit in
    /// memory on every node.
    Broadcast {
        /// Node holding the source data.
        source: NodeId,
        /// Nodes that will receive a full copy.
        targets: Vec<NodeId>,
    },

    /// Hash-partition the data and send each partition to its
    /// assigned target node.
    Shuffle {
        /// Node holding the source data.
        source: NodeId,
        /// Destination nodes for each partition.
        targets: Vec<NodeId>,
        /// Expressions used to compute the hash partition.
        partition_keys: Vec<Expr>,
    },

    /// Data is already co-located; no movement needed.
    CoLocated,

    /// Execute the operator partition-by-partition without moving
    /// data. Both inputs must be partitioned on the same key.
    PartitionWise {
        /// The common partition key.
        partition_key: Expr,
    },

    /// Repartition data using range boundaries for ordered
    /// operations (e.g., distributed sort-merge join).
    RangePartition {
        /// The partitioning key expression.
        partition_key: Expr,
        /// Range boundaries defining the partitions.
        ranges: Vec<(String, String)>,
    },

    /// Broadcast to a subset of nodes rather than all nodes.
    ///
    /// Useful when a predicate restricts which partitions
    /// participate in the join.
    PartialBroadcast {
        /// Node holding the source data.
        source: NodeId,
        /// Subset of nodes that need the data.
        targets: Vec<NodeId>,
        /// The predicate that restricts the target set.
        predicate: Expr,
    },
}

impl DistributionStrategy {
    /// Return the number of target nodes in this strategy.
    #[must_use]
    pub fn target_count(&self) -> usize {
        match self {
            Self::Broadcast { targets, .. }
            | Self::Shuffle { targets, .. }
            | Self::PartialBroadcast { targets, .. } => targets.len(),
            Self::CoLocated | Self::PartitionWise { .. } => 0,
            Self::RangePartition { ranges, .. } => ranges.len(),
        }
    }

    /// Whether this strategy requires network data transfer.
    #[must_use]
    pub fn requires_network(&self) -> bool {
        !matches!(self, Self::CoLocated | Self::PartitionWise { .. })
    }

    /// Human-readable label for the strategy kind.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Broadcast { .. } => "Broadcast",
            Self::Shuffle { .. } => "Shuffle",
            Self::CoLocated => "CoLocated",
            Self::PartitionWise { .. } => "PartitionWise",
            Self::RangePartition { .. } => "RangePartition",
            Self::PartialBroadcast { .. } => "PartialBroadcast",
        }
    }
}

/// A distributed relational expression: a logical plan annotated
/// with data distribution information and node assignments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistributedRelExpr {
    /// The logical relational expression.
    pub plan: crate::algebra::RelExpr,
    /// How the output data is distributed.
    pub distribution: DataDistribution,
    /// Which node this fragment executes on (for single-partition).
    pub node_assignment: Option<NodeId>,
    /// The strategy used to distribute inputs for this operator.
    pub input_strategy: Option<DistributionStrategy>,
}

impl DistributedRelExpr {
    /// Create a new distributed expression with arbitrary
    /// distribution and no node assignment.
    #[must_use]
    pub fn new(plan: crate::algebra::RelExpr) -> Self {
        Self {
            plan,
            distribution: DataDistribution::Arbitrary,
            node_assignment: None,
            input_strategy: None,
        }
    }

    /// Set the data distribution.
    #[must_use]
    pub fn with_distribution(
        mut self,
        distribution: DataDistribution,
    ) -> Self {
        self.distribution = distribution;
        self
    }

    /// Assign this fragment to a specific node.
    #[must_use]
    pub fn on_node(mut self, node: NodeId) -> Self {
        self.node_assignment = Some(node);
        self
    }

    /// Set the input redistribution strategy.
    #[must_use]
    pub fn with_strategy(
        mut self,
        strategy: DistributionStrategy,
    ) -> Self {
        self.input_strategy = Some(strategy);
        self
    }

    /// Whether the output is hash-partitioned on the given keys.
    #[must_use]
    pub fn is_partitioned_on(&self, keys: &[Expr]) -> bool {
        if let DataDistribution::HashPartitioned {
            keys: dist_keys, ..
        } = &self.distribution
        {
            keys == dist_keys.as_slice()
        } else {
            false
        }
    }

    /// Whether the data is fully replicated.
    #[must_use]
    pub fn is_replicated(&self) -> bool {
        matches!(self.distribution, DataDistribution::Replicated)
    }

    /// Whether the data lives on a single node.
    #[must_use]
    pub fn is_single_partition(&self) -> bool {
        matches!(
            self.distribution,
            DataDistribution::SinglePartition { .. }
        )
    }
}

/// Compatibility information between two distributions.
///
/// Used by the optimizer to decide whether redistribution is
/// needed for a binary operator (e.g., join).
#[derive(Debug, Clone, PartialEq)]
pub enum DistributionCompatibility {
    /// Both sides are co-located on the join key; no movement
    /// needed.
    Compatible,
    /// The left side must be redistributed.
    LeftMustRedistribute,
    /// The right side must be redistributed.
    RightMustRedistribute,
    /// Both sides must be redistributed.
    BothMustRedistribute,
}

/// Check whether two distributions are compatible for a join on
/// the given key expressions.
#[must_use]
pub fn check_join_compatibility(
    left: &DataDistribution,
    right: &DataDistribution,
    join_keys_left: &[Expr],
    join_keys_right: &[Expr],
) -> DistributionCompatibility {
    // If either side is replicated, no redistribution needed.
    if matches!(left, DataDistribution::Replicated)
        || matches!(right, DataDistribution::Replicated)
    {
        return DistributionCompatibility::Compatible;
    }

    // If both sides are hash-partitioned on the join keys, they
    // are co-located.
    if let (
        DataDistribution::HashPartitioned {
            keys: l_keys,
            partition_count: l_count,
        },
        DataDistribution::HashPartitioned {
            keys: r_keys,
            partition_count: r_count,
        },
    ) = (left, right)
    {
        if l_keys == join_keys_left
            && r_keys == join_keys_right
            && l_count == r_count
        {
            return DistributionCompatibility::Compatible;
        }
    }

    // If left is hash-partitioned on join key, only right needs
    // redistribution.
    if let DataDistribution::HashPartitioned { keys, .. } = left {
        if keys == join_keys_left {
            return DistributionCompatibility::RightMustRedistribute;
        }
    }

    // Symmetric case.
    if let DataDistribution::HashPartitioned { keys, .. } = right {
        if keys == join_keys_right {
            return DistributionCompatibility::LeftMustRedistribute;
        }
    }

    DistributionCompatibility::BothMustRedistribute
}

/// Decide whether broadcasting is cheaper than shuffling.
///
/// Returns `true` when the smaller side's broadcast cost is less
/// than reshuffling both sides.
#[must_use]
pub fn should_broadcast(
    small_bytes: u64,
    large_bytes: u64,
    num_nodes: u32,
    broadcast_threshold: u64,
) -> bool {
    if small_bytes > broadcast_threshold {
        return false;
    }
    if num_nodes == 0 {
        return false;
    }
    let broadcast_cost =
        u128::from(small_bytes) * u128::from(num_nodes);
    let shuffle_fraction_num = u128::from(num_nodes - 1);
    let shuffle_cost =
        (u128::from(small_bytes) + u128::from(large_bytes))
            * shuffle_fraction_num
            / u128::from(num_nodes);
    broadcast_cost < shuffle_cost
}

/// Parameters for selecting a join distribution strategy.
pub struct JoinStrategyInput<'a> {
    /// Distribution of the left input.
    pub left_dist: &'a DataDistribution,
    /// Distribution of the right input.
    pub right_dist: &'a DataDistribution,
    /// Estimated byte size of the left input.
    pub left_bytes: u64,
    /// Estimated byte size of the right input.
    pub right_bytes: u64,
    /// Left-side join key expressions.
    pub join_keys_left: &'a [Expr],
    /// Right-side join key expressions.
    pub join_keys_right: &'a [Expr],
    /// Available cluster nodes.
    pub nodes: &'a [NodeId],
    /// Max bytes for a broadcast candidate.
    pub broadcast_threshold: u64,
}

/// Select the best distribution strategy for a join.
///
/// Evaluates broadcast, shuffle, co-located, and partition-wise
/// strategies and returns the cheapest one.
#[must_use]
pub fn select_join_strategy(
    input: &JoinStrategyInput<'_>,
) -> DistributionStrategy {
    let JoinStrategyInput {
        left_dist,
        right_dist,
        left_bytes,
        right_bytes,
        join_keys_left,
        join_keys_right,
        nodes,
        broadcast_threshold,
    } = input;
    let left_bytes = *left_bytes;
    let right_bytes = *right_bytes;
    let broadcast_threshold = *broadcast_threshold;
    let compat = check_join_compatibility(
        left_dist,
        right_dist,
        join_keys_left,
        join_keys_right,
    );

    // Already compatible - no movement needed.
    if compat == DistributionCompatibility::Compatible {
        // Determine if it is partition-wise (both hash-partitioned).
        if let DataDistribution::HashPartitioned { keys, .. } =
            left_dist
        {
            if let Some(key) = keys.first() {
                return DistributionStrategy::PartitionWise {
                    partition_key: key.clone(),
                };
            }
        }
        return DistributionStrategy::CoLocated;
    }

    // Try broadcasting the smaller side.
    let (small_bytes, _large_bytes, broadcast_source) =
        if left_bytes <= right_bytes {
            (left_bytes, right_bytes, nodes.first().copied())
        } else {
            (right_bytes, left_bytes, nodes.first().copied())
        };

    if let Some(source) = broadcast_source {
        if should_broadcast(
            small_bytes,
            left_bytes.max(right_bytes),
            u32::try_from(nodes.len()).unwrap_or(u32::MAX),
            broadcast_threshold,
        ) {
            return DistributionStrategy::Broadcast {
                source,
                targets: nodes.to_vec(),
            };
        }
    }

    // Default: shuffle both sides by join key.
    let source = nodes.first().copied().unwrap_or(NodeId(0));
    let keys = if left_bytes <= right_bytes {
        join_keys_left.to_vec()
    } else {
        join_keys_right.to_vec()
    };

    DistributionStrategy::Shuffle {
        source,
        targets: nodes.to_vec(),
        partition_keys: keys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ColumnRef, Const};

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef::new(name))
    }

    fn nodes(n: u32) -> Vec<NodeId> {
        (0..n).map(NodeId).collect()
    }

    // --- NodeId ---

    #[test]
    fn node_id_display() {
        assert_eq!(NodeId(0).to_string(), "node-0");
        assert_eq!(NodeId(42).to_string(), "node-42");
    }

    #[test]
    fn node_id_ordering() {
        assert!(NodeId(0) < NodeId(1));
        assert_eq!(NodeId(5), NodeId(5));
    }

    // --- DataDistribution ---

    #[test]
    fn data_distribution_arbitrary() {
        let d = DataDistribution::Arbitrary;
        assert_eq!(d, DataDistribution::Arbitrary);
    }

    #[test]
    fn data_distribution_replicated() {
        let d = DataDistribution::Replicated;
        assert_eq!(d, DataDistribution::Replicated);
    }

    #[test]
    fn data_distribution_hash_partitioned() {
        let d = DataDistribution::HashPartitioned {
            keys: vec![col("id")],
            partition_count: 8,
        };
        if let DataDistribution::HashPartitioned {
            keys,
            partition_count,
        } = &d
        {
            assert_eq!(keys.len(), 1);
            assert_eq!(*partition_count, 8);
        } else {
            panic!("expected HashPartitioned");
        }
    }

    #[test]
    fn data_distribution_range_partitioned() {
        let d = DataDistribution::RangePartitioned {
            key: col("ts"),
            boundaries: vec![
                RangeBoundary {
                    value: "2024-01-01".into(),
                    inclusive: true,
                },
                RangeBoundary {
                    value: "2024-07-01".into(),
                    inclusive: false,
                },
            ],
        };
        if let DataDistribution::RangePartitioned {
            boundaries, ..
        } = &d
        {
            assert_eq!(boundaries.len(), 2);
        } else {
            panic!("expected RangePartitioned");
        }
    }

    #[test]
    fn data_distribution_single_partition() {
        let d = DataDistribution::SinglePartition { node: NodeId(3) };
        if let DataDistribution::SinglePartition { node } = &d {
            assert_eq!(*node, NodeId(3));
        } else {
            panic!("expected SinglePartition");
        }
    }

    // --- DistributionStrategy ---

    #[test]
    fn strategy_broadcast_target_count() {
        let s = DistributionStrategy::Broadcast {
            source: NodeId(0),
            targets: nodes(4),
        };
        assert_eq!(s.target_count(), 4);
        assert!(s.requires_network());
        assert_eq!(s.label(), "Broadcast");
    }

    #[test]
    fn strategy_shuffle_target_count() {
        let s = DistributionStrategy::Shuffle {
            source: NodeId(0),
            targets: nodes(8),
            partition_keys: vec![col("id")],
        };
        assert_eq!(s.target_count(), 8);
        assert!(s.requires_network());
        assert_eq!(s.label(), "Shuffle");
    }

    #[test]
    fn strategy_colocated() {
        let s = DistributionStrategy::CoLocated;
        assert_eq!(s.target_count(), 0);
        assert!(!s.requires_network());
        assert_eq!(s.label(), "CoLocated");
    }

    #[test]
    fn strategy_partition_wise() {
        let s = DistributionStrategy::PartitionWise {
            partition_key: col("region"),
        };
        assert_eq!(s.target_count(), 0);
        assert!(!s.requires_network());
        assert_eq!(s.label(), "PartitionWise");
    }

    #[test]
    fn strategy_range_partition() {
        let s = DistributionStrategy::RangePartition {
            partition_key: col("ts"),
            ranges: vec![
                ("0".into(), "100".into()),
                ("100".into(), "200".into()),
            ],
        };
        assert_eq!(s.target_count(), 2);
        assert!(s.requires_network());
        assert_eq!(s.label(), "RangePartition");
    }

    #[test]
    fn strategy_partial_broadcast() {
        let s = DistributionStrategy::PartialBroadcast {
            source: NodeId(0),
            targets: vec![NodeId(1), NodeId(2)],
            predicate: Expr::Const(Const::Bool(true)),
        };
        assert_eq!(s.target_count(), 2);
        assert!(s.requires_network());
        assert_eq!(s.label(), "PartialBroadcast");
    }

    // --- DistributedRelExpr ---

    #[test]
    fn distributed_rel_expr_new() {
        let plan = crate::algebra::RelExpr::scan("users");
        let dre = DistributedRelExpr::new(plan.clone());
        assert_eq!(dre.distribution, DataDistribution::Arbitrary);
        assert!(dre.node_assignment.is_none());
        assert!(dre.input_strategy.is_none());
    }

    #[test]
    fn distributed_rel_expr_with_distribution() {
        let plan = crate::algebra::RelExpr::scan("users");
        let dre = DistributedRelExpr::new(plan)
            .with_distribution(DataDistribution::Replicated);
        assert!(dre.is_replicated());
    }

    #[test]
    fn distributed_rel_expr_on_node() {
        let plan = crate::algebra::RelExpr::scan("orders");
        let dre = DistributedRelExpr::new(plan).on_node(NodeId(2));
        assert_eq!(dre.node_assignment, Some(NodeId(2)));
    }

    #[test]
    fn distributed_rel_expr_with_strategy() {
        let plan = crate::algebra::RelExpr::scan("orders");
        let dre = DistributedRelExpr::new(plan)
            .with_strategy(DistributionStrategy::CoLocated);
        assert_eq!(
            dre.input_strategy,
            Some(DistributionStrategy::CoLocated)
        );
    }

    #[test]
    fn distributed_rel_expr_is_partitioned_on() {
        let plan = crate::algebra::RelExpr::scan("orders");
        let keys = vec![col("region")];
        let dre = DistributedRelExpr::new(plan).with_distribution(
            DataDistribution::HashPartitioned {
                keys: keys.clone(),
                partition_count: 4,
            },
        );
        assert!(dre.is_partitioned_on(&keys));
        assert!(!dre.is_partitioned_on(&[col("other")]));
    }

    #[test]
    fn distributed_rel_expr_is_single_partition() {
        let plan = crate::algebra::RelExpr::scan("small");
        let dre = DistributedRelExpr::new(plan).with_distribution(
            DataDistribution::SinglePartition { node: NodeId(0) },
        );
        assert!(dre.is_single_partition());
        assert!(!dre.is_replicated());
    }

    // --- check_join_compatibility ---

    #[test]
    fn compat_replicated_left() {
        let left = DataDistribution::Replicated;
        let right = DataDistribution::Arbitrary;
        let result = check_join_compatibility(
            &left,
            &right,
            &[col("id")],
            &[col("id")],
        );
        assert_eq!(result, DistributionCompatibility::Compatible);
    }

    #[test]
    fn compat_replicated_right() {
        let left = DataDistribution::Arbitrary;
        let right = DataDistribution::Replicated;
        let result = check_join_compatibility(
            &left,
            &right,
            &[col("id")],
            &[col("id")],
        );
        assert_eq!(result, DistributionCompatibility::Compatible);
    }

    #[test]
    fn compat_hash_colocated() {
        let keys_l = vec![col("id")];
        let keys_r = vec![col("id")];
        let left = DataDistribution::HashPartitioned {
            keys: keys_l.clone(),
            partition_count: 8,
        };
        let right = DataDistribution::HashPartitioned {
            keys: keys_r.clone(),
            partition_count: 8,
        };
        let result = check_join_compatibility(
            &left, &right, &keys_l, &keys_r,
        );
        assert_eq!(result, DistributionCompatibility::Compatible);
    }

    #[test]
    fn compat_different_partition_count() {
        let keys = vec![col("id")];
        let left = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 8,
        };
        let right = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 16,
        };
        let result = check_join_compatibility(
            &left, &right, &keys, &keys,
        );
        assert_eq!(
            result,
            DistributionCompatibility::RightMustRedistribute
        );
    }

    #[test]
    fn compat_left_partitioned_right_not() {
        let keys = vec![col("id")];
        let left = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 8,
        };
        let right = DataDistribution::Arbitrary;
        let result = check_join_compatibility(
            &left, &right, &keys, &keys,
        );
        assert_eq!(
            result,
            DistributionCompatibility::RightMustRedistribute
        );
    }

    #[test]
    fn compat_right_partitioned_left_not() {
        let keys = vec![col("id")];
        let left = DataDistribution::Arbitrary;
        let right = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 8,
        };
        let result = check_join_compatibility(
            &left, &right, &keys, &keys,
        );
        assert_eq!(
            result,
            DistributionCompatibility::LeftMustRedistribute
        );
    }

    #[test]
    fn compat_both_arbitrary() {
        let left = DataDistribution::Arbitrary;
        let right = DataDistribution::Arbitrary;
        let result = check_join_compatibility(
            &left,
            &right,
            &[col("id")],
            &[col("id")],
        );
        assert_eq!(
            result,
            DistributionCompatibility::BothMustRedistribute
        );
    }

    #[test]
    fn compat_different_keys() {
        let left = DataDistribution::HashPartitioned {
            keys: vec![col("a")],
            partition_count: 8,
        };
        let right = DataDistribution::HashPartitioned {
            keys: vec![col("b")],
            partition_count: 8,
        };
        let result = check_join_compatibility(
            &left,
            &right,
            &[col("x")],
            &[col("y")],
        );
        assert_eq!(
            result,
            DistributionCompatibility::BothMustRedistribute
        );
    }

    // --- should_broadcast ---

    #[test]
    fn broadcast_small_table() {
        assert!(should_broadcast(
            1_000_000,    // 1 MB small
            10_000_000_000, // 10 GB large
            10,
            100_000_000,  // 100 MB threshold
        ));
    }

    #[test]
    fn broadcast_too_large() {
        assert!(!should_broadcast(
            200_000_000,  // 200 MB - exceeds threshold
            10_000_000_000,
            10,
            100_000_000,  // 100 MB threshold
        ));
    }

    #[test]
    fn broadcast_zero_nodes() {
        assert!(!should_broadcast(1000, 1_000_000, 0, 100_000_000));
    }

    #[test]
    fn broadcast_cost_exceeds_shuffle() {
        // When small * nodes > (small + large) * (nodes-1)/nodes,
        // shuffle is cheaper.
        assert!(!should_broadcast(
            500_000_000,  // 500 MB
            600_000_000,  // 600 MB
            100,
            1_000_000_000, // 1 GB threshold
        ));
    }

    #[test]
    fn broadcast_single_node() {
        // With 1 node, shuffle fraction = 0, broadcast cost = small.
        // broadcast_cost = 1000 * 1 = 1000
        // shuffle_cost = (1000 + 1_000_000) * 0 / 1 = 0
        // So broadcast is not cheaper.
        assert!(!should_broadcast(1000, 1_000_000, 1, 100_000_000));
    }

    // --- select_join_strategy ---

    #[test]
    fn strategy_select_colocated() {
        let keys = vec![col("id")];
        let left = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 8,
        };
        let right = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 8,
        };
        let ns = nodes(8);
        let strategy = select_join_strategy(&JoinStrategyInput {
            left_dist: &left,
            right_dist: &right,
            left_bytes: 1_000_000,
            right_bytes: 1_000_000,
            join_keys_left: &keys,
            join_keys_right: &keys,
            nodes: &ns,
            broadcast_threshold: 100_000_000,
        });
        assert_eq!(strategy.label(), "PartitionWise");
    }

    #[test]
    fn strategy_select_broadcast() {
        let left = DataDistribution::Arbitrary;
        let right = DataDistribution::Arbitrary;
        let ns = nodes(4);
        let keys = [col("id")];
        let strategy = select_join_strategy(&JoinStrategyInput {
            left_dist: &left,
            right_dist: &right,
            left_bytes: 10_000_000_000,
            right_bytes: 1_000_000,
            join_keys_left: &keys,
            join_keys_right: &keys,
            nodes: &ns,
            broadcast_threshold: 100_000_000,
        });
        assert_eq!(strategy.label(), "Broadcast");
    }

    #[test]
    fn strategy_select_shuffle() {
        let left = DataDistribution::Arbitrary;
        let right = DataDistribution::Arbitrary;
        let ns = nodes(8);
        let keys = [col("id")];
        let strategy = select_join_strategy(&JoinStrategyInput {
            left_dist: &left,
            right_dist: &right,
            left_bytes: 10_000_000_000,
            right_bytes: 10_000_000_000,
            join_keys_left: &keys,
            join_keys_right: &keys,
            nodes: &ns,
            broadcast_threshold: 100_000_000,
        });
        assert_eq!(strategy.label(), "Shuffle");
    }

    #[test]
    fn strategy_replicated_right() {
        let left = DataDistribution::Arbitrary;
        let right = DataDistribution::Replicated;
        let ns = nodes(4);
        let keys = [col("id")];
        let strategy = select_join_strategy(&JoinStrategyInput {
            left_dist: &left,
            right_dist: &right,
            left_bytes: 1_000_000_000,
            right_bytes: 100_000,
            join_keys_left: &keys,
            join_keys_right: &keys,
            nodes: &ns,
            broadcast_threshold: 100_000_000,
        });
        assert_eq!(strategy.label(), "CoLocated");
    }

    // --- Serialization ---

    #[test]
    fn serialize_distribution_strategy() {
        let s = DistributionStrategy::Broadcast {
            source: NodeId(0),
            targets: vec![NodeId(1), NodeId(2)],
        };
        let json = serde_json::to_string(&s)
            .expect("serialization should succeed");
        let deserialized: DistributionStrategy =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(s, deserialized);
    }

    #[test]
    fn serialize_data_distribution() {
        let d = DataDistribution::HashPartitioned {
            keys: vec![col("id")],
            partition_count: 16,
        };
        let json = serde_json::to_string(&d)
            .expect("serialization should succeed");
        let deserialized: DataDistribution =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(d, deserialized);
    }

    #[test]
    fn serialize_distributed_rel_expr() {
        let plan = crate::algebra::RelExpr::scan("t");
        let dre = DistributedRelExpr::new(plan)
            .with_distribution(DataDistribution::Replicated)
            .on_node(NodeId(0));
        let json = serde_json::to_string(&dre)
            .expect("serialization should succeed");
        let deserialized: DistributedRelExpr =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(dre, deserialized);
    }

    // --- Additional edge cases ---

    #[test]
    fn broadcast_two_nodes() {
        // With 2 nodes: broadcast = small*2, shuffle = total*0.5
        // small=100, large=10000
        // broadcast = 200, shuffle = 10100*0.5 = 5050
        // broadcast cheaper
        assert!(should_broadcast(100, 10_000, 2, 100_000));
    }

    #[test]
    fn compat_single_partition_left() {
        let left = DataDistribution::SinglePartition {
            node: NodeId(0),
        };
        let right = DataDistribution::Arbitrary;
        let result = check_join_compatibility(
            &left,
            &right,
            &[col("id")],
            &[col("id")],
        );
        assert_eq!(
            result,
            DistributionCompatibility::BothMustRedistribute
        );
    }

    #[test]
    fn strategy_label_all_variants() {
        let labels = [
            DistributionStrategy::Broadcast {
                source: NodeId(0),
                targets: vec![],
            },
            DistributionStrategy::Shuffle {
                source: NodeId(0),
                targets: vec![],
                partition_keys: vec![],
            },
            DistributionStrategy::CoLocated,
            DistributionStrategy::PartitionWise {
                partition_key: col("k"),
            },
            DistributionStrategy::RangePartition {
                partition_key: col("k"),
                ranges: vec![],
            },
            DistributionStrategy::PartialBroadcast {
                source: NodeId(0),
                targets: vec![],
                predicate: Expr::Const(Const::Bool(true)),
            },
        ];
        let expected = [
            "Broadcast",
            "Shuffle",
            "CoLocated",
            "PartitionWise",
            "RangePartition",
            "PartialBroadcast",
        ];
        for (s, e) in labels.iter().zip(expected.iter()) {
            assert_eq!(s.label(), *e);
        }
    }

    #[test]
    fn range_boundary_fields() {
        let b = RangeBoundary {
            value: "2024-01-01".into(),
            inclusive: true,
        };
        assert_eq!(b.value, "2024-01-01");
        assert!(b.inclusive);
    }

    #[test]
    fn distributed_rel_expr_builder_chain() {
        let plan = crate::algebra::RelExpr::scan("t");
        let dre = DistributedRelExpr::new(plan)
            .with_distribution(DataDistribution::HashPartitioned {
                keys: vec![col("id")],
                partition_count: 4,
            })
            .on_node(NodeId(1))
            .with_strategy(DistributionStrategy::Shuffle {
                source: NodeId(0),
                targets: nodes(4),
                partition_keys: vec![col("id")],
            });
        assert!(dre.is_partitioned_on(&[col("id")]));
        assert_eq!(dre.node_assignment, Some(NodeId(1)));
        assert!(dre.input_strategy.is_some());
    }
}
