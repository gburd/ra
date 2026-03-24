//! OLTP plan cache benchmark.
//!
//! Simulates a typical OLTP workload with 5 query templates and
//! 200 total queries (parameter variations). Measures:
//! - Optimization latency with and without plan caching
//! - Cache hit rate
//! - Throughput (queries/second)

#![allow(clippy::expect_used)]

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId,
    Criterion,
};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, RelExpr,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{
    Optimizer, OptimizerConfig, PlanCacheConfig,
};

// ── Query templates ─────────────────────────────────────────────

/// Template 1: Point lookup by primary key.
/// `SELECT * FROM users WHERE id = ?`
fn point_lookup(user_id: i64) -> RelExpr {
    RelExpr::scan("users").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("id"))),
        right: Box::new(Expr::Const(Const::Int(user_id))),
    })
}

/// Template 2: Range scan with filter.
/// `SELECT * FROM orders WHERE amount > ? AND status = ?`
fn range_scan(threshold: i64, status: &str) -> RelExpr {
    RelExpr::scan("orders").filter(Expr::BinOp {
        op: BinOp::And,
        left: Box::new(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("amount"))),
            right: Box::new(Expr::Const(Const::Int(threshold))),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("status"))),
            right: Box::new(Expr::Const(Const::String(
                status.to_owned(),
            ))),
        }),
    })
}

/// Template 3: Two-table join with filter.
/// `SELECT * FROM users JOIN orders ON users.id = orders.user_id
///  WHERE users.age > ?`
fn join_with_filter(age: i64) -> RelExpr {
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
        left: Box::new(RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(age))),
        })),
        right: Box::new(RelExpr::scan("orders")),
    }
}

/// Template 4: Aggregation query.
/// `SELECT dept, COUNT(*) FROM employees WHERE salary > ?
///  GROUP BY dept`
fn aggregation(salary_threshold: i64) -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("dept"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: None,
            distinct: false,
            alias: Some("cnt".to_owned()),
        }],
        input: Box::new(
            RelExpr::scan("employees").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new(
                    "salary",
                ))),
                right: Box::new(Expr::Const(Const::Int(
                    salary_threshold,
                ))),
            }),
        ),
    }
}

/// Template 5: Three-table join (user -> orders -> products).
/// `SELECT * FROM users
///  JOIN orders ON users.id = orders.user_id
///  JOIN products ON orders.product_id = products.id
///  WHERE products.price > ?`
fn three_table_join(price: i64) -> RelExpr {
    let user_orders = RelExpr::Join {
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
        left: Box::new(RelExpr::scan("users")),
        right: Box::new(RelExpr::scan("orders")),
    };
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(
                ColumnRef::qualified("orders", "product_id"),
            )),
            right: Box::new(Expr::Column(
                ColumnRef::qualified("products", "id"),
            )),
        },
        left: Box::new(user_orders),
        right: Box::new(
            RelExpr::scan("products").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new(
                    "price",
                ))),
                right: Box::new(Expr::Const(Const::Int(price))),
            }),
        ),
    }
}

/// Generate a workload of 200 queries from 5 templates.
fn oltp_workload() -> Vec<RelExpr> {
    let mut queries = Vec::with_capacity(200);
    let statuses = ["active", "pending", "shipped", "returned"];

    for i in 0..40 {
        queries.push(point_lookup(i * 7 + 1));
    }
    for i in 0..40 {
        let status = statuses[i as usize % statuses.len()];
        queries.push(range_scan(i * 50 + 100, status));
    }
    for i in 0..40 {
        queries.push(join_with_filter(18 + i));
    }
    for i in 0..40 {
        queries.push(aggregation(30000 + i * 1000));
    }
    for i in 0..40 {
        queries.push(three_table_join(10 + i * 5));
    }
    queries
}

// ── Benchmarks ──────────────────────────────────────────────────

fn bench_oltp_no_cache(c: &mut Criterion) {
    let optimizer = Optimizer::new();
    let workload = oltp_workload();

    c.bench_function("oltp_no_cache_200q", |b| {
        b.iter(|| {
            for q in &workload {
                let _ = optimizer.optimize(black_box(q));
            }
        });
    });
}

fn bench_oltp_with_cache(c: &mut Criterion) {
    let config = OptimizerConfig {
        enable_plan_cache: true,
        plan_cache_config: PlanCacheConfig {
            max_entries: 1024,
            similarity_threshold: 0.9,
            enable_fuzzy_matching: true,
        },
        ..OptimizerConfig::default()
    };
    let optimizer = Optimizer::with_config(config);
    let workload = oltp_workload();

    c.bench_function("oltp_with_cache_200q", |b| {
        b.iter(|| {
            // Clear the cache before each iteration so we
            // measure cold-start + warm behavior together.
            optimizer.clear_cache();

            for q in &workload {
                let _ = optimizer.optimize(black_box(q));
            }
        });
    });
}

fn bench_cache_hit_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("plan_cache_hit_rate");

    for &template_count in &[1_usize, 3, 5] {
        let config = OptimizerConfig {
            enable_plan_cache: true,
            plan_cache_config: PlanCacheConfig::default(),
            ..OptimizerConfig::default()
        };
        let optimizer = Optimizer::with_config(config);

        group.bench_with_input(
            BenchmarkId::new("templates", template_count),
            &template_count,
            |b, &tc| {
                b.iter(|| {
                    optimizer.clear_cache();
                    let mut workload = Vec::with_capacity(200);
                    for i in 0..200_i64 {
                        let q = match (i as usize) % tc {
                            0 => point_lookup(i),
                            1 => range_scan(i * 10, "active"),
                            2 => join_with_filter(18 + i),
                            3 => aggregation(50000 + i * 100),
                            _ => three_table_join(i * 5),
                        };
                        workload.push(q);
                    }
                    for q in &workload {
                        let _ = optimizer.optimize(black_box(q));
                    }

                    // Return stats for verification
                    optimizer.cache_stats()
                });
            },
        );
    }

    group.finish();
}

fn bench_cached_lookup_latency(c: &mut Criterion) {
    let config = OptimizerConfig {
        enable_plan_cache: true,
        plan_cache_config: PlanCacheConfig::default(),
        ..OptimizerConfig::default()
    };
    let optimizer = Optimizer::with_config(config);

    // Prime the cache with one query from each template
    let _ = optimizer.optimize(&point_lookup(42));
    let _ = optimizer.optimize(&range_scan(500, "active"));
    let _ = optimizer.optimize(&join_with_filter(25));
    let _ = optimizer.optimize(&aggregation(60000));
    let _ = optimizer.optimize(&three_table_join(100));

    // Now measure cache-hit latency with parameter variations
    let queries: Vec<RelExpr> = (0..100)
        .map(|i| match i % 5 {
            0 => point_lookup(i * 3),
            1 => range_scan(i * 20, "active"),
            2 => join_with_filter(18 + i),
            3 => aggregation(40000 + i * 500),
            _ => three_table_join(i * 7),
        })
        .collect();

    c.bench_function("cached_lookup_100q", |b| {
        b.iter(|| {
            for q in &queries {
                let _ = optimizer.optimize(black_box(q));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_oltp_no_cache,
    bench_oltp_with_cache,
    bench_cache_hit_rate,
    bench_cached_lookup_latency,
);
criterion_main!(benches);
