//! Benchmarks for resource-bounded optimization.
//!
//! Measures latency under different resource profiles to verify
//! performance targets:
//! - Interactive: <100ms
//! - Standard: <1s
//! - Memory-constrained: stays under 10MB
//! - Resource tracking overhead: <5%
//! - Color rendering overhead: <1ms

#![allow(clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{Optimizer, ResourceBudget};

// ── Query fixtures ──────────────────────────────────────────

fn simple_scan() -> RelExpr {
    RelExpr::scan("lineitem")
}

fn filtered_scan() -> RelExpr {
    RelExpr::scan("lineitem").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("l_quantity"))),
        right: Box::new(Expr::Const(Const::Int(10))),
    })
}

fn two_table_join() -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("orders", "o_orderkey"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("lineitem", "l_orderkey"))),
        },
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("lineitem")),
    }
}

fn tpch_q1() -> RelExpr {
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

fn tpch_q3() -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("l_orderkey"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("l_extendedprice"))),
            distinct: false,
            alias: Some("revenue".to_owned()),
        }],
        input: Box::new(RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("l_orderkey"))),
                right: Box::new(Expr::Column(ColumnRef::new("o_orderkey"))),
            },
            left: Box::new(RelExpr::Join {
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
            }),
            right: Box::new(RelExpr::scan("lineitem")),
        }),
    }
}

fn tpch_q6() -> RelExpr {
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
                op: BinOp::Ge,
                left: Box::new(Expr::Column(ColumnRef::new("l_shipdate"))),
                right: Box::new(Expr::Const(Const::String("1994-01-01".to_owned()))),
            }),
            right: Box::new(Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(Expr::Column(ColumnRef::new("l_quantity"))),
                right: Box::new(Expr::Const(Const::Int(24))),
            }),
        })),
    }
}

// ── Interactive profile benchmarks (<100ms target) ──────────

fn bench_interactive(c: &mut Criterion) {
    let mut group = c.benchmark_group("interactive");
    group.sample_size(50);

    let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::interactive());

    group.bench_function("simple_scan", |b| {
        let expr = simple_scan();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("filtered_scan", |b| {
        let expr = filtered_scan();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("two_table_join", |b| {
        let expr = two_table_join();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("tpch_q1", |b| {
        let expr = tpch_q1();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("tpch_q3", |b| {
        let expr = tpch_q3();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("tpch_q6", |b| {
        let expr = tpch_q6();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

// ── Standard profile benchmarks (<1s target) ────────────────

fn bench_standard(c: &mut Criterion) {
    let mut group = c.benchmark_group("standard");
    group.sample_size(20);

    let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());

    group.bench_function("tpch_q1", |b| {
        let expr = tpch_q1();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("tpch_q3", |b| {
        let expr = tpch_q3();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("tpch_q6", |b| {
        let expr = tpch_q6();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

// ── Memory-constrained benchmarks ───────────────────────────

fn bench_memory_constrained(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_constrained");
    group.sample_size(20);

    let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::memory_constrained());

    group.bench_function("filtered_scan", |b| {
        let expr = filtered_scan();
        b.iter(|| {
            let result = optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed");
            // Verify memory stays under 10MB
            assert!(
                result.resource_usage.peak_memory_estimate <= 10 * 1024 * 1024,
                "memory exceeded 10MB"
            );
            result
        });
    });

    group.bench_function("tpch_q1", |b| {
        let expr = tpch_q1();
        b.iter(|| {
            let result = optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed");
            assert!(
                result.resource_usage.peak_memory_estimate <= 10 * 1024 * 1024,
                "memory exceeded 10MB"
            );
            result
        });
    });

    group.finish();
}

// ── Tracking overhead benchmarks ────────────────────────────

fn bench_tracking_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("tracking_overhead");
    group.sample_size(30);

    let unbounded = Optimizer::new();
    let bounded = Optimizer::new().with_resource_budget(ResourceBudget::unlimited());

    group.bench_function("unbounded_tpch_q1", |b| {
        let expr = tpch_q1();
        b.iter(|| {
            unbounded
                .optimize(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("bounded_unlimited_tpch_q1", |b| {
        let expr = tpch_q1();
        b.iter(|| {
            bounded
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_interactive,
    bench_standard,
    bench_memory_constrained,
    bench_tracking_overhead,
);
criterion_main!(benches);
