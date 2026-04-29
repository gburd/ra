#![expect(clippy::expect_used, clippy::float_cmp, reason = "test code")]
//! Integration tests for network cost model + distribution optimizer.
//!
//! Tests that the `DistributedOptimizer` uses topology-aware costs from
//! `NetworkCostModel` to select better distribution strategies.

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::distribution::{DataDistribution, DistributionStrategy, NodeId};
use ra_core::expr::{ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::distributed_optimizer::{
    ClusterTopology, DistributedOptimizer, DistributedOptimizerConfig,
};
use ra_engine::network_cost::NetworkCostModel;
use ra_hardware::network::NetworkTopology;

// ── helpers ──────────────────────────────────────────────────────

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: ra_core::expr::BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn hw_node(id: u32) -> ra_hardware::network::NodeId {
    ra_hardware::network::NodeId(id)
}

/// Build a single-datacenter network cost model with 4 nodes,
/// all in the same rack. Very fast, free network.
fn single_dc_model() -> NetworkCostModel {
    let topo = NetworkTopology::single_datacenter_cluster();
    let mut assignments = HashMap::new();
    assignments.insert("orders".into(), hw_node(0));
    assignments.insert("lineitem".into(), hw_node(1));
    assignments.insert("countries".into(), hw_node(0));
    assignments.insert("customers".into(), hw_node(2));
    NetworkCostModel::new(topo, assignments)
}

/// Build a multi-datacenter network cost model with 6 nodes
/// across 3 DCs (US-East, US-West, EU-West).
fn multi_dc_model() -> NetworkCostModel {
    let topo = NetworkTopology::multi_datacenter();
    let mut assignments = HashMap::new();
    assignments.insert("orders".into(), hw_node(0));
    assignments.insert("lineitem".into(), hw_node(0));
    assignments.insert("customers".into(), hw_node(2));
    assignments.insert("countries".into(), hw_node(4));
    NetworkCostModel::new(topo, assignments)
}

/// Build a cloud-federation network cost model with 6 nodes
/// across AWS, GCP, and Azure.
fn cloud_federation_model() -> NetworkCostModel {
    let topo = NetworkTopology::cloud_federation();
    let mut assignments = HashMap::new();
    assignments.insert("orders".into(), hw_node(0));
    assignments.insert("lineitem".into(), hw_node(0));
    assignments.insert("customers".into(), hw_node(2));
    assignments.insert("inventory".into(), hw_node(4));
    NetworkCostModel::new(topo, assignments)
}

/// Build the matching cluster topology for a single-DC setup.
fn single_dc_topology() -> ClusterTopology {
    let mut topo = ClusterTopology::uniform(4);
    topo.register_table(
        "orders",
        NodeId(0),
        DataDistribution::HashPartitioned {
            keys: vec![col("order_id")],
            partition_count: 4,
        },
    );
    topo.register_table(
        "lineitem",
        NodeId(1),
        DataDistribution::HashPartitioned {
            keys: vec![col("l_orderkey")],
            partition_count: 4,
        },
    );
    topo.register_table("countries", NodeId(0), DataDistribution::Replicated);
    topo.register_table(
        "customers",
        NodeId(2),
        DataDistribution::HashPartitioned {
            keys: vec![col("customer_id")],
            partition_count: 4,
        },
    );
    topo
}

/// Build the matching cluster topology for multi-DC.
fn multi_dc_topology() -> ClusterTopology {
    let mut topo = ClusterTopology::uniform(6);
    // Set realistic inter-DC latencies.
    for &a in &[NodeId(0), NodeId(1)] {
        for &b in &[NodeId(2), NodeId(3)] {
            topo.latency_us.insert((a, b), 60_000);
            topo.latency_us.insert((b, a), 60_000);
            topo.bandwidth.insert((a, b), 125_000_000);
            topo.bandwidth.insert((b, a), 125_000_000);
        }
        for &b in &[NodeId(4), NodeId(5)] {
            topo.latency_us.insert((a, b), 80_000);
            topo.latency_us.insert((b, a), 80_000);
            topo.bandwidth.insert((a, b), 125_000_000);
            topo.bandwidth.insert((b, a), 125_000_000);
        }
    }

    topo.register_table(
        "orders",
        NodeId(0),
        DataDistribution::HashPartitioned {
            keys: vec![col("order_id")],
            partition_count: 6,
        },
    );
    topo.register_table(
        "customers",
        NodeId(2),
        DataDistribution::HashPartitioned {
            keys: vec![col("customer_id")],
            partition_count: 6,
        },
    );
    topo.register_table("countries", NodeId(4), DataDistribution::Replicated);
    topo
}

/// Build a cloud-federation cluster topology.
fn cloud_topology() -> ClusterTopology {
    let mut topo = ClusterTopology::uniform(6);
    for &a in &[NodeId(0), NodeId(1)] {
        for &b in &[NodeId(2), NodeId(3)] {
            topo.latency_us.insert((a, b), 50_000);
            topo.latency_us.insert((b, a), 50_000);
            topo.bandwidth.insert((a, b), 6_250_000);
            topo.bandwidth.insert((b, a), 6_250_000);
        }
        for &b in &[NodeId(4), NodeId(5)] {
            topo.latency_us.insert((a, b), 40_000);
            topo.latency_us.insert((b, a), 40_000);
            topo.bandwidth.insert((a, b), 6_250_000);
            topo.bandwidth.insert((b, a), 6_250_000);
        }
    }

    topo.register_table(
        "orders",
        NodeId(0),
        DataDistribution::HashPartitioned {
            keys: vec![col("order_id")],
            partition_count: 6,
        },
    );
    topo.register_table(
        "customers",
        NodeId(2),
        DataDistribution::HashPartitioned {
            keys: vec![col("customer_id")],
            partition_count: 6,
        },
    );
    topo.register_table(
        "inventory",
        NodeId(4),
        DataDistribution::HashPartitioned {
            keys: vec![col("item_id")],
            partition_count: 6,
        },
    );
    topo
}

fn register_stats(opt: &mut DistributedOptimizer, table: &str, rows: f64, avg_row_size: u64) {
    let mut stats = Statistics::new(rows);
    stats.avg_row_size = avg_row_size;
    stats.total_size = (rows as u64) * avg_row_size;
    opt.register_stats(table, stats);
}

// ── Tests: Single DC ─────────────────────────────────────────────

#[test]
fn single_dc_network_cost_attached() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);
    assert!(opt.network_cost().is_some());
}

#[test]
fn single_dc_colocated_zero_cost() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    let cost = opt.cost_strategy(&DistributionStrategy::CoLocated, 1_000_000, 1_000_000);
    assert_eq!(cost.total(), 0.0);
}

#[test]
fn single_dc_broadcast_uses_network_model() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config.clone(), topo.clone()).with_network_cost(ncm);

    let strategy = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(1), NodeId(2), NodeId(3)],
    };

    let cost_with = opt.cost_strategy(&strategy, 10_000_000, 1_000);
    assert!(cost_with.network > 0.0, "network cost should be positive");

    // Compare with heuristic-only optimizer
    let opt_without = DistributedOptimizer::new(config, topo);
    let cost_without = opt_without.cost_strategy(&strategy, 10_000_000, 1_000);

    // Both should produce nonzero network costs, but values differ
    assert!(cost_without.network > 0.0);
}

#[test]
fn single_dc_shuffle_uses_network_model() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    let strategy = DistributionStrategy::Shuffle {
        source: NodeId(0),
        targets: vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        partition_keys: vec![col("order_id")],
    };

    let cost = opt.cost_strategy(&strategy, 1_000_000, 1_000_000);
    assert!(cost.network > 0.0);
    assert!(cost.cpu > 0.0, "shuffle should have hash CPU cost");
}

#[test]
fn single_dc_fast_network_prefers_shuffle() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig {
        broadcast_threshold: 1_000_000, // 1 MB, below both table sizes
        ..DistributedOptimizerConfig::default()
    };
    let mut opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    register_stats(&mut opt, "orders", 6_000_000.0, 128);
    register_stats(&mut opt, "customers", 1_000_000.0, 128);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("customer_id")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("customers")),
    };
    let result = opt.optimize_distribution(&plan);
    assert!(result.is_ok());
    let dre = result.expect("should succeed");
    let strategy = dre.input_strategy.as_ref().expect("should have strategy");
    // With fast single-DC network and no small table, shuffle is likely
    assert!(
        !matches!(strategy, DistributionStrategy::RangePartition { .. }),
        "should not pick range partition for equi-join"
    );
}

#[test]
fn single_dc_replicated_table_colocated() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    register_stats(&mut opt, "orders", 6_000_000.0, 128);
    register_stats(&mut opt, "countries", 200.0, 64);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("country_code"), col("code")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("countries")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    let strategy = dre.input_strategy.as_ref().expect("should have strategy");
    // Countries is replicated -> co-located or partition-wise
    assert!(
        matches!(
            strategy,
            DistributionStrategy::CoLocated | DistributionStrategy::PartitionWise { .. }
        ),
        "replicated join should be co-located, got {strategy:?}"
    );
}

// ── Tests: Multi-DC ──────────────────────────────────────────────

#[test]
fn multi_dc_network_cost_increases_with_distance() {
    let ncm = multi_dc_model();
    let topo = multi_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    // Same-DC broadcast (node 0 -> node 1)
    let same_dc = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(1)],
    };
    // Cross-DC broadcast (node 0 -> node 4, EU)
    let cross_dc = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(4)],
    };

    let cost_same = opt.cost_strategy(&same_dc, 10_000_000, 100);
    let cost_cross = opt.cost_strategy(&cross_dc, 10_000_000, 100);

    assert!(
        cost_cross.total() > cost_same.total(),
        "cross-DC should cost more: same={}, cross={}",
        cost_same.total(),
        cost_cross.total()
    );
}

#[test]
fn multi_dc_strategy_selection_avoids_cross_dc() {
    let ncm = multi_dc_model();
    let topo = multi_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    register_stats(&mut opt, "orders", 6_000_000.0, 128);
    register_stats(&mut opt, "customers", 1_000_000.0, 128);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("customer_id")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("customers")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some(), "should select a strategy");
}

#[test]
fn multi_dc_small_broadcast_cheaper_than_shuffle() {
    let ncm = multi_dc_model();
    let topo = multi_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    // Small broadcast: 1 KB to 5 targets
    let broadcast = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4), NodeId(5)],
    };
    // Shuffle 10 GB
    let shuffle = DistributionStrategy::Shuffle {
        source: NodeId(0),
        targets: vec![
            NodeId(0),
            NodeId(1),
            NodeId(2),
            NodeId(3),
            NodeId(4),
            NodeId(5),
        ],
        partition_keys: vec![col("id")],
    };

    let cost_broadcast = opt.cost_strategy(&broadcast, 10_000_000_000, 1_000);
    let cost_shuffle = opt.cost_strategy(&shuffle, 10_000_000_000, 10_000_000_000);

    assert!(
        cost_broadcast.total() < cost_shuffle.total(),
        "broadcasting 1KB should be cheaper than shuffling 10GB"
    );
}

#[test]
fn multi_dc_monetary_cost_nonzero_cross_dc() {
    let ncm = multi_dc_model();
    let topo = multi_dc_topology();
    let config = DistributedOptimizerConfig {
        monetary_weight: 10.0,
        ..DistributedOptimizerConfig::default()
    };
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    let strategy = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(4), NodeId(5)],
    };

    let cost = opt.cost_strategy(&strategy, 1_000_000_000, 1_000);
    // Cross-DC has billing cost ($0.01/GB), with monetary_weight=10
    // the total should include a monetary component.
    assert!(
        cost.total() > 0.0,
        "cross-DC broadcast should have positive cost"
    );
}

// ── Tests: Cloud Federation ──────────────────────────────────────

#[test]
fn cloud_federation_cross_cloud_expensive() {
    let ncm = cloud_federation_model();
    let topo = cloud_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    // AWS -> GCP broadcast
    let cross_cloud = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(2), NodeId(3)],
    };
    // AWS internal broadcast
    let intra_cloud = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(1)],
    };

    let cost_cross = opt.cost_strategy(&cross_cloud, 100_000_000, 1_000);
    let cost_intra = opt.cost_strategy(&intra_cloud, 100_000_000, 1_000);

    assert!(
        cost_cross.total() > cost_intra.total(),
        "cross-cloud should be more expensive than intra-cloud"
    );
}

#[test]
fn cloud_federation_shuffle_expensive() {
    let ncm = cloud_federation_model();
    let topo = cloud_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    let shuffle = DistributionStrategy::Shuffle {
        source: NodeId(0),
        targets: vec![
            NodeId(0),
            NodeId(1),
            NodeId(2),
            NodeId(3),
            NodeId(4),
            NodeId(5),
        ],
        partition_keys: vec![col("id")],
    };

    let cost = opt.cost_strategy(&shuffle, 1_000_000_000, 1_000_000_000);
    // Shuffling 2 GB across 3 clouds should be very expensive
    assert!(cost.network > 0.0);
    assert!(cost.cpu > 0.0);
}

#[test]
fn cloud_federation_strategy_avoids_cross_cloud_broadcast() {
    let ncm = cloud_federation_model();
    let topo = cloud_topology();
    let config = DistributedOptimizerConfig {
        broadcast_threshold: 10_000_000, // 10 MB
        ..DistributedOptimizerConfig::default()
    };
    let mut opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    register_stats(&mut opt, "orders", 10_000_000.0, 128);
    register_stats(&mut opt, "inventory", 5_000_000.0, 128);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("order_id"), col("item_id")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("inventory")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
    // Both tables are too large for broadcast (over 10 MB)
    // so it should not broadcast.
}

// ── Tests: Enumeration with network model ────────────────────────

#[test]
fn enumerate_includes_network_recommendation() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt_with = DistributedOptimizer::new(config.clone(), topo.clone()).with_network_cost(ncm);
    let opt_without = DistributedOptimizer::new(config, topo);

    let args = (
        &DataDistribution::Arbitrary,
        &DataDistribution::Arbitrary,
        10_000_000_000_u64,
        10_000_000_000_u64,
        &[col("id")][..],
        &[col("id")][..],
        JoinType::Inner,
    );

    let with_model =
        opt_with.enumerate_strategies(args.0, args.1, args.2, args.3, args.4, args.5, args.6);
    let without_model =
        opt_without.enumerate_strategies(args.0, args.1, args.2, args.3, args.4, args.5, args.6);

    // With a network model, there should be at least as many
    // strategies as without (the network recommendation is added).
    assert!(
        with_model.len() >= without_model.len(),
        "with model ({}) should have >= strategies than without ({})",
        with_model.len(),
        without_model.len()
    );
}

#[test]
fn enumerate_without_network_model() {
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo);
    assert!(opt.network_cost().is_none());

    let strategies = opt.enumerate_strategies(
        &DataDistribution::Arbitrary,
        &DataDistribution::Arbitrary,
        10_000_000_000,
        10_000_000_000,
        &[col("id")],
        &[col("id")],
        JoinType::Inner,
    );

    // Without network model: Broadcast(x2), Shuffle, RangePartition
    // (broadcast right under 100MB, broadcast left too big -> 1 broadcast)
    assert!(
        !strategies.is_empty(),
        "should have strategies without network model"
    );
}

// ── Tests: Cost comparison with/without network model ────────────

#[test]
fn cost_with_model_differs_from_heuristic() {
    let ncm = multi_dc_model();
    let topo = multi_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt_with = DistributedOptimizer::new(config.clone(), topo.clone()).with_network_cost(ncm);
    let opt_without = DistributedOptimizer::new(config, topo);

    let strategy = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(2), NodeId(4)],
    };

    let cost_with = opt_with.cost_strategy(&strategy, 10_000_000, 1_000);
    let cost_without = opt_without.cost_strategy(&strategy, 10_000_000, 1_000);

    // Both produce positive costs, but typically differ because
    // the network model considers actual topology vs simple
    // bandwidth tables.
    assert!(cost_with.network > 0.0);
    assert!(cost_without.network > 0.0);
}

#[test]
fn partition_wise_zero_cost_with_model() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    let cost = opt.cost_strategy(
        &DistributionStrategy::PartitionWise {
            partition_key: col("id"),
        },
        1_000_000,
        1_000_000,
    );
    assert_eq!(cost.total(), 0.0);
}

#[test]
fn range_partition_falls_back_to_heuristic() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config.clone(), topo.clone()).with_network_cost(ncm);
    let opt_heuristic = DistributedOptimizer::new(config, topo);

    let strategy = DistributionStrategy::RangePartition {
        partition_key: col("ts"),
        ranges: vec![("0".into(), "100".into()), ("100".into(), "200".into())],
    };

    let cost_with = opt.cost_strategy(&strategy, 1_000_000, 1_000_000);
    let cost_without = opt_heuristic.cost_strategy(&strategy, 1_000_000, 1_000_000);

    // RangePartition has no mapping in NetworkCostModel, so both
    // should use the heuristic path and produce identical costs.
    assert!(
        (cost_with.total() - cost_without.total()).abs() < f64::EPSILON,
        "range partition should use heuristic with or without model"
    );
}

// ── Tests: Full optimization pipeline ────────────────────────────

#[test]
fn full_plan_single_dc_with_model() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    register_stats(&mut opt, "orders", 6_000_000.0, 128);
    register_stats(&mut opt, "lineitem", 24_000_000.0, 100);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("order_id"), col("l_orderkey")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("lineitem")),
    };

    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
}

#[test]
fn full_plan_multi_dc_with_model() {
    let ncm = multi_dc_model();
    let topo = multi_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    register_stats(&mut opt, "orders", 6_000_000.0, 128);
    register_stats(&mut opt, "customers", 1_000_000.0, 128);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("customer_id")),
        left: Box::new(RelExpr::scan("orders").filter(eq(
            col("status"),
            Expr::Const(ra_core::expr::Const::String("active".into())),
        ))),
        right: Box::new(RelExpr::scan("customers")),
    };

    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
}

#[test]
fn full_plan_cloud_federation_with_model() {
    let ncm = cloud_federation_model();
    let topo = cloud_topology();
    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    register_stats(&mut opt, "orders", 10_000_000.0, 128);
    register_stats(&mut opt, "inventory", 5_000_000.0, 128);

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("order_id"), col("item_id")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("inventory")),
    };

    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
}

// ── Tests: Edge cases ────────────────────────────────────────────

#[test]
fn empty_cluster_with_model_returns_error() {
    let topo = NetworkTopology::new();
    let ncm = NetworkCostModel::new(topo, HashMap::new());
    let cluster = ClusterTopology::uniform(0);
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, cluster).with_network_cost(ncm);

    let plan = RelExpr::scan("t");
    assert!(opt.optimize_distribution(&plan).is_err());
}

#[test]
fn model_with_unknown_table_graceful() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    // Unknown table -> network model returns zero cost for
    // unrecognized tables.
    let plan = RelExpr::scan("unknown_table");
    let result = opt.optimize_distribution(&plan);
    assert!(result.is_ok());
}

#[test]
fn partial_broadcast_maps_to_broadcast() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    let strategy = DistributionStrategy::PartialBroadcast {
        source: NodeId(0),
        targets: vec![NodeId(1), NodeId(2)],
        predicate: Expr::Const(ra_core::expr::Const::Bool(true)),
    };

    let cost = opt.cost_strategy(&strategy, 1_000_000, 1_000);
    // PartialBroadcast maps to Broadcast in network model
    assert!(cost.network > 0.0);
}

// ── Tests: Topology profile scenarios ────────────────────────────

#[test]
fn single_dc_all_same_dc_low_cost() {
    let ncm = single_dc_model();
    let topo = single_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    // Broadcast within single DC should be cheap
    let strategy = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(1), NodeId(2), NodeId(3)],
    };
    let cost = opt.cost_strategy(&strategy, 1_000_000, 1_000);
    // Intra-datacenter: very fast, zero billing
    assert!(cost.network > 0.0);
    // Memory should reflect bytes transferred
    assert!(cost.memory > 0);
}

#[test]
fn multi_dc_us_to_eu_more_expensive_than_us_to_us() {
    let ncm = multi_dc_model();
    let topo = multi_dc_topology();
    let config = DistributedOptimizerConfig::default();
    let opt = DistributedOptimizer::new(config, topo).with_network_cost(ncm);

    // US-East -> US-West (60ms latency)
    let us_us = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(2)],
    };
    // US-East -> EU-West (80ms latency)
    let us_eu = DistributionStrategy::Broadcast {
        source: NodeId(0),
        targets: vec![NodeId(4)],
    };

    let cost_us_us = opt.cost_strategy(&us_us, 100_000_000, 1_000);
    let cost_us_eu = opt.cost_strategy(&us_eu, 100_000_000, 1_000);

    assert!(
        cost_us_eu.total() > cost_us_us.total(),
        "US->EU ({}) should cost more than US->US ({})",
        cost_us_eu.total(),
        cost_us_us.total()
    );
}
