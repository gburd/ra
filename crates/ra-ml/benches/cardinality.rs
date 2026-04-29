//! Benchmarks for cardinality estimation inference latency.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_ml::estimator::{
    CardinalityEstimator, HeuristicEstimator, MlEstimator, SimpleStatsProvider,
};

fn setup_provider() -> SimpleStatsProvider {
    let mut provider = SimpleStatsProvider::new();
    provider.add("users", Statistics::new(10_000.0));
    provider.add("orders", Statistics::new(50_000.0));
    provider.add("products", Statistics::new(5_000.0));
    provider.add("line_items", Statistics::new(200_000.0));
    provider
}

fn simple_scan() -> RelExpr {
    RelExpr::scan("users")
}

fn filtered_scan() -> RelExpr {
    RelExpr::scan("orders").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("amount"))),
        right: Box::new(Expr::Const(Const::Int(100))),
    })
}

fn two_way_join() -> RelExpr {
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

fn three_way_join() -> RelExpr {
    let users_orders = two_way_join();
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("orders", "product_id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("products", "id"))),
        },
        left: Box::new(users_orders),
        right: Box::new(RelExpr::scan("products")),
    }
}

fn bench_heuristic(c: &mut Criterion) {
    let estimator = HeuristicEstimator;
    let provider = setup_provider();

    let mut group = c.benchmark_group("heuristic");
    group.bench_function("scan", |b| {
        let expr = simple_scan();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.bench_function("filter", |b| {
        let expr = filtered_scan();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.bench_function("2-way-join", |b| {
        let expr = two_way_join();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.bench_function("3-way-join", |b| {
        let expr = three_way_join();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.finish();
}

fn bench_ml(c: &mut Criterion) {
    let estimator = MlEstimator::with_default_model(
        &["users", "orders", "products", "line_items"],
        &["id", "name", "amount", "user_id", "product_id"],
    );
    let provider = setup_provider();

    let mut group = c.benchmark_group("ml");
    group.bench_function("scan", |b| {
        let expr = simple_scan();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.bench_function("filter", |b| {
        let expr = filtered_scan();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.bench_function("2-way-join", |b| {
        let expr = two_way_join();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.bench_function("3-way-join", |b| {
        let expr = three_way_join();
        b.iter(|| estimator.estimate(black_box(&expr), &provider));
    });
    group.finish();
}

criterion_group!(benches, bench_heuristic, bench_ml);
criterion_main!(benches);
