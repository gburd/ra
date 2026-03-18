//! Distributed query optimizer.
//!
//! Selects optimal data distribution strategies for operators in a
//! distributed query plan. Given a logical plan and cluster topology,
//! the optimizer annotates each operator with the cheapest
//! redistribution strategy by considering network transfer costs,
//! co-location opportunities, and broadcast thresholds.

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::cost::Cost;
use ra_core::distribution::{
    check_join_compatibility, DataDistribution, DistributedRelExpr,
    DistributionCompatibility, DistributionStrategy, NodeId,
};
use ra_core::expr::Expr;
use ra_core::statistics::Statistics;

use crate::network_cost::{
    self, JoinSides, NetworkCostModel,
};

/// Errors from the distributed optimizer.
#[derive(Debug, thiserror::Error)]
pub enum DistributedOptimizerError {
    /// No nodes configured in the cluster.
    #[error("cluster has no nodes")]
    EmptyCluster,

    /// Table not found in the catalog.
    #[error("table not found: {0}")]
    TableNotFound(String),

    /// Strategy enumeration produced no candidates.
    #[error("no valid strategy for operator")]
    NoStrategy,
}

/// Configuration for the distributed optimizer.
#[derive(Debug, Clone)]
pub struct DistributedOptimizerConfig {
    /// Maximum size (bytes) for a broadcast candidate.
    pub broadcast_threshold: u64,
    /// Weight for network cost relative to CPU cost.
    pub network_weight: f64,
    /// Weight for monetary cost (cloud billing).
    pub monetary_weight: f64,
    /// Default row width when statistics are unavailable.
    pub default_row_width: u64,
    /// Whether to consider partial broadcast strategies.
    pub enable_partial_broadcast: bool,
    /// Whether to consider range partition strategies.
    pub enable_range_partition: bool,
    /// Skew threshold: ratio above which a key is considered skewed.
    pub skew_threshold: f64,
}

impl Default for DistributedOptimizerConfig {
    fn default() -> Self {
        Self {
            broadcast_threshold: 100 * 1024 * 1024, // 100 MB
            network_weight: 2.0,
            monetary_weight: 1.0,
            default_row_width: 128,
            enable_partial_broadcast: true,
            enable_range_partition: true,
            skew_threshold: 10.0,
        }
    }
}

/// Cluster topology for the optimizer.
#[derive(Debug, Clone)]
pub struct ClusterTopology {
    /// All nodes in the cluster.
    pub nodes: Vec<NodeId>,
    /// Bandwidth between node pairs in bytes/sec.
    pub bandwidth: HashMap<(NodeId, NodeId), u64>,
    /// Latency between node pairs in microseconds.
    pub latency_us: HashMap<(NodeId, NodeId), u64>,
    /// Table-to-node assignment (which node holds a table).
    pub table_locations: HashMap<String, NodeId>,
    /// Table distribution metadata.
    pub table_distributions: HashMap<String, DataDistribution>,
}

impl ClusterTopology {
    /// Create a simple uniform cluster with N nodes.
    #[must_use]
    pub fn uniform(num_nodes: u32) -> Self {
        let nodes: Vec<NodeId> =
            (0..num_nodes).map(NodeId).collect();
        let mut bandwidth = HashMap::new();
        let mut latency_us = HashMap::new();

        for &a in &nodes {
            for &b in &nodes {
                if a != b {
                    bandwidth.insert((a, b), 1_250_000_000); // 10 Gbps
                    latency_us.insert((a, b), 100); // 100 us
                }
            }
        }

        Self {
            nodes,
            bandwidth,
            latency_us,
            table_locations: HashMap::new(),
            table_distributions: HashMap::new(),
        }
    }

    /// Register a table's location and distribution.
    pub fn register_table(
        &mut self,
        table: &str,
        node: NodeId,
        distribution: DataDistribution,
    ) {
        self.table_locations
            .insert(table.to_owned(), node);
        self.table_distributions
            .insert(table.to_owned(), distribution);
    }

    /// Get the distribution for a table.
    #[must_use]
    pub fn table_distribution(
        &self,
        table: &str,
    ) -> DataDistribution {
        self.table_distributions
            .get(table)
            .cloned()
            .unwrap_or(DataDistribution::Arbitrary)
    }

    /// Estimate transfer time in milliseconds.
    #[must_use]
    pub fn transfer_time_ms(
        &self,
        from: NodeId,
        to: NodeId,
        bytes: u64,
    ) -> f64 {
        if from == to {
            return 0.0;
        }
        let bw = self
            .bandwidth
            .get(&(from, to))
            .copied()
            .unwrap_or(1_000_000_000); // default 1 Gbps
        let lat = self
            .latency_us
            .get(&(from, to))
            .copied()
            .unwrap_or(1000); // default 1 ms

        let latency_ms = lat as f64 / 1000.0;
        let transfer_ms = bytes as f64 / bw as f64 * 1000.0;
        latency_ms + transfer_ms
    }
}

/// Convert a `ra_core::distribution::DistributionStrategy` to the
/// `network_cost` module's simpler `DistributionStrategy` for costing.
///
/// Strategies with no direct mapping (`PartitionWise`, `RangePartition`,
/// `PartialBroadcast`) return `None` because they don't participate
/// in the network cost model's cost calculation.
fn to_network_strategy(
    strategy: &DistributionStrategy,
) -> Option<network_cost::DistributionStrategy> {
    match strategy {
        DistributionStrategy::Broadcast { source, targets } => {
            Some(network_cost::DistributionStrategy::Broadcast {
                source: ra_hardware::network::NodeId(source.0),
                targets: targets
                    .iter()
                    .map(|n| ra_hardware::network::NodeId(n.0))
                    .collect(),
            })
        }
        DistributionStrategy::Shuffle {
            source, targets, ..
        } => {
            Some(network_cost::DistributionStrategy::Shuffle {
                source: ra_hardware::network::NodeId(source.0),
                targets: targets
                    .iter()
                    .map(|n| ra_hardware::network::NodeId(n.0))
                    .collect(),
            })
        }
        DistributionStrategy::CoLocated
        | DistributionStrategy::PartitionWise { .. } => {
            Some(network_cost::DistributionStrategy::CoLocated)
        }
        DistributionStrategy::PartialBroadcast {
            source, targets, ..
        } => {
            Some(network_cost::DistributionStrategy::Broadcast {
                source: ra_hardware::network::NodeId(source.0),
                targets: targets
                    .iter()
                    .map(|n| ra_hardware::network::NodeId(n.0))
                    .collect(),
            })
        }
        DistributionStrategy::RangePartition { .. } => None,
    }
}

/// Convert a `network_cost::DistributionStrategy` back to a
/// `ra_core::distribution::DistributionStrategy`.
///
/// This is used when the network cost model recommends a strategy
/// and we need to include it as a candidate in the optimizer's
/// enumeration.
fn from_network_strategy(
    strategy: &network_cost::DistributionStrategy,
    nodes: &[NodeId],
) -> Option<DistributionStrategy> {
    match strategy {
        network_cost::DistributionStrategy::Broadcast {
            source,
            targets,
        } => Some(DistributionStrategy::Broadcast {
            source: NodeId(source.0),
            targets: targets
                .iter()
                .map(|n| NodeId(n.0))
                .collect(),
        }),
        network_cost::DistributionStrategy::Shuffle {
            source,
            targets,
        } => {
            // We don't have join keys from the network model's
            // recommendation, so use an empty key list. The
            // caller's existing Shuffle strategy with keys will
            // typically be preferred if keys are available.
            Some(DistributionStrategy::Shuffle {
                source: NodeId(source.0),
                targets: targets
                    .iter()
                    .map(|n| NodeId(n.0))
                    .collect(),
                partition_keys: Vec::new(),
            })
        }
        network_cost::DistributionStrategy::CoLocated => {
            // Only add if we don't already have CoLocated.
            if nodes.len() > 1 {
                Some(DistributionStrategy::CoLocated)
            } else {
                None
            }
        }
    }
}

/// Distributed query optimizer.
///
/// Takes a logical plan and cluster topology, and annotates each
/// operator with the cheapest distribution strategy.
///
/// When a [`NetworkCostModel`] is provided via [`with_network_cost`],
/// the optimizer uses topology-aware transfer times and billing
/// costs instead of simple heuristic estimates.
///
/// [`with_network_cost`]: DistributedOptimizer::with_network_cost
#[derive(Debug)]
pub struct DistributedOptimizer {
    config: DistributedOptimizerConfig,
    topology: ClusterTopology,
    table_stats: HashMap<String, Statistics>,
    network_cost: Option<NetworkCostModel>,
}

impl DistributedOptimizer {
    /// Create a new distributed optimizer.
    #[must_use]
    pub fn new(
        config: DistributedOptimizerConfig,
        topology: ClusterTopology,
    ) -> Self {
        Self {
            config,
            topology,
            table_stats: HashMap::new(),
            network_cost: None,
        }
    }

    /// Attach a network cost model for topology-aware costing.
    ///
    /// When set, `cost_strategy` uses real network transfer times
    /// and billing costs from the topology instead of simple
    /// heuristic estimates. Strategy enumeration also consults
    /// `recommend_join_strategy` from the network cost model.
    #[must_use]
    pub fn with_network_cost(
        mut self,
        model: NetworkCostModel,
    ) -> Self {
        self.network_cost = Some(model);
        self
    }

    /// Get a reference to the attached network cost model, if any.
    #[must_use]
    pub fn network_cost(&self) -> Option<&NetworkCostModel> {
        self.network_cost.as_ref()
    }

    /// Get a reference to the cluster topology.
    #[must_use]
    pub fn topology(&self) -> &ClusterTopology {
        &self.topology
    }

    /// Register statistics for a table.
    pub fn register_stats(
        &mut self,
        table: &str,
        stats: Statistics,
    ) {
        self.table_stats.insert(table.to_owned(), stats);
    }

    /// Optimize the distribution for a complete plan.
    ///
    /// Returns a distributed plan annotated with strategies.
    ///
    /// # Errors
    ///
    /// Returns an error if the cluster has no nodes or no valid
    /// strategy can be found for an operator.
    pub fn optimize_distribution(
        &self,
        plan: &RelExpr,
    ) -> Result<DistributedRelExpr, DistributedOptimizerError> {
        if self.topology.nodes.is_empty() {
            return Err(DistributedOptimizerError::EmptyCluster);
        }
        self.annotate(plan)
    }

    /// Recursively annotate a plan with distribution info.
    fn annotate(
        &self,
        plan: &RelExpr,
    ) -> Result<DistributedRelExpr, DistributedOptimizerError> {
        match plan {
            RelExpr::Scan { table, .. } => {
                self.annotate_scan(table, plan)
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => self.annotate_join(
                *join_type, condition, left, right, plan,
            ),
            RelExpr::Filter { input, .. } => {
                let child = self.annotate(input)?;
                Ok(DistributedRelExpr {
                    plan: plan.clone(),
                    distribution: child.distribution,
                    node_assignment: child.node_assignment,
                    input_strategy: None,
                })
            }
            RelExpr::Project { input, .. } => {
                let child = self.annotate(input)?;
                Ok(DistributedRelExpr {
                    plan: plan.clone(),
                    distribution: child.distribution,
                    node_assignment: child.node_assignment,
                    input_strategy: None,
                })
            }
            RelExpr::Aggregate { input, group_by, .. } => {
                self.annotate_aggregate(input, group_by, plan)
            }
            _ => {
                // For other operators, use arbitrary distribution.
                Ok(DistributedRelExpr::new(plan.clone()))
            }
        }
    }

    /// Annotate a scan node.
    fn annotate_scan(
        &self,
        table: &str,
        plan: &RelExpr,
    ) -> Result<DistributedRelExpr, DistributedOptimizerError> {
        let dist = self.topology.table_distribution(table);
        let node = self.topology.table_locations.get(table).copied();
        Ok(DistributedRelExpr {
            plan: plan.clone(),
            distribution: dist,
            node_assignment: node,
            input_strategy: None,
        })
    }

    /// Annotate a join node with the best distribution strategy.
    fn annotate_join(
        &self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        plan: &RelExpr,
    ) -> Result<DistributedRelExpr, DistributedOptimizerError> {
        let left_ann = self.annotate(left)?;
        let right_ann = self.annotate(right)?;

        let join_keys = extract_equi_join_keys(condition);
        let (keys_l, keys_r): (Vec<Expr>, Vec<Expr>) =
            join_keys.into_iter().unzip();

        let left_bytes = self.estimate_bytes(left);
        let right_bytes = self.estimate_bytes(right);

        // Enumerate candidate strategies.
        let candidates = self.enumerate_strategies(
            &left_ann.distribution,
            &right_ann.distribution,
            left_bytes,
            right_bytes,
            &keys_l,
            &keys_r,
            join_type,
        );

        // Cost each candidate and pick the cheapest.
        let best = candidates
            .into_iter()
            .map(|s| {
                let cost = self.cost_strategy(
                    &s, left_bytes, right_bytes,
                );
                (s, cost)
            })
            .min_by(|(_, a), (_, b)| {
                a.total().partial_cmp(&b.total()).unwrap_or(
                    std::cmp::Ordering::Equal,
                )
            })
            .map(|(s, _)| s)
            .unwrap_or(DistributionStrategy::CoLocated);

        // Determine output distribution.
        let output_dist = match &best {
            DistributionStrategy::Shuffle {
                partition_keys, ..
            } => DataDistribution::HashPartitioned {
                keys: partition_keys.clone(),
                partition_count: self.topology.nodes.len() as u32,
            },
            DistributionStrategy::CoLocated
            | DistributionStrategy::PartitionWise { .. } => {
                left_ann.distribution.clone()
            }
            DistributionStrategy::Broadcast { .. }
            | DistributionStrategy::PartialBroadcast { .. } => {
                // After broadcast, data is effectively replicated
                // where the large side sits.
                left_ann.distribution.clone()
            }
            DistributionStrategy::RangePartition {
                partition_key,
                ranges,
            } => DataDistribution::HashPartitioned {
                keys: vec![partition_key.clone()],
                partition_count: ranges.len() as u32,
            },
        };

        Ok(DistributedRelExpr {
            plan: plan.clone(),
            distribution: output_dist,
            node_assignment: None,
            input_strategy: Some(best),
        })
    }

    /// Annotate an aggregate with distribution info.
    fn annotate_aggregate(
        &self,
        input: &RelExpr,
        group_by: &[Expr],
        plan: &RelExpr,
    ) -> Result<DistributedRelExpr, DistributedOptimizerError> {
        let child = self.annotate(input)?;

        // If already partitioned on group-by keys, partition-wise.
        if child.is_partitioned_on(group_by) {
            return Ok(DistributedRelExpr {
                plan: plan.clone(),
                distribution: child.distribution,
                node_assignment: child.node_assignment,
                input_strategy: Some(
                    DistributionStrategy::PartitionWise {
                        partition_key: group_by
                            .first()
                            .cloned()
                            .unwrap_or(Expr::Const(
                                ra_core::expr::Const::Null,
                            )),
                    },
                ),
            });
        }

        // Otherwise, shuffle by group-by keys.
        if !group_by.is_empty() {
            let source = self
                .topology
                .nodes
                .first()
                .copied()
                .unwrap_or(NodeId(0));
            return Ok(DistributedRelExpr {
                plan: plan.clone(),
                distribution: DataDistribution::HashPartitioned {
                    keys: group_by.to_vec(),
                    partition_count: self.topology.nodes.len()
                        as u32,
                },
                node_assignment: None,
                input_strategy: Some(
                    DistributionStrategy::Shuffle {
                        source,
                        targets: self.topology.nodes.clone(),
                        partition_keys: group_by.to_vec(),
                    },
                ),
            });
        }

        // Global aggregate: gather to one node.
        let target =
            self.topology.nodes.first().copied().unwrap_or(NodeId(0));
        Ok(DistributedRelExpr {
            plan: plan.clone(),
            distribution: DataDistribution::SinglePartition {
                node: target,
            },
            node_assignment: Some(target),
            input_strategy: None,
        })
    }

    /// Enumerate candidate distribution strategies for a join.
    ///
    /// When a [`NetworkCostModel`] is attached, the network model's
    /// `recommend_join_strategy` is also consulted and its result
    /// is included as an additional candidate (converted to
    /// `ra_core`'s `DistributionStrategy`).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    pub fn enumerate_strategies(
        &self,
        left_dist: &DataDistribution,
        right_dist: &DataDistribution,
        left_bytes: u64,
        right_bytes: u64,
        join_keys_left: &[Expr],
        join_keys_right: &[Expr],
        join_type: JoinType,
    ) -> Vec<DistributionStrategy> {
        let mut strategies = Vec::new();
        let nodes = &self.topology.nodes;

        if nodes.is_empty() {
            return strategies;
        }

        let compat = check_join_compatibility(
            left_dist,
            right_dist,
            join_keys_left,
            join_keys_right,
        );

        // Strategy 1: Co-located / Partition-wise.
        if compat == DistributionCompatibility::Compatible {
            if let DataDistribution::HashPartitioned {
                keys, ..
            } = left_dist
            {
                if let Some(key) = keys.first() {
                    strategies.push(
                        DistributionStrategy::PartitionWise {
                            partition_key: key.clone(),
                        },
                    );
                }
            } else {
                strategies.push(DistributionStrategy::CoLocated);
            }
        }

        // Strategy 2: Broadcast right side.
        if right_bytes < self.config.broadcast_threshold
            && is_broadcast_compatible(join_type, false)
        {
            let source =
                nodes.first().copied().unwrap_or(NodeId(0));
            strategies.push(DistributionStrategy::Broadcast {
                source,
                targets: nodes.clone(),
            });
        }

        // Strategy 3: Broadcast left side.
        if left_bytes < self.config.broadcast_threshold
            && is_broadcast_compatible(join_type, true)
        {
            let source =
                nodes.first().copied().unwrap_or(NodeId(0));
            strategies.push(DistributionStrategy::Broadcast {
                source,
                targets: nodes.clone(),
            });
        }

        // Strategy 4: Shuffle both sides.
        let source = nodes.first().copied().unwrap_or(NodeId(0));
        let keys = if !join_keys_left.is_empty() {
            join_keys_left.to_vec()
        } else {
            join_keys_right.to_vec()
        };
        if !keys.is_empty() {
            strategies.push(DistributionStrategy::Shuffle {
                source,
                targets: nodes.clone(),
                partition_keys: keys,
            });
        }

        // Strategy 5: Range partition (if enabled).
        if self.config.enable_range_partition
            && !join_keys_left.is_empty()
        {
            let key = join_keys_left[0].clone();
            let range_count = nodes.len().min(32);
            let ranges: Vec<(String, String)> = (0..range_count)
                .map(|i| {
                    (i.to_string(), (i + 1).to_string())
                })
                .collect();
            strategies.push(
                DistributionStrategy::RangePartition {
                    partition_key: key,
                    ranges,
                },
            );
        }

        // Strategy 6: Network cost model recommendation.
        if let Some(ncm) = &self.network_cost {
            let row_width =
                self.config.default_row_width.max(1) as usize;
            let left_rows = left_bytes / row_width as u64;
            let right_rows = right_bytes / row_width as u64;

            let left_node = nodes
                .first()
                .copied()
                .unwrap_or(NodeId(0));
            let right_node = nodes
                .last()
                .copied()
                .unwrap_or(NodeId(0));

            let hw_nodes: Vec<ra_hardware::network::NodeId> =
                nodes
                    .iter()
                    .map(|n| ra_hardware::network::NodeId(n.0))
                    .collect();

            let sides = JoinSides {
                left_node: ra_hardware::network::NodeId(
                    left_node.0,
                ),
                right_node: ra_hardware::network::NodeId(
                    right_node.0,
                ),
                left_rows,
                right_rows,
                row_width,
            };

            let recommended = ncm.recommend_join_strategy(
                &sides,
                &hw_nodes,
                self.config.broadcast_threshold,
            );

            if let Some(converted) =
                from_network_strategy(&recommended, nodes)
            {
                strategies.push(converted);
            }
        }

        strategies
    }

    /// Cost a distribution strategy.
    ///
    /// When a [`NetworkCostModel`] is attached, Broadcast and Shuffle
    /// costs are computed from real topology transfer times and
    /// billing costs. Otherwise, falls back to heuristic estimates
    /// using the `ClusterTopology` bandwidth/latency tables.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn cost_strategy(
        &self,
        strategy: &DistributionStrategy,
        left_bytes: u64,
        right_bytes: u64,
    ) -> Cost {
        // Try the network cost model for Broadcast/Shuffle/CoLocated
        if let Some(ncm) = &self.network_cost {
            if let Some(net_strat) = to_network_strategy(strategy) {
                return self.cost_via_network_model(
                    ncm,
                    &net_strat,
                    strategy,
                    left_bytes,
                    right_bytes,
                );
            }
        }

        self.cost_heuristic(strategy, left_bytes, right_bytes)
    }

    /// Cost using the [`NetworkCostModel`] for topology-aware
    /// transfer time and billing.
    #[allow(clippy::cast_precision_loss)]
    fn cost_via_network_model(
        &self,
        ncm: &NetworkCostModel,
        net_strat: &network_cost::DistributionStrategy,
        orig: &DistributionStrategy,
        left_bytes: u64,
        right_bytes: u64,
    ) -> Cost {
        let row_width =
            self.config.default_row_width.max(1) as usize;

        let input_rows = match orig {
            DistributionStrategy::Broadcast { .. }
            | DistributionStrategy::PartialBroadcast { .. } => {
                let small = left_bytes.min(right_bytes);
                small / row_width as u64
            }
            DistributionStrategy::Shuffle { .. } => {
                (left_bytes + right_bytes) / row_width as u64
            }
            _ => 0,
        };

        let est =
            ncm.distribution_cost(net_strat, input_rows, row_width);

        // Combine: network component from topology model,
        // add CPU hashing cost for shuffle, add monetary weight.
        let network_ms = est.cost.network;
        let monetary = est.monetary_cost;

        let cpu = match orig {
            DistributionStrategy::Shuffle { .. } => {
                (left_bytes + right_bytes) as f64 * 0.001
            }
            _ => 0.0,
        };

        Cost::new(
            cpu,
            0.0,
            network_ms * self.config.network_weight
                + monetary * self.config.monetary_weight,
            est.bytes_transferred,
        )
    }

    /// Heuristic cost when no `NetworkCostModel` is available.
    #[allow(clippy::cast_precision_loss)]
    fn cost_heuristic(
        &self,
        strategy: &DistributionStrategy,
        left_bytes: u64,
        right_bytes: u64,
    ) -> Cost {
        match strategy {
            DistributionStrategy::CoLocated
            | DistributionStrategy::PartitionWise { .. } => {
                Cost::ZERO
            }
            DistributionStrategy::Broadcast { source, targets } => {
                let small_bytes = left_bytes.min(right_bytes);
                let mut total_network = 0.0;
                for &target in targets {
                    total_network += self
                        .topology
                        .transfer_time_ms(
                            *source,
                            target,
                            small_bytes,
                        );
                }
                Cost::new(
                    0.0,
                    0.0,
                    total_network * self.config.network_weight,
                    small_bytes * targets.len() as u64,
                )
            }
            DistributionStrategy::Shuffle {
                source,
                targets,
                ..
            } => {
                let total_bytes = left_bytes + right_bytes;
                let per_node = if targets.is_empty() {
                    0
                } else {
                    total_bytes / targets.len() as u64
                };
                let mut total_network = 0.0;
                for &target in targets {
                    total_network += self
                        .topology
                        .transfer_time_ms(
                            *source, target, per_node,
                        );
                }
                let hash_cpu = total_bytes as f64 * 0.001;
                Cost::new(
                    hash_cpu,
                    0.0,
                    total_network * self.config.network_weight,
                    per_node,
                )
            }
            DistributionStrategy::PartialBroadcast {
                source,
                targets,
                ..
            } => {
                let small_bytes = left_bytes.min(right_bytes);
                let mut total_network = 0.0;
                for &target in targets {
                    total_network += self
                        .topology
                        .transfer_time_ms(
                            *source,
                            target,
                            small_bytes,
                        );
                }
                Cost::new(
                    0.0,
                    0.0,
                    total_network * self.config.network_weight,
                    small_bytes * targets.len() as u64,
                )
            }
            DistributionStrategy::RangePartition {
                ranges, ..
            } => {
                let total_bytes = left_bytes + right_bytes;
                let per_range = if ranges.is_empty() {
                    0
                } else {
                    total_bytes / ranges.len() as u64
                };
                let sort_cpu = total_bytes as f64 * 0.005;
                let network = total_bytes as f64 * 0.001
                    * self.config.network_weight;
                Cost::new(sort_cpu, 0.0, network, per_range)
            }
        }
    }

    /// Estimate total bytes for a relation.
    fn estimate_bytes(&self, plan: &RelExpr) -> u64 {
        match plan {
            RelExpr::Scan { table, .. } => {
                if let Some(stats) = self.table_stats.get(table) {
                    stats.total_size.max(
                        (stats.row_count as u64)
                            .saturating_mul(stats.avg_row_size),
                    )
                } else {
                    1000 * self.config.default_row_width
                }
            }
            RelExpr::Filter { input, .. } => {
                // Assume 10% selectivity.
                self.estimate_bytes(input) / 10
            }
            RelExpr::Project { input, .. } => {
                // Assume projection reduces width by 50%.
                self.estimate_bytes(input) / 2
            }
            RelExpr::Join { left, right, .. } => {
                // Rough estimate: product of inputs * selectivity.
                let l = self.estimate_bytes(left);
                let r = self.estimate_bytes(right);
                (l / 10).saturating_add(r / 10)
            }
            RelExpr::Aggregate { input, .. } => {
                // Aggregation reduces rows significantly.
                self.estimate_bytes(input) / 100
            }
            _ => 1000 * self.config.default_row_width,
        }
    }
}

/// Whether broadcast is compatible with the given join type and
/// which side is being broadcast.
fn is_broadcast_compatible(
    join_type: JoinType,
    broadcast_left: bool,
) -> bool {
    match join_type {
        JoinType::Inner | JoinType::Cross | JoinType::Semi => true,
        JoinType::LeftOuter | JoinType::Anti => !broadcast_left,
        JoinType::RightOuter => broadcast_left,
        JoinType::FullOuter => false,
    }
}

/// Extract equi-join key pairs from a join condition.
///
/// Returns pairs of (left_key, right_key) for equality predicates.
fn extract_equi_join_keys(condition: &Expr) -> Vec<(Expr, Expr)> {
    let mut keys = Vec::new();
    collect_equi_keys(condition, &mut keys);
    keys
}

fn collect_equi_keys(
    expr: &Expr,
    keys: &mut Vec<(Expr, Expr)>,
) {
    match expr {
        Expr::BinOp {
            op: ra_core::expr::BinOp::Eq,
            left,
            right,
        } => {
            keys.push((*left.clone(), *right.clone()));
        }
        Expr::BinOp {
            op: ra_core::expr::BinOp::And,
            left,
            right,
        } => {
            collect_equi_keys(left, keys);
            collect_equi_keys(right, keys);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{AggregateExpr, AggregateFunction};
    use ra_core::expr::{BinOp, ColumnRef, Const};

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef::new(name))
    }

    fn eq(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn and(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::And,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn make_optimizer(
        num_nodes: u32,
    ) -> DistributedOptimizer {
        let config = DistributedOptimizerConfig::default();
        let topology = ClusterTopology::uniform(num_nodes);
        DistributedOptimizer::new(config, topology)
    }

    fn make_optimizer_with_tables(
        num_nodes: u32,
    ) -> DistributedOptimizer {
        let config = DistributedOptimizerConfig::default();
        let mut topology = ClusterTopology::uniform(num_nodes);

        // Register a large orders table.
        let mut orders_stats = Statistics::new(100_000_000.0);
        orders_stats.avg_row_size = 256;
        orders_stats.total_size = 25_600_000_000;

        topology.register_table(
            "orders",
            NodeId(0),
            DataDistribution::HashPartitioned {
                keys: vec![col("order_id")],
                partition_count: num_nodes,
            },
        );

        // Register a small countries table.
        let mut countries_stats = Statistics::new(200.0);
        countries_stats.avg_row_size = 64;
        countries_stats.total_size = 12_800;

        topology.register_table(
            "countries",
            NodeId(0),
            DataDistribution::Replicated,
        );

        // Register a medium customers table.
        let mut customers_stats = Statistics::new(1_000_000.0);
        customers_stats.avg_row_size = 128;
        customers_stats.total_size = 128_000_000;

        topology.register_table(
            "customers",
            NodeId(1),
            DataDistribution::HashPartitioned {
                keys: vec![col("customer_id")],
                partition_count: num_nodes,
            },
        );

        let mut opt =
            DistributedOptimizer::new(config, topology);
        opt.register_stats("orders", orders_stats);
        opt.register_stats("countries", countries_stats);
        opt.register_stats("customers", customers_stats);
        opt
    }

    // --- ClusterTopology ---

    #[test]
    fn uniform_cluster_creation() {
        let t = ClusterTopology::uniform(4);
        assert_eq!(t.nodes.len(), 4);
        assert_eq!(
            t.bandwidth.len(),
            4 * 3 // 4 nodes, each connected to 3 others
        );
    }

    #[test]
    fn register_table_topology() {
        let mut t = ClusterTopology::uniform(4);
        t.register_table(
            "users",
            NodeId(0),
            DataDistribution::Replicated,
        );
        assert_eq!(
            t.table_distribution("users"),
            DataDistribution::Replicated,
        );
        assert_eq!(
            t.table_locations.get("users"),
            Some(&NodeId(0)),
        );
    }

    #[test]
    fn table_distribution_default() {
        let t = ClusterTopology::uniform(4);
        assert_eq!(
            t.table_distribution("nonexistent"),
            DataDistribution::Arbitrary,
        );
    }

    #[test]
    fn transfer_time_same_node() {
        let t = ClusterTopology::uniform(4);
        assert_eq!(
            t.transfer_time_ms(NodeId(0), NodeId(0), 1_000_000),
            0.0,
        );
    }

    #[test]
    fn transfer_time_different_nodes() {
        let t = ClusterTopology::uniform(4);
        let time = t.transfer_time_ms(
            NodeId(0),
            NodeId(1),
            1_250_000_000, // 1.25 GB
        );
        // latency: 100us = 0.1ms
        // transfer: 1.25GB / 1.25GB/s = 1000ms
        // total: 1000.1ms
        assert!((time - 1000.1).abs() < 0.01);
    }

    // --- DistributedOptimizerConfig ---

    #[test]
    fn default_config() {
        let c = DistributedOptimizerConfig::default();
        assert_eq!(c.broadcast_threshold, 100 * 1024 * 1024);
        assert!((c.network_weight - 2.0).abs() < f64::EPSILON);
        assert_eq!(c.default_row_width, 128);
    }

    // --- DistributedOptimizer ---

    #[test]
    fn empty_cluster_error() {
        let opt = make_optimizer(0);
        let plan = RelExpr::scan("t");
        let result = opt.optimize_distribution(&plan);
        assert!(result.is_err());
    }

    #[test]
    fn optimize_scan() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::scan("orders");
        let result = opt.optimize_distribution(&plan);
        assert!(result.is_ok());
        let dre = result.expect("should succeed");
        assert!(dre.input_strategy.is_none());
    }

    #[test]
    fn optimize_scan_replicated() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::scan("countries");
        let dre = opt
            .optimize_distribution(&plan)
            .expect("should succeed");
        assert!(dre.is_replicated());
    }

    #[test]
    fn optimize_filter_preserves_distribution() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::scan("orders").filter(eq(
            col("status"),
            Expr::Const(Const::String("active".into())),
        ));
        let dre = opt
            .optimize_distribution(&plan)
            .expect("should succeed");
        // Filter preserves the input's distribution.
        if let DataDistribution::HashPartitioned { keys, .. } =
            &dre.distribution
        {
            assert_eq!(keys, &[col("order_id")]);
        } else {
            panic!("expected HashPartitioned after filter");
        }
    }

    #[test]
    fn optimize_join_replicated_right() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("country_code"), col("code")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("countries")),
        };
        let dre = opt
            .optimize_distribution(&plan)
            .expect("should succeed");
        // Countries is replicated, so should be co-located.
        let strategy = dre.input_strategy.as_ref()
            .expect("join should have a strategy");
        // The cheapest should be CoLocated or PartitionWise
        // since countries is replicated.
        assert!(
            matches!(
                strategy,
                DistributionStrategy::CoLocated
                    | DistributionStrategy::PartitionWise { .. }
            ),
            "expected CoLocated or PartitionWise, got {strategy:?}"
        );
    }

    #[test]
    fn optimize_join_both_large() {
        let opt = make_optimizer_with_tables(4);
        // Two large tables that are not co-located.
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("customer_id"), col("customer_id")),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };
        let dre = opt
            .optimize_distribution(&plan)
            .expect("should succeed");
        let strategy = dre.input_strategy.as_ref()
            .expect("join should have a strategy");
        // Should pick shuffle or partition-wise, not broadcast.
        assert!(
            !matches!(strategy, DistributionStrategy::Broadcast { .. }),
            "should not broadcast two large tables"
        );
    }

    #[test]
    fn optimize_aggregate_with_group_by() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::Aggregate {
            group_by: vec![col("country_code")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".into()),
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let dre = opt
            .optimize_distribution(&plan)
            .expect("should succeed");
        let strategy = dre.input_strategy.as_ref()
            .expect("aggregate should have a strategy");
        // Should shuffle by group-by key.
        assert_eq!(strategy.label(), "Shuffle");
    }

    #[test]
    fn optimize_global_aggregate() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("total".into()),
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let dre = opt
            .optimize_distribution(&plan)
            .expect("should succeed");
        assert!(dre.is_single_partition());
    }

    // --- enumerate_strategies ---

    #[test]
    fn enumerate_colocated_strategy() {
        let opt = make_optimizer(4);
        let keys = vec![col("id")];
        let left = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 4,
        };
        let right = DataDistribution::HashPartitioned {
            keys: keys.clone(),
            partition_count: 4,
        };
        let strategies = opt.enumerate_strategies(
            &left,
            &right,
            1_000_000_000,
            1_000_000_000,
            &keys,
            &keys,
            JoinType::Inner,
        );
        assert!(
            strategies.iter().any(|s| {
                matches!(s, DistributionStrategy::PartitionWise { .. })
            }),
            "should include PartitionWise"
        );
    }

    #[test]
    fn enumerate_broadcast_strategy() {
        let opt = make_optimizer(4);
        let strategies = opt.enumerate_strategies(
            &DataDistribution::Arbitrary,
            &DataDistribution::Arbitrary,
            10_000_000_000, // 10 GB left
            10_000,         // 10 KB right
            &[col("id")],
            &[col("id")],
            JoinType::Inner,
        );
        assert!(
            strategies.iter().any(|s| {
                matches!(s, DistributionStrategy::Broadcast { .. })
            }),
            "should include Broadcast for small right side"
        );
    }

    #[test]
    fn enumerate_no_broadcast_for_full_outer() {
        let opt = make_optimizer(4);
        let strategies = opt.enumerate_strategies(
            &DataDistribution::Arbitrary,
            &DataDistribution::Arbitrary,
            1_000,
            1_000,
            &[col("id")],
            &[col("id")],
            JoinType::FullOuter,
        );
        assert!(
            !strategies.iter().any(|s| {
                matches!(s, DistributionStrategy::Broadcast { .. })
            }),
            "should not include Broadcast for FULL OUTER"
        );
    }

    #[test]
    fn enumerate_includes_shuffle() {
        let opt = make_optimizer(4);
        let strategies = opt.enumerate_strategies(
            &DataDistribution::Arbitrary,
            &DataDistribution::Arbitrary,
            1_000_000_000,
            1_000_000_000,
            &[col("id")],
            &[col("id")],
            JoinType::Inner,
        );
        assert!(
            strategies.iter().any(|s| {
                matches!(s, DistributionStrategy::Shuffle { .. })
            }),
            "should always include Shuffle"
        );
    }

    #[test]
    fn enumerate_includes_range_partition() {
        let config = DistributedOptimizerConfig {
            enable_range_partition: true,
            ..DistributedOptimizerConfig::default()
        };
        let topology = ClusterTopology::uniform(4);
        let opt = DistributedOptimizer::new(config, topology);
        let strategies = opt.enumerate_strategies(
            &DataDistribution::Arbitrary,
            &DataDistribution::Arbitrary,
            1_000_000,
            1_000_000,
            &[col("id")],
            &[col("id")],
            JoinType::Inner,
        );
        assert!(
            strategies.iter().any(|s| {
                matches!(
                    s,
                    DistributionStrategy::RangePartition { .. }
                )
            }),
            "should include RangePartition when enabled"
        );
    }

    #[test]
    fn enumerate_empty_cluster() {
        let opt = make_optimizer(0);
        let strategies = opt.enumerate_strategies(
            &DataDistribution::Arbitrary,
            &DataDistribution::Arbitrary,
            1000,
            1000,
            &[col("id")],
            &[col("id")],
            JoinType::Inner,
        );
        assert!(strategies.is_empty());
    }

    // --- cost_strategy ---

    #[test]
    fn cost_colocated_is_zero() {
        let opt = make_optimizer(4);
        let cost = opt.cost_strategy(
            &DistributionStrategy::CoLocated,
            1_000_000,
            1_000_000,
        );
        assert_eq!(cost, Cost::ZERO);
    }

    #[test]
    fn cost_partition_wise_is_zero() {
        let opt = make_optimizer(4);
        let cost = opt.cost_strategy(
            &DistributionStrategy::PartitionWise {
                partition_key: col("id"),
            },
            1_000_000,
            1_000_000,
        );
        assert_eq!(cost, Cost::ZERO);
    }

    #[test]
    fn cost_broadcast_nonzero() {
        let opt = make_optimizer(4);
        let cost = opt.cost_strategy(
            &DistributionStrategy::Broadcast {
                source: NodeId(0),
                targets: vec![NodeId(1), NodeId(2), NodeId(3)],
            },
            10_000_000_000,
            1_000_000,
        );
        assert!(cost.network > 0.0);
        assert!(cost.memory > 0);
    }

    #[test]
    fn cost_shuffle_nonzero() {
        let opt = make_optimizer(4);
        let cost = opt.cost_strategy(
            &DistributionStrategy::Shuffle {
                source: NodeId(0),
                targets: vec![
                    NodeId(0),
                    NodeId(1),
                    NodeId(2),
                    NodeId(3),
                ],
                partition_keys: vec![col("id")],
            },
            1_000_000_000,
            1_000_000_000,
        );
        assert!(cost.network > 0.0);
        assert!(cost.cpu > 0.0);
    }

    #[test]
    fn cost_range_partition_nonzero() {
        let opt = make_optimizer(4);
        let cost = opt.cost_strategy(
            &DistributionStrategy::RangePartition {
                partition_key: col("ts"),
                ranges: vec![
                    ("0".into(), "100".into()),
                    ("100".into(), "200".into()),
                ],
            },
            1_000_000,
            1_000_000,
        );
        assert!(cost.cpu > 0.0);
        assert!(cost.network > 0.0);
    }

    // --- extract_equi_join_keys ---

    #[test]
    fn extract_single_equality() {
        let cond = eq(col("a"), col("b"));
        let keys = extract_equi_join_keys(&cond);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, col("a"));
        assert_eq!(keys[0].1, col("b"));
    }

    #[test]
    fn extract_conjunctive_equalities() {
        let cond = and(
            eq(col("a"), col("b")),
            eq(col("c"), col("d")),
        );
        let keys = extract_equi_join_keys(&cond);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn extract_non_equality_ignored() {
        let cond = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(col("a")),
            right: Box::new(col("b")),
        };
        let keys = extract_equi_join_keys(&cond);
        assert!(keys.is_empty());
    }

    // --- is_broadcast_compatible ---

    #[test]
    fn broadcast_compat_inner() {
        assert!(is_broadcast_compatible(JoinType::Inner, true));
        assert!(is_broadcast_compatible(JoinType::Inner, false));
    }

    #[test]
    fn broadcast_compat_left_outer() {
        assert!(!is_broadcast_compatible(
            JoinType::LeftOuter,
            true
        ));
        assert!(is_broadcast_compatible(
            JoinType::LeftOuter,
            false
        ));
    }

    #[test]
    fn broadcast_compat_right_outer() {
        assert!(is_broadcast_compatible(
            JoinType::RightOuter,
            true
        ));
        assert!(!is_broadcast_compatible(
            JoinType::RightOuter,
            false
        ));
    }

    #[test]
    fn broadcast_compat_full_outer() {
        assert!(!is_broadcast_compatible(
            JoinType::FullOuter,
            true
        ));
        assert!(!is_broadcast_compatible(
            JoinType::FullOuter,
            false
        ));
    }

    #[test]
    fn broadcast_compat_semi() {
        assert!(is_broadcast_compatible(JoinType::Semi, true));
        assert!(is_broadcast_compatible(JoinType::Semi, false));
    }

    #[test]
    fn broadcast_compat_anti() {
        assert!(!is_broadcast_compatible(JoinType::Anti, true));
        assert!(is_broadcast_compatible(JoinType::Anti, false));
    }

    #[test]
    fn broadcast_compat_cross() {
        assert!(is_broadcast_compatible(JoinType::Cross, true));
        assert!(is_broadcast_compatible(JoinType::Cross, false));
    }

    // --- estimate_bytes ---

    #[test]
    fn estimate_bytes_unknown_table() {
        let opt = make_optimizer(4);
        let plan = RelExpr::scan("unknown");
        let bytes = opt.estimate_bytes(&plan);
        // default: 1000 * 128 = 128000
        assert_eq!(bytes, 128_000);
    }

    #[test]
    fn estimate_bytes_known_table() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::scan("orders");
        let bytes = opt.estimate_bytes(&plan);
        assert_eq!(bytes, 25_600_000_000);
    }

    #[test]
    fn estimate_bytes_filter() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::scan("orders").filter(
            Expr::Const(Const::Bool(true)),
        );
        let bytes = opt.estimate_bytes(&plan);
        // 25_600_000_000 / 10 = 2_560_000_000
        assert_eq!(bytes, 2_560_000_000);
    }

    // --- Full plan optimization ---

    #[test]
    fn optimize_nested_join_filter() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("customer_id"), col("id")),
            left: Box::new(
                RelExpr::scan("orders").filter(eq(
                    col("status"),
                    Expr::Const(Const::String("active".into())),
                )),
            ),
            right: Box::new(RelExpr::scan("customers")),
        };
        let result = opt.optimize_distribution(&plan);
        assert!(result.is_ok());
        let dre = result.expect("should succeed");
        assert!(dre.input_strategy.is_some());
    }

    #[test]
    fn optimize_project_preserves_distribution() {
        let opt = make_optimizer_with_tables(4);
        let plan = RelExpr::Project {
            columns: vec![],
            input: Box::new(RelExpr::scan("orders")),
        };
        let dre = opt
            .optimize_distribution(&plan)
            .expect("should succeed");
        if let DataDistribution::HashPartitioned { .. } =
            &dre.distribution
        {
            // OK - preserved.
        } else {
            panic!("project should preserve distribution");
        }
    }
}
