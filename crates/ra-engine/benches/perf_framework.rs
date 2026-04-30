#![expect(clippy::expect_used)]
//! Comprehensive performance framework benchmarks.
//!
//! Validates optimizer performance targets in release mode:
//! - Simple OLTP: <1ms
//! - Medium OLTP: <10ms
//! - Complex OLAP: <100ms
//!
//! Also benchmarks TPC-C (TPROC-C) and TPC-H (TPROC-H) workloads,
//! dynamic budget switching overhead, and A/B optimizer comparisons.
//!
//! Run with:
//!   `cargo bench --package ra-engine --bench perf_framework`

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion,
};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, RelExpr,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_engine::{Optimizer, ResourceBudget};

// ── Helpers ─────────────────────────────────────────────────

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn qcol(table: &str, name: &str) -> Expr {
    Expr::Column(ColumnRef::qualified(table, name))
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

fn le(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Le,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn ge(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Ge,
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

fn int(v: i64) -> Expr {
    Expr::Const(Const::Int(v))
}

fn str_const(v: &str) -> Expr {
    Expr::Const(Const::String(v.into()))
}

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan {
        table: name.to_string(),
        alias: None,
    }
}

fn join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: cond,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn filter(input: RelExpr, pred: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate: pred,
        input: Box::new(input),
    }
}

fn aggregate(
    input: RelExpr,
    group_by: Vec<Expr>,
    aggregates: Vec<AggregateExpr>,
) -> RelExpr {
    RelExpr::Aggregate {
        input: Box::new(input),
        group_by,
        aggregates,
    }
}

fn sum_col(name: &str) -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Sum,
        arg: Some(col(name)),
        distinct: false,
        alias: None,
    }
}

fn count_star() -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
        alias: None,
    }
}

fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

fn make_tpch_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
    for (name, stats) in [
        ("lineitem", make_stats(6_001_215.0, 128)),
        ("orders", make_stats(1_500_000.0, 150)),
        ("customer", make_stats(150_000.0, 200)),
        ("supplier", make_stats(10_000.0, 180)),
        ("nation", make_stats(25.0, 64)),
        ("region", make_stats(5.0, 48)),
        ("part", make_stats(200_000.0, 160)),
        ("partsupp", make_stats(800_000.0, 144)),
    ] {
        opt.add_table_stats(name, stats);
    }
    opt
}

fn make_tpcc_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
    for (name, stats) in [
        ("warehouse", make_stats(10.0, 64)),
        ("district", make_stats(100.0, 96)),
        ("customer", make_stats(30_000.0, 256)),
        ("orders", make_stats(300_000.0, 48)),
        ("new_order", make_stats(90_000.0, 16)),
        ("order_line", make_stats(3_000_000.0, 64)),
        ("stock", make_stats(100_000.0, 128)),
        ("item", make_stats(100_000.0, 80)),
    ] {
        opt.add_table_stats(name, stats);
    }
    opt
}

// ── Query fixtures ──────────────────────────────────────────

fn oltp_point_lookup() -> RelExpr {
    filter(scan("orders"), eq(col("o_orderkey"), int(42)))
}

fn oltp_filtered_scan() -> RelExpr {
    filter(scan("lineitem"), gt(col("l_quantity"), int(10)))
}

fn oltp_two_table_join() -> RelExpr {
    join(
        scan("orders"),
        scan("lineitem"),
        eq(
            qcol("orders", "o_orderkey"),
            qcol("lineitem", "l_orderkey"),
        ),
    )
}

fn oltp_three_table_join() -> RelExpr {
    join(
        join(
            filter(
                scan("customer"),
                eq(col("c_mktsegment"), str_const("BUILDING")),
            ),
            scan("orders"),
            eq(
                qcol("customer", "c_custkey"),
                qcol("orders", "o_custkey"),
            ),
        ),
        scan("lineitem"),
        eq(
            qcol("orders", "o_orderkey"),
            qcol("lineitem", "l_orderkey"),
        ),
    )
}

fn olap_tpch_q1() -> RelExpr {
    aggregate(
        filter(
            scan("lineitem"),
            le(col("l_shipdate"), str_const("1998-09-02")),
        ),
        vec![col("l_returnflag"), col("l_linestatus")],
        vec![sum_col("l_quantity"), sum_col("l_extendedprice")],
    )
}

fn olap_tpch_q3() -> RelExpr {
    aggregate(
        join(
            join(
                filter(
                    scan("customer"),
                    eq(col("c_mktsegment"), str_const("BUILDING")),
                ),
                filter(
                    scan("orders"),
                    lt(col("o_orderdate"), str_const("1995-03-15")),
                ),
                eq(col("c_custkey"), col("o_custkey")),
            ),
            scan("lineitem"),
            eq(col("l_orderkey"), col("o_orderkey")),
        ),
        vec![col("l_orderkey")],
        vec![sum_col("l_extendedprice")],
    )
}

fn olap_tpch_q5() -> RelExpr {
    aggregate(
        join(
            join(
                join(
                    join(
                        join(
                            filter(
                                scan("region"),
                                eq(col("r_name"), str_const("ASIA")),
                            ),
                            scan("nation"),
                            eq(col("r_regionkey"), col("n_regionkey")),
                        ),
                        scan("supplier"),
                        eq(col("n_nationkey"), col("s_nationkey")),
                    ),
                    scan("customer"),
                    eq(col("n_nationkey"), col("c_nationkey")),
                ),
                scan("orders"),
                eq(col("c_custkey"), col("o_custkey")),
            ),
            scan("lineitem"),
            and(
                eq(col("l_orderkey"), col("o_orderkey")),
                eq(col("l_suppkey"), col("s_suppkey")),
            ),
        ),
        vec![col("n_name")],
        vec![sum_col("l_extendedprice")],
    )
}

fn tproc_c_new_order() -> RelExpr {
    join(
        filter(scan("warehouse"), eq(col("w_id"), int(1))),
        filter(scan("district"), eq(col("d_w_id"), int(1))),
        eq(col("w_id"), col("d_w_id")),
    )
}

fn tproc_c_stock_level() -> RelExpr {
    aggregate(
        filter(
            join(
                join(
                    filter(
                        scan("district"),
                        and(
                            eq(col("d_w_id"), int(1)),
                            eq(col("d_id"), int(5)),
                        ),
                    ),
                    scan("order_line"),
                    eq(col("d_id"), col("ol_d_id")),
                ),
                scan("stock"),
                eq(col("ol_i_id"), col("s_i_id")),
            ),
            lt(col("s_quantity"), int(20)),
        ),
        vec![],
        vec![count_star()],
    )
}

fn tproc_h_q6() -> RelExpr {
    aggregate(
        filter(
            scan("lineitem"),
            and(
                ge(col("l_shipdate"), str_const("1994-01-01")),
                lt(col("l_quantity"), int(24)),
            ),
        ),
        vec![],
        vec![sum_col("l_extendedprice")],
    )
}

// ── Simple OLTP benchmarks (<1ms target) ────────────────────

fn bench_simple_oltp(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_oltp");
    group.sample_size(50);

    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());

    group.bench_function("point_lookup", |b| {
        let expr = oltp_point_lookup();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("filtered_scan", |b| {
        let expr = oltp_filtered_scan();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

// ── Medium OLTP benchmarks (<10ms target) ───────────────────

fn bench_medium_oltp(c: &mut Criterion) {
    let mut group = c.benchmark_group("medium_oltp");
    group.sample_size(30);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());

    group.bench_function("two_table_join", |b| {
        let expr = oltp_two_table_join();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("three_table_join", |b| {
        let expr = oltp_three_table_join();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

// ── Complex OLAP benchmarks (<100ms target) ─────────────────

fn bench_complex_olap(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_olap");
    group.sample_size(20);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());

    group.bench_function("tpch_q1", |b| {
        let expr = olap_tpch_q1();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("tpch_q3", |b| {
        let expr = olap_tpch_q3();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("tpch_q5", |b| {
        let expr = olap_tpch_q5();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

// ── TPROC-C (HammerDB) benchmarks ──────────────────────────

fn bench_tproc_c(c: &mut Criterion) {
    let mut group = c.benchmark_group("tproc_c");
    group.sample_size(30);

    let mut optimizer = make_tpcc_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());

    group.bench_function("new_order", |b| {
        let expr = tproc_c_new_order();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("stock_level", |b| {
        let expr = tproc_c_stock_level();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

// ── TPROC-H (HammerDB) benchmarks ──────────────────────────

fn bench_tproc_h(c: &mut Criterion) {
    let mut group = c.benchmark_group("tproc_h");
    group.sample_size(20);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());

    group.bench_function("q6", |b| {
        let expr = tproc_h_q6();
        b.iter(|| {
            optimizer
                .optimize_bounded(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.finish();
}

// ── Budget switching overhead ───────────────────────────────

fn bench_budget_switching(c: &mut Criterion) {
    let mut group = c.benchmark_group("budget_switching");
    group.sample_size(20);

    let expr = olap_tpch_q3();

    for (label, budget) in [
        ("interactive", ResourceBudget::interactive()),
        ("standard", ResourceBudget::standard()),
        ("batch", ResourceBudget::batch()),
    ] {
        let mut optimizer = make_tpch_optimizer();
        optimizer.set_resource_budget(budget);

        group.bench_with_input(
            BenchmarkId::new("tpch_q3", label),
            &expr,
            |b, expr| {
                b.iter(|| {
                    optimizer
                        .optimize_bounded(black_box(expr))
                        .expect("should succeed")
                });
            },
        );
    }

    group.finish();
}

// ── A/B comparison: bounded vs unbounded ────────────────────

fn bench_ab_bounded_vs_unbounded(c: &mut Criterion) {
    let mut group = c.benchmark_group("ab_bounded_vs_unbounded");
    group.sample_size(20);

    let expr = olap_tpch_q1();

    let unbounded = make_tpch_optimizer();
    let mut bounded = make_tpch_optimizer();
    bounded.set_resource_budget(ResourceBudget::unlimited());

    group.bench_function("unbounded", |b| {
        b.iter(|| {
            unbounded
                .optimize(black_box(&expr))
                .expect("should succeed")
        });
    });

    group.bench_function("bounded_unlimited", |b| {
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
    bench_simple_oltp,
    bench_medium_oltp,
    bench_complex_olap,
    bench_tproc_c,
    bench_tproc_h,
    bench_budget_switching,
    bench_ab_bounded_vs_unbounded,
);
criterion_main!(benches);
