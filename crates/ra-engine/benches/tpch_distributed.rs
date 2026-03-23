//! TPC-H distributed query benchmarks.
//!
//! Measures distributed optimizer strategy selection and costing
//! for TPC-H queries across four network topology profiles:
//! single-node, single-DC, multi-DC, and cloud federation.

#![allow(clippy::expect_used)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_precision_loss)]

use std::collections::HashMap;
use std::path::PathBuf;

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId,
    Criterion,
};

use ra_core::algebra::RelExpr;
use ra_core::distribution::{DataDistribution, NodeId};
use ra_core::expr::{ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::distributed_optimizer::{
    ClusterTopology, DistributedOptimizer,
    DistributedOptimizerConfig,
};
use ra_engine::network_cost::NetworkCostModel;
use ra_hardware::network::NetworkTopology;
use ra_parser::sql_to_relexpr;

// ── helpers ──────────────────────────────────────────────────────

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn hw_node(id: u32) -> ra_hardware::network::NodeId {
    ra_hardware::network::NodeId(id)
}

// ── TPC-H table statistics (SF=1) ───────────────────────────────

fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

fn register_tpch_stats(opt: &mut DistributedOptimizer) {
    opt.register_stats("lineitem", make_stats(6_001_215.0, 128));
    opt.register_stats("orders", make_stats(1_500_000.0, 150));
    opt.register_stats("customer", make_stats(150_000.0, 200));
    opt.register_stats("supplier", make_stats(10_000.0, 180));
    opt.register_stats("nation", make_stats(25.0, 64));
    opt.register_stats("region", make_stats(5.0, 48));
    opt.register_stats("part", make_stats(200_000.0, 160));
    opt.register_stats("partsupp", make_stats(800_000.0, 144));
}

// ── TPC-H queries (loaded from SQL files) ────────────────────────

fn tpch_queries_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop(); // crates/
    dir.pop(); // project root
    dir.push("benchmarks/tpch/queries");
    dir
}

fn load_tpch_query(name: &str) -> RelExpr {
    let path = tpch_queries_dir().join(name);
    let sql = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    sql_to_relexpr(&sql)
        .unwrap_or_else(|e| panic!("parse {name}: {e}"))
}

fn tpch_q1() -> RelExpr { load_tpch_query("q1.sql") }
fn tpch_q3() -> RelExpr { load_tpch_query("q3.sql") }
fn tpch_q5() -> RelExpr { load_tpch_query("q5.sql") }
fn tpch_q6() -> RelExpr { load_tpch_query("q6.sql") }
fn tpch_q8() -> RelExpr { load_tpch_query("q8.sql") }
fn tpch_q13() -> RelExpr { load_tpch_query("q13.sql") }
fn tpch_q18() -> RelExpr { load_tpch_query("q18.sql") }

// ── topology builders ────────────────────────────────────────────

fn tpch_tables() -> Vec<&'static str> {
    vec![
        "lineitem", "orders", "customer", "supplier", "nation",
        "region", "part", "partsupp",
    ]
}

/// Single-node baseline: 1 node, no distribution.
fn single_node_optimizer() -> DistributedOptimizer {
    let mut topo = ClusterTopology::uniform(1);
    for table in tpch_tables() {
        topo.register_table(
            table,
            NodeId(0),
            DataDistribution::SinglePartition {
                node: NodeId(0),
            },
        );
    }
    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, topo);
    register_tpch_stats(&mut opt);
    opt
}

/// 4-node single-DC cluster with hash-partitioned tables.
fn single_dc_optimizer() -> DistributedOptimizer {
    let hw_topo = NetworkTopology::single_datacenter_cluster();
    let mut assignments = HashMap::new();
    for (i, table) in tpch_tables().iter().enumerate() {
        assignments.insert(
            (*table).to_string(),
            hw_node((i % 4) as u32),
        );
    }
    let ncm = NetworkCostModel::new(hw_topo, assignments);

    let mut cluster = ClusterTopology::uniform(4);
    // Hash-partition large tables, replicate small ones
    let large = ["lineitem", "orders", "customer", "partsupp"];
    let small = ["nation", "region", "supplier", "part"];
    for table in large {
        cluster.register_table(
            table,
            NodeId(0),
            DataDistribution::HashPartitioned {
                keys: vec![col(&format!("{}_key", table))],
                partition_count: 4,
            },
        );
    }
    for table in small {
        cluster.register_table(
            table,
            NodeId(0),
            DataDistribution::Replicated,
        );
    }

    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, cluster)
        .with_network_cost(ncm);
    register_tpch_stats(&mut opt);
    opt
}

/// 6-node multi-DC (US-East, US-West, EU-West).
fn multi_dc_optimizer() -> DistributedOptimizer {
    let hw_topo = NetworkTopology::multi_datacenter();
    let mut assignments = HashMap::new();
    // Spread tables across DCs
    let dc_assignments = [
        ("lineitem", 0),
        ("orders", 0),
        ("customer", 2),
        ("supplier", 4),
        ("nation", 0),
        ("region", 0),
        ("part", 2),
        ("partsupp", 4),
    ];
    for (table, node) in dc_assignments {
        assignments.insert(table.to_string(), hw_node(node));
    }
    let ncm = NetworkCostModel::new(hw_topo, assignments);

    let mut cluster = ClusterTopology::uniform(6);
    for &a in &[NodeId(0), NodeId(1)] {
        for &b in &[NodeId(2), NodeId(3)] {
            cluster.latency_us.insert((a, b), 60_000);
            cluster.latency_us.insert((b, a), 60_000);
            cluster.bandwidth.insert((a, b), 125_000_000);
            cluster.bandwidth.insert((b, a), 125_000_000);
        }
        for &b in &[NodeId(4), NodeId(5)] {
            cluster.latency_us.insert((a, b), 80_000);
            cluster.latency_us.insert((b, a), 80_000);
            cluster.bandwidth.insert((a, b), 125_000_000);
            cluster.bandwidth.insert((b, a), 125_000_000);
        }
    }
    let large = ["lineitem", "orders", "customer", "partsupp"];
    let small = ["nation", "region", "supplier", "part"];
    for table in large {
        cluster.register_table(
            table,
            NodeId(0),
            DataDistribution::HashPartitioned {
                keys: vec![col(&format!("{}_key", table))],
                partition_count: 6,
            },
        );
    }
    for table in small {
        cluster.register_table(
            table,
            NodeId(0),
            DataDistribution::Replicated,
        );
    }

    let config = DistributedOptimizerConfig::default();
    let mut opt = DistributedOptimizer::new(config, cluster)
        .with_network_cost(ncm);
    register_tpch_stats(&mut opt);
    opt
}

/// 6-node cloud federation (AWS + GCP + Azure).
fn cloud_federation_optimizer() -> DistributedOptimizer {
    let hw_topo = NetworkTopology::cloud_federation();
    let mut assignments = HashMap::new();
    let cloud_assignments = [
        ("lineitem", 0),
        ("orders", 0),
        ("customer", 2),
        ("supplier", 2),
        ("nation", 0),
        ("region", 0),
        ("part", 4),
        ("partsupp", 4),
    ];
    for (table, node) in cloud_assignments {
        assignments.insert(table.to_string(), hw_node(node));
    }
    let ncm = NetworkCostModel::new(hw_topo, assignments);

    let mut cluster = ClusterTopology::uniform(6);
    for &a in &[NodeId(0), NodeId(1)] {
        for &b in &[NodeId(2), NodeId(3)] {
            cluster.latency_us.insert((a, b), 50_000);
            cluster.latency_us.insert((b, a), 50_000);
            cluster.bandwidth.insert((a, b), 6_250_000);
            cluster.bandwidth.insert((b, a), 6_250_000);
        }
        for &b in &[NodeId(4), NodeId(5)] {
            cluster.latency_us.insert((a, b), 40_000);
            cluster.latency_us.insert((b, a), 40_000);
            cluster.bandwidth.insert((a, b), 6_250_000);
            cluster.bandwidth.insert((b, a), 6_250_000);
        }
    }
    let large = ["lineitem", "orders", "customer", "partsupp"];
    let small = ["nation", "region", "supplier", "part"];
    for table in large {
        cluster.register_table(
            table,
            NodeId(0),
            DataDistribution::HashPartitioned {
                keys: vec![col(&format!("{}_key", table))],
                partition_count: 6,
            },
        );
    }
    for table in small {
        cluster.register_table(
            table,
            NodeId(0),
            DataDistribution::Replicated,
        );
    }

    let config = DistributedOptimizerConfig {
        monetary_weight: 5.0,
        ..DistributedOptimizerConfig::default()
    };
    let mut opt = DistributedOptimizer::new(config, cluster)
        .with_network_cost(ncm);
    register_tpch_stats(&mut opt);
    opt
}

// ── benchmark groups ─────────────────────────────────────────────

type QueryFn = fn() -> RelExpr;

const QUERIES: &[(&str, QueryFn)] = &[
    ("Q1", tpch_q1 as QueryFn),
    ("Q3", tpch_q3),
    ("Q5", tpch_q5),
    ("Q6", tpch_q6),
    ("Q8", tpch_q8),
    ("Q13", tpch_q13),
    ("Q18", tpch_q18),
];

fn bench_strategy_selection(c: &mut Criterion) {
    let mut group =
        c.benchmark_group("tpch_strategy_selection");

    for (name, query_fn) in QUERIES {
        let plan = query_fn();

        group.bench_with_input(
            BenchmarkId::new("single_node", name),
            &plan,
            |b, p| {
                let opt = single_node_optimizer();
                b.iter(|| {
                    let _ = black_box(
                        opt.optimize_distribution(p),
                    );
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single_dc", name),
            &plan,
            |b, p| {
                let opt = single_dc_optimizer();
                b.iter(|| {
                    let _ = black_box(
                        opt.optimize_distribution(p),
                    );
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("multi_dc", name),
            &plan,
            |b, p| {
                let opt = multi_dc_optimizer();
                b.iter(|| {
                    let _ = black_box(
                        opt.optimize_distribution(p),
                    );
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("cloud_federation", name),
            &plan,
            |b, p| {
                let opt = cloud_federation_optimizer();
                b.iter(|| {
                    let _ = black_box(
                        opt.optimize_distribution(p),
                    );
                });
            },
        );
    }
    group.finish();
}

fn bench_cost_estimation(c: &mut Criterion) {
    let mut group = c.benchmark_group("tpch_cost_estimation");

    let topologies: &[(&str, fn() -> DistributedOptimizer)] = &[
        ("single_node", single_node_optimizer as fn() -> _),
        ("single_dc", single_dc_optimizer),
        ("multi_dc", multi_dc_optimizer),
        ("cloud_federation", cloud_federation_optimizer),
    ];

    let strategies = [
        (
            "broadcast_small",
            DataDistribution::Replicated,
            DataDistribution::Arbitrary,
            10_000_u64,
            6_000_000_000_u64,
        ),
        (
            "shuffle_both",
            DataDistribution::Arbitrary,
            DataDistribution::Arbitrary,
            1_500_000_000,
            6_000_000_000,
        ),
    ];

    for (topo_name, builder) in topologies {
        for (strat_name, _, _, left_bytes, right_bytes) in
            &strategies
        {
            let opt = builder();
            let nodes = &opt.topology().nodes;
            let source =
                nodes.first().copied().unwrap_or(NodeId(0));

            let strat = if *strat_name == "broadcast_small" {
                ra_core::distribution::DistributionStrategy::Broadcast {
                    source,
                    targets: nodes.clone(),
                }
            } else {
                ra_core::distribution::DistributionStrategy::Shuffle {
                    source,
                    targets: nodes.clone(),
                    partition_keys: vec![col("key")],
                }
            };

            group.bench_with_input(
                BenchmarkId::new(
                    format!("{topo_name}/{strat_name}"),
                    "",
                ),
                &(),
                |b, _| {
                    b.iter(|| {
                        black_box(opt.cost_strategy(
                            &strat,
                            *left_bytes,
                            *right_bytes,
                        ));
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_strategy_selection,
    bench_cost_estimation,
);
criterion_main!(benches);
