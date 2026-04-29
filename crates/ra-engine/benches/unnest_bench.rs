//! Performance benchmarks for UNNEST operations.
//!
//! Measures execution throughput for various UNNEST patterns:
//! - Literal array unnest (single and multi-arg)
//! - Comparison with VALUES clause
//! - Lateral unnest performance

#![expect(clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ra_core::expr::{Const, Expr};
use ra_engine::executors::lateral_join::{LateralJoinExecutor, LateralRhs};
use ra_engine::executors::table_function::TableFunctionExecutor;
use ra_engine::executors::unnest::{MultiUnnestExecutor, UnnestExecutor};
use ra_engine::Row;

fn make_int_array(n: usize) -> Expr {
    let elements: Vec<Expr> = (1..=n).map(|i| Expr::Const(Const::Int(i as i64))).collect();
    Expr::Array(elements)
}

fn bench_unnest_literal(c: &mut Criterion) {
    let mut group = c.benchmark_group("unnest_literal");

    for size in [10, 100, 1000] {
        group.bench_function(format!("array_{size}"), |b| {
            let expr = make_int_array(size);
            let executor = UnnestExecutor::new(expr, None, false);
            b.iter(|| {
                black_box(executor.execute(None)).expect("should succeed");
            });
        });
    }

    group.finish();
}

fn bench_unnest_with_ordinality(c: &mut Criterion) {
    let mut group = c.benchmark_group("unnest_with_ordinality");

    for size in [10, 100, 1000] {
        group.bench_function(format!("array_{size}"), |b| {
            let expr = make_int_array(size);
            let executor = UnnestExecutor::new(expr, None, true);
            b.iter(|| {
                black_box(executor.execute(None)).expect("should succeed");
            });
        });
    }

    group.finish();
}

fn bench_unnest_vs_generate_series(c: &mut Criterion) {
    let mut group = c.benchmark_group("unnest_vs_generate_series");

    group.bench_function("unnest_1000", |b| {
        let expr = make_int_array(1000);
        let executor = UnnestExecutor::new(expr, None, false);
        b.iter(|| {
            black_box(executor.execute(None)).expect("should succeed");
        });
    });

    group.bench_function("generate_series_1000", |b| {
        let exec = TableFunctionExecutor::new(
            "generate_series",
            vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(1000))],
        );
        b.iter(|| {
            black_box(exec.execute(None)).expect("should succeed");
        });
    });

    group.finish();
}

fn bench_multi_unnest(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_unnest");

    for num_arrays in [2, 5, 10] {
        group.bench_function(format!("{num_arrays}_arrays_100"), |b| {
            let exprs: Vec<Expr> = (0..num_arrays).map(|_| make_int_array(100)).collect();
            let aliases: Vec<Option<String>> = (0..num_arrays).map(|_| None).collect();
            let executor = MultiUnnestExecutor::new(exprs, aliases, false);
            b.iter(|| {
                black_box(executor.execute()).expect("should succeed");
            });
        });
    }

    group.finish();
}

fn bench_lateral_unnest(c: &mut Criterion) {
    let mut group = c.benchmark_group("lateral_unnest");

    for outer_rows in [10, 100] {
        group.bench_function(format!("{outer_rows}_outer_rows"), |b| {
            let left_rows: Vec<Row> = (0..outer_rows)
                .map(|i| {
                    Row::new(vec![
                        Const::Int(i64::from(i)),
                        Const::String("{1,2,3,4,5}".into()),
                    ])
                })
                .collect();

            let unnest = UnnestExecutor::new(
                Expr::Column(ra_core::expr::ColumnRef::new("arr")),
                None,
                false,
            );
            let executor = LateralJoinExecutor::new(LateralRhs::Unnest(unnest), false);

            b.iter(|| {
                black_box(executor.execute(&left_rows)).expect("should succeed");
            });
        });
    }

    group.finish();
}

fn bench_json_array_elements(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_array_elements");

    for size in [10, 100, 1000] {
        group.bench_function(format!("elements_{size}"), |b| {
            let elements: Vec<String> = (1..=size).map(|i| i.to_string()).collect();
            let json = format!("[{}]", elements.join(","));
            let exec = TableFunctionExecutor::new(
                "json_array_elements",
                vec![Expr::Const(Const::String(json))],
            );
            b.iter(|| {
                black_box(exec.execute(None)).expect("should succeed");
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_unnest_literal,
    bench_unnest_with_ordinality,
    bench_unnest_vs_generate_series,
    bench_multi_unnest,
    bench_lateral_unnest,
    bench_json_array_elements,
);
criterion_main!(benches);
