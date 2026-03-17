//! Benchmarks for the optimization engine.
//!
//! Measures optimization latency for typical query patterns to
//! verify the <100ms performance target.

#![allow(clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{rec_expr_to_rel_expr, to_rec_expr, Optimizer, OptimizerConfig};

fn simple_scan() -> RelExpr {
    RelExpr::scan("users")
}

fn filtered_scan() -> RelExpr {
    RelExpr::scan("users").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("age"))),
        right: Box::new(Expr::Const(Const::Int(18))),
    })
}

fn two_table_join() -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("users", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("orders", "user_id"))),
        },
        left: Box::new(RelExpr::scan("users")),
        right: Box::new(RelExpr::scan("orders")),
    }
}

fn three_table_join() -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("orders", "product_id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("products", "id"))),
        },
        left: Box::new(two_table_join()),
        right: Box::new(RelExpr::scan("products")),
    }
}

fn filtered_join() -> RelExpr {
    two_table_join().filter(Expr::BinOp {
        op: BinOp::And,
        left: Box::new(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("amount"))),
            right: Box::new(Expr::Const(Const::Int(100))),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("status"))),
            right: Box::new(Expr::Const(Const::String("active".to_owned()))),
        }),
    })
}

fn aggregate_query() -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("department"))],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".to_owned()),
            },
            AggregateExpr {
                function: AggregateFunction::Avg,
                arg: Some(Expr::Column(ColumnRef::new("salary"))),
                distinct: false,
                alias: Some("avg_salary".to_owned()),
            },
        ],
        input: Box::new(filtered_scan()),
    }
}

fn complex_query() -> RelExpr {
    RelExpr::Sort {
        keys: vec![SortKey {
            expr: Expr::Column(ColumnRef::new("total")),
            direction: SortDirection::Desc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(
            RelExpr::Aggregate {
                group_by: vec![Expr::Column(ColumnRef::qualified("users", "name"))],
                aggregates: vec![AggregateExpr {
                    function: AggregateFunction::Sum,
                    arg: Some(Expr::Column(ColumnRef::new("amount"))),
                    distinct: false,
                    alias: Some("total".to_owned()),
                }],
                input: Box::new(filtered_join()),
            }
            .limit(10, 0),
        ),
    }
}

fn project_query() -> RelExpr {
    RelExpr::scan("users").project(vec![
        ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("name")),
            alias: None,
        },
        ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("email")),
            alias: None,
        },
        ProjectionColumn {
            expr: Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(1))),
            },
            alias: Some("next_age".to_owned()),
        },
    ])
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    group.bench_function("scan", |b| {
        let expr = simple_scan();
        b.iter(|| {
            let rec = to_rec_expr(black_box(&expr)).expect("conversion should succeed");
            rec_expr_to_rel_expr(black_box(&rec)).expect("back-conversion should succeed")
        });
    });

    group.bench_function("filtered_join", |b| {
        let expr = filtered_join();
        b.iter(|| {
            let rec = to_rec_expr(black_box(&expr)).expect("conversion should succeed");
            rec_expr_to_rel_expr(black_box(&rec)).expect("back-conversion should succeed")
        });
    });

    group.bench_function("complex", |b| {
        let expr = complex_query();
        b.iter(|| {
            let rec = to_rec_expr(black_box(&expr)).expect("conversion should succeed");
            rec_expr_to_rel_expr(black_box(&rec)).expect("back-conversion should succeed")
        });
    });

    group.finish();
}

fn bench_optimization(c: &mut Criterion) {
    let optimizer = Optimizer::with_config(OptimizerConfig {
        node_limit: 100_000,
        iter_limit: 30,
        time_limit_secs: 10,
    });

    let mut group = c.benchmark_group("optimize");

    group.bench_function("scan", |b| {
        let expr = simple_scan();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("filtered_scan", |b| {
        let expr = filtered_scan();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("two_table_join", |b| {
        let expr = two_table_join();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("three_table_join", |b| {
        let expr = three_table_join();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("filtered_join", |b| {
        let expr = filtered_join();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("aggregate", |b| {
        let expr = aggregate_query();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("project", |b| {
        let expr = project_query();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("complex", |b| {
        let expr = complex_query();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.finish();
}

// ── TPC-H Subset Queries ────────────────────────────────────────────

/// TPC-H Q1: Pricing Summary Report
fn tpch_q1() -> RelExpr {
    // SELECT l_returnflag, l_linestatus, sum(l_quantity), sum(l_extendedprice)
    // FROM lineitem
    // WHERE l_shipdate <= date '1998-12-01' - interval '90' day
    // GROUP BY l_returnflag, l_linestatus
    RelExpr::Aggregate {
        group_by: vec![
            Expr::Column(ColumnRef::new("l_returnflag")),
            Expr::Column(ColumnRef::new("l_linestatus")),
        ],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("l_quantity"))),
                distinct: false,
                alias: Some("sum_qty".to_owned()),
            },
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("l_extendedprice"))),
                distinct: false,
                alias: Some("sum_price".to_owned()),
            },
        ],
        input: Box::new(RelExpr::scan("lineitem").filter(Expr::BinOp {
            op: BinOp::Le,
            left: Box::new(Expr::Column(ColumnRef::new("l_shipdate"))),
            right: Box::new(Expr::Const(Const::String("1998-09-02".to_owned()))),
        })),
    }
}

/// TPC-H Q3: Shipping Priority
fn tpch_q3() -> RelExpr {
    // SELECT l_orderkey, sum(l_extendedprice * (1 - l_discount))
    // FROM customer, orders, lineitem
    // WHERE c_custkey = o_custkey AND l_orderkey = o_orderkey
    //   AND c_mktsegment = 'BUILDING' AND o_orderdate < '1995-03-15'
    // GROUP BY l_orderkey
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("l_orderkey"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("l_extendedprice"))),
            distinct: false,
            alias: Some("revenue".to_owned()),
        }],
        input: Box::new(
            RelExpr::Join {
                join_type: JoinType::Inner,
                condition: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::new("l_orderkey"))),
                    right: Box::new(Expr::Column(ColumnRef::new("o_orderkey"))),
                },
                left: Box::new(
                    RelExpr::Join {
                        join_type: JoinType::Inner,
                        condition: Expr::BinOp {
                            op: BinOp::Eq,
                            left: Box::new(Expr::Column(ColumnRef::new("c_custkey"))),
                            right: Box::new(Expr::Column(ColumnRef::new("o_custkey"))),
                        },
                        left: Box::new(RelExpr::scan("customer").filter(Expr::BinOp {
                            op: BinOp::Eq,
                            left: Box::new(Expr::Column(ColumnRef::new("c_mktsegment"))),
                            right: Box::new(Expr::Const(Const::String("BUILDING".to_owned()))),
                        })),
                        right: Box::new(RelExpr::scan("orders").filter(Expr::BinOp {
                            op: BinOp::Lt,
                            left: Box::new(Expr::Column(ColumnRef::new("o_orderdate"))),
                            right: Box::new(Expr::Const(Const::String("1995-03-15".to_owned()))),
                        })),
                    }
                ),
                right: Box::new(RelExpr::scan("lineitem")),
            }
        ),
    }
}

/// TPC-H Q6: Forecasting Revenue Change
fn tpch_q6() -> RelExpr {
    // SELECT sum(l_extendedprice * l_discount)
    // FROM lineitem
    // WHERE l_shipdate >= '1994-01-01' AND l_shipdate < '1995-01-01'
    //   AND l_discount BETWEEN 0.05 AND 0.07 AND l_quantity < 24
    RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("l_extendedprice"))),
            distinct: false,
            alias: Some("revenue".to_owned()),
        }],
        input: Box::new(RelExpr::scan("lineitem").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::BinOp {
                    op: BinOp::Ge,
                    left: Box::new(Expr::Column(ColumnRef::new("l_shipdate"))),
                    right: Box::new(Expr::Const(Const::String("1994-01-01".to_owned()))),
                }),
                right: Box::new(Expr::BinOp {
                    op: BinOp::Lt,
                    left: Box::new(Expr::Column(ColumnRef::new("l_shipdate"))),
                    right: Box::new(Expr::Const(Const::String("1995-01-01".to_owned()))),
                }),
            }),
            right: Box::new(Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(Expr::Column(ColumnRef::new("l_quantity"))),
                right: Box::new(Expr::Const(Const::Int(24))),
            }),
        })),
    }
}

// ── Hardware-specific benchmarks ────────────────────────────────────

fn bench_tpch_subset(c: &mut Criterion) {
    let optimizer = Optimizer::new();
    let mut group = c.benchmark_group("tpch");

    group.bench_function("q1_pricing_summary", |b| {
        let expr = tpch_q1();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("q3_shipping_priority", |b| {
        let expr = tpch_q3();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.bench_function("q6_forecasting_revenue", |b| {
        let expr = tpch_q6();
        b.iter(|| {
            optimizer
                .optimize(black_box(&expr))
                .expect("optimization should succeed")
        });
    });

    group.finish();
}

fn bench_hardware_aware(c: &mut Criterion) {
    use ra_hardware::HardwareProfile;

    let profiles = vec![
        ("auto", ra_hardware::detect_hardware()),
        ("cpu-only", HardwareProfile::cpu_only()),
        ("gpu-server", HardwareProfile::gpu_server()),
    ];

    let mut group = c.benchmark_group("hardware");

    for (name, profile) in profiles {
        let mut optimizer = Optimizer::new();
        optimizer.set_hardware_profile(profile);

        group.bench_function(format!("join_{name}"), |b| {
            let expr = two_table_join();
            b.iter(|| {
                optimizer
                    .optimize(black_box(&expr))
                    .expect("optimization should succeed")
            });
        });

        group.bench_function(format!("aggregate_{name}"), |b| {
            let expr = aggregate_query();
            b.iter(|| {
                optimizer
                    .optimize(black_box(&expr))
                    .expect("optimization should succeed")
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_roundtrip,
    bench_optimization,
    bench_tpch_subset,
    bench_hardware_aware
);
criterion_main!(benches);
