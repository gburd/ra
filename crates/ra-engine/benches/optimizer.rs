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

criterion_group!(benches, bench_roundtrip, bench_optimization);
criterion_main!(benches);
