//! TPC-H distributed query benchmarks.
//!
//! Measures distributed optimizer strategy selection and costing
//! for TPC-H queries across four network topology profiles:
//! single-node, single-DC, multi-DC, and cloud federation.

#![allow(clippy::expect_used)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_precision_loss)]

use std::collections::HashMap;

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId,
    Criterion,
};

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, RelExpr,
};
use ra_core::distribution::{DataDistribution, NodeId};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_engine::distributed_optimizer::{
    ClusterTopology, DistributedOptimizer,
    DistributedOptimizerConfig,
};
use ra_engine::network_cost::NetworkCostModel;
use ra_hardware::network::NetworkTopology;

// ── expression helpers ───────────────────────────────────────────

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

fn gt(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn lt(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Lt,
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

fn mul(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Mul,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn sub(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Sub,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn int(v: i64) -> Expr {
    Expr::Const(Const::Int(v))
}

fn float(v: f64) -> Expr {
    Expr::Const(Const::Float(v))
}

fn str_const(v: &str) -> Expr {
    Expr::Const(Const::String(v.into()))
}

fn agg(func: AggregateFunction, arg: Expr) -> AggregateExpr {
    AggregateExpr {
        function: func,
        arg: Some(arg),
        distinct: false,
        alias: None,
    }
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

// ── TPC-H queries ────────────────────────────────────────────────

/// Q1: Pricing summary report (aggregation with filter).
fn tpch_q1() -> RelExpr {
    let scan = RelExpr::scan("lineitem").filter(lt(
        col("l_shipdate"),
        str_const("1998-09-02"),
    ));
    RelExpr::Aggregate {
        input: Box::new(scan),
        group_by: vec![col("l_returnflag"), col("l_linestatus")],
        aggregates: vec![
            agg(AggregateFunction::Sum, col("l_quantity")),
            agg(
                AggregateFunction::Sum,
                col("l_extendedprice"),
            ),
            agg(
                AggregateFunction::Sum,
                mul(
                    col("l_extendedprice"),
                    sub(int(1), col("l_discount")),
                ),
            ),
            agg(AggregateFunction::Count, col("l_orderkey")),
        ],
    }
}

/// Q3: Shipping priority (join + aggregation).
fn tpch_q3() -> RelExpr {
    let cust =
        RelExpr::scan("customer").filter(eq(
            col("c_mktsegment"),
            str_const("BUILDING"),
        ));
    let orders = RelExpr::scan("orders").filter(lt(
        col("o_orderdate"),
        str_const("1995-03-15"),
    ));
    let lineitem = RelExpr::scan("lineitem").filter(gt(
        col("l_shipdate"),
        str_const("1995-03-15"),
    ));
    let co_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("c_custkey"), col("o_custkey")),
        left: Box::new(cust),
        right: Box::new(orders),
    };
    let col_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("l_orderkey"), col("o_orderkey")),
        left: Box::new(co_join),
        right: Box::new(lineitem),
    };
    RelExpr::Aggregate {
        input: Box::new(col_join),
        group_by: vec![
            col("l_orderkey"),
            col("o_orderdate"),
            col("o_shippriority"),
        ],
        aggregates: vec![agg(
            AggregateFunction::Sum,
            mul(
                col("l_extendedprice"),
                sub(int(1), col("l_discount")),
            ),
        )],
    }
}

/// Q5: Local supplier volume (multi-table join).
fn tpch_q5() -> RelExpr {
    let region = RelExpr::scan("region").filter(eq(
        col("r_name"),
        str_const("ASIA"),
    ));
    let nr_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("n_regionkey"), col("r_regionkey")),
        left: Box::new(RelExpr::scan("nation")),
        right: Box::new(region),
    };
    let cn_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("c_nationkey"), col("n_nationkey")),
        left: Box::new(RelExpr::scan("customer")),
        right: Box::new(nr_join),
    };
    let oc_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("o_custkey"), col("c_custkey")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(cn_join),
    };
    let lo_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("l_orderkey"), col("o_orderkey")),
        left: Box::new(RelExpr::scan("lineitem")),
        right: Box::new(oc_join),
    };
    let ls_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: and(
            eq(col("l_suppkey"), col("s_suppkey")),
            eq(col("s_nationkey"), col("c_nationkey")),
        ),
        left: Box::new(lo_join),
        right: Box::new(RelExpr::scan("supplier")),
    };
    RelExpr::Aggregate {
        input: Box::new(ls_join),
        group_by: vec![col("n_name")],
        aggregates: vec![agg(
            AggregateFunction::Sum,
            mul(
                col("l_extendedprice"),
                sub(int(1), col("l_discount")),
            ),
        )],
    }
}

/// Q6: Forecasting revenue change (simple filter + aggregation).
fn tpch_q6() -> RelExpr {
    let scan = RelExpr::scan("lineitem").filter(and(
        and(
            gt(col("l_shipdate"), str_const("1994-01-01")),
            lt(col("l_shipdate"), str_const("1995-01-01")),
        ),
        and(
            gt(col("l_discount"), float(0.05)),
            lt(col("l_quantity"), int(24)),
        ),
    ));
    RelExpr::Aggregate {
        input: Box::new(scan),
        group_by: vec![],
        aggregates: vec![agg(
            AggregateFunction::Sum,
            mul(col("l_extendedprice"), col("l_discount")),
        )],
    }
}

/// Q8: National market share (8-table join).
fn tpch_q8() -> RelExpr {
    let region = RelExpr::scan("region").filter(eq(
        col("r_name"),
        str_const("AMERICA"),
    ));
    let nr_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("n_regionkey"), col("r_regionkey")),
        left: Box::new(RelExpr::scan("nation")),
        right: Box::new(region),
    };
    let cn_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("c_nationkey"), col("n_nationkey")),
        left: Box::new(RelExpr::scan("customer")),
        right: Box::new(nr_join),
    };
    let oc_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("o_custkey"), col("c_custkey")),
        left: Box::new(
            RelExpr::scan("orders").filter(and(
                gt(col("o_orderdate"), str_const("1995-01-01")),
                lt(col("o_orderdate"), str_const("1996-12-31")),
            )),
        ),
        right: Box::new(cn_join),
    };
    let lo_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("l_orderkey"), col("o_orderkey")),
        left: Box::new(RelExpr::scan("lineitem")),
        right: Box::new(oc_join),
    };
    let ps_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("l_suppkey"), col("s_suppkey")),
        left: Box::new(lo_join),
        right: Box::new(RelExpr::scan("supplier")),
    };
    let part = RelExpr::scan("part").filter(eq(
        col("p_type"),
        str_const("ECONOMY ANODIZED STEEL"),
    ));
    let lp_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("l_partkey"), col("p_partkey")),
        left: Box::new(ps_join),
        right: Box::new(part),
    };
    RelExpr::Aggregate {
        input: Box::new(lp_join),
        group_by: vec![col("o_year")],
        aggregates: vec![agg(
            AggregateFunction::Sum,
            col("volume"),
        )],
    }
}

/// Q13: Customer distribution (left outer join).
fn tpch_q13() -> RelExpr {
    let co_join = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("c_custkey"), col("o_custkey")),
        left: Box::new(RelExpr::scan("customer")),
        right: Box::new(RelExpr::scan("orders")),
    };
    let inner = RelExpr::Aggregate {
        input: Box::new(co_join),
        group_by: vec![col("c_custkey")],
        aggregates: vec![agg(
            AggregateFunction::Count,
            col("o_orderkey"),
        )],
    };
    RelExpr::Aggregate {
        input: Box::new(inner),
        group_by: vec![col("c_count")],
        aggregates: vec![agg(
            AggregateFunction::Count,
            col("c_custkey"),
        )],
    }
}

/// Q18: Large volume customer (group by + having equivalent).
fn tpch_q18() -> RelExpr {
    let sub_agg = RelExpr::Aggregate {
        input: Box::new(RelExpr::scan("lineitem")),
        group_by: vec![col("l_orderkey")],
        aggregates: vec![agg(
            AggregateFunction::Sum,
            col("l_quantity"),
        )],
    };
    let sub_filter = sub_agg.filter(gt(col("sum_qty"), int(300)));
    let co_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("c_custkey"), col("o_custkey")),
        left: Box::new(RelExpr::scan("customer")),
        right: Box::new(RelExpr::scan("orders")),
    };
    let main_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("o_orderkey"), col("l_orderkey")),
        left: Box::new(co_join),
        right: Box::new(sub_filter),
    };
    RelExpr::Aggregate {
        input: Box::new(main_join),
        group_by: vec![
            col("c_name"),
            col("c_custkey"),
            col("o_orderkey"),
            col("o_orderdate"),
            col("o_totalprice"),
        ],
        aggregates: vec![agg(
            AggregateFunction::Sum,
            col("l_quantity"),
        )],
    }
}

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
