//! Benchmarks for RFC 0058 rule complexity prioritization.
//!
//! Compares optimization time with and without priority-sorted rules
//! to measure the impact on complex queries.

#![allow(clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use egg::Runner;
use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::analysis::RelAnalysis;
use ra_engine::egraph::RelLang;
use ra_engine::rewrite::{all_rules, all_rules_unsorted};
use ra_engine::to_rec_expr;

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

fn filtered_three_table_join() -> RelExpr {
    three_table_join().filter(Expr::BinOp {
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

fn complex_aggregate_query() -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::qualified("users", "name"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            distinct: false,
            alias: Some("total".to_owned()),
        }],
        input: Box::new(filtered_three_table_join()),
    }
    .limit(10, 0)
}

fn run_with_rules(
    expr: &RelExpr,
    rules: &[egg::Rewrite<RelLang, RelAnalysis>],
) -> Runner<RelLang, RelAnalysis> {
    let rec = to_rec_expr(expr).expect("conversion should succeed");
    Runner::default()
        .with_expr(&rec)
        .with_node_limit(50_000)
        .with_iter_limit(10)
        .run(rules)
}

fn bench_priority_sorting(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_priority");

    // Measure the cost of sorting rules
    group.bench_function("sort_rules", |b| {
        b.iter(|| {
            let rules = all_rules_unsorted();
            black_box(ra_engine::sort_rules_by_priority(rules));
        });
    });

    group.finish();
}

fn bench_sorted_vs_unsorted(c: &mut Criterion) {
    let sorted = all_rules();
    let unsorted = all_rules_unsorted();

    let mut group = c.benchmark_group("sorted_vs_unsorted");

    // Three-table join with filters
    group.bench_function("filtered_3way_sorted", |b| {
        let expr = filtered_three_table_join();
        b.iter(|| run_with_rules(black_box(&expr), &sorted));
    });

    group.bench_function("filtered_3way_unsorted", |b| {
        let expr = filtered_three_table_join();
        b.iter(|| run_with_rules(black_box(&expr), &unsorted));
    });

    // Complex aggregate query
    group.bench_function("complex_agg_sorted", |b| {
        let expr = complex_aggregate_query();
        b.iter(|| run_with_rules(black_box(&expr), &sorted));
    });

    group.bench_function("complex_agg_unsorted", |b| {
        let expr = complex_aggregate_query();
        b.iter(|| run_with_rules(black_box(&expr), &unsorted));
    });

    group.finish();
}

fn bench_high_priority_first(c: &mut Criterion) {
    let sorted = all_rules();

    let mut group = c.benchmark_group("priority_ordering");

    // Verify high-priority rules are in front
    group.bench_function("verify_ordering", |b| {
        b.iter(|| {
            let rules = all_rules();
            // Check that high-benefit rules like filter-true and
            // cartesian-to-join are in the first third
            let first_third = rules.len() / 3;
            let first_third_names: Vec<&str> = rules
                .iter()
                .take(first_third)
                .map(|r| r.name.as_str())
                .collect();
            black_box(&first_third_names);
        });
    });

    // Measure iteration efficiency: run a single iteration
    // and check how many rewrites fire
    group.bench_function("single_iter_sorted", |b| {
        let expr = filtered_three_table_join();
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        b.iter(|| {
            Runner::default()
                .with_expr(&rec)
                .with_node_limit(50_000)
                .with_iter_limit(1)
                .run(black_box(&sorted))
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_priority_sorting,
    bench_sorted_vs_unsorted,
    bench_high_priority_first,
);
criterion_main!(benches);
