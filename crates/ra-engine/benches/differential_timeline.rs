//! Benchmarks comparing full vs incremental optimization.
//!
//! Measures the speedup from differential statistics updates
//! for various delta sizes (small, medium, large).

#![allow(clippy::expect_used)]

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion,
};
use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{Optimizer, OptimizerConfig, egraph::ParallelConfig};
use ra_stats::delta::DeltaSet;
use ra_stats::timeline::{ColumnSnapshot, Snapshot, TableSnapshot};

fn make_snapshot(
    time: u64,
    row_count: u64,
    ndv: u64,
) -> Snapshot {
    Snapshot {
        time_offset: time,
        label: None,
        tables: vec![
            TableSnapshot {
                name: "users".to_string(),
                row_count,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![
                    ColumnSnapshot {
                        name: "id".to_string(),
                        ndv,
                        null_fraction: 0.0,
                        avg_width: 8.0,
                        correlation: Some(1.0),
                        min_value: None,
                        max_value: None,
                    },
                    ColumnSnapshot {
                        name: "age".to_string(),
                        ndv: 80,
                        null_fraction: 0.01,
                        avg_width: 4.0,
                        correlation: None,
                        min_value: None,
                        max_value: None,
                    },
                ],
            },
            TableSnapshot {
                name: "orders".to_string(),
                row_count: row_count * 5,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![ColumnSnapshot {
                    name: "user_id".to_string(),
                    ndv,
                    null_fraction: 0.0,
                    avg_width: 8.0,
                    correlation: Some(0.7),
                    min_value: None,
                    max_value: None,
                }],
            },
        ],
    }
}

fn join_query() -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(
                ColumnRef::qualified("users", "id"),
            )),
            right: Box::new(Expr::Column(
                ColumnRef::qualified("orders", "user_id"),
            )),
        },
        left: Box::new(
            RelExpr::scan("users").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
        ),
        right: Box::new(RelExpr::scan("orders")),
    }
}

fn bench_full_vs_incremental(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_vs_incremental");

    let base = make_snapshot(0, 100_000, 100_000);
    let query = join_query();
    let config = OptimizerConfig {
        node_limit: 10_000,
        iter_limit: 10,
        time_limit_secs: 5,
        large_join_threshold: 10,
        large_join_strategy: ra_engine::large_join::LargeJoinStrategy::Greedy,
        max_optimization_time_ms: 5000,
        parallel: ParallelConfig::default(),
    };

    // Small change: 1% row count increase
    let small_next = make_snapshot(60, 101_000, 101_000);
    let small_delta = DeltaSet::compute(&base, &small_next);

    // Medium change: 10% row count increase
    let medium_next = make_snapshot(120, 110_000, 110_000);
    let medium_delta = DeltaSet::compute(&base, &medium_next);

    // Large change: 50% row count increase
    let large_next = make_snapshot(180, 150_000, 150_000);
    let large_delta = DeltaSet::compute(&base, &large_next);

    group.bench_function("full_optimization", |b| {
        b.iter(|| {
            let optimizer = Optimizer::with_config(config.clone());
            optimizer.optimize(black_box(&query)).expect("optimize");
        });
    });

    for (label, delta) in [
        ("incremental_1pct", &small_delta),
        ("incremental_10pct", &medium_delta),
        ("incremental_50pct", &large_delta),
    ] {
        group.bench_function(
            BenchmarkId::from_parameter(label),
            |b| {
                b.iter(|| {
                    let mut optimizer =
                        Optimizer::with_config(config.clone());
                    optimizer
                        .optimize_incremental(
                            black_box(&query),
                            black_box(delta),
                        )
                        .expect("incremental");
                });
            },
        );
    }

    group.finish();
}

fn bench_delta_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("delta_computation");

    let base = make_snapshot(0, 100_000, 100_000);

    for pct in [1, 5, 10, 25, 50] {
        #[allow(clippy::cast_sign_loss)]
        let new_rows = 100_000 + (100_000 * pct / 100);
        let next = make_snapshot(60, new_rows, new_rows);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{pct}pct")),
            &pct,
            |b, _| {
                b.iter(|| {
                    DeltaSet::compute(
                        black_box(&base),
                        black_box(&next),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_full_vs_incremental,
    bench_delta_computation
);
criterion_main!(benches);
