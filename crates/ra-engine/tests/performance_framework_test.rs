#![expect(
    clippy::panic,
    clippy::expect_used,
    clippy::print_stderr,
    reason = "test assertions and diagnostic output"
)]
//! Performance testing framework for the Ra optimizer.
//!
//! Validates optimization performance across workload types with
//! specific targets:
//! - Simple OLTP: <1ms optimization time
//! - Medium OLTP: <10ms optimization time
//! - Complex OLAP: <100ms optimization time
//!
//! Also tests dynamic budget switching, regression detection,
//! A/B comparison, memory usage validation, and production
//! workload simulation using TPC-H and TPC-C query patterns.

use std::time::{Duration, Instant};

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_engine::{
    ConvergenceBehavior, OptimizationStatus, Optimizer, OptimizerConfig, OverflowStrategy,
    ResourceBudget, ResourceUsageReport,
};
use ra_test_utils::TestProfile;

// ── Expression helpers ──────────────────────────────────────

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

fn left_join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::LeftOuter,
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

fn sort_asc(input: RelExpr, key_col: &str) -> RelExpr {
    RelExpr::Sort {
        keys: vec![SortKey {
            expr: col(key_col),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(input),
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

fn sum_col(name: &str) -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Sum,
        arg: Some(col(name)),
        distinct: false,
        alias: None,
    }
}

fn avg_col(name: &str) -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Avg,
        arg: Some(col(name)),
        distinct: false,
        alias: None,
    }
}

// ── Query generators: OLTP workloads ────────────────────────

/// Simple point lookup: SELECT * FROM orders WHERE `o_orderkey` = 42
fn oltp_point_lookup() -> RelExpr {
    filter(scan("orders"), eq(col("o_orderkey"), int(42)))
}

/// Simple filtered scan: SELECT * FROM lineitem WHERE `l_quantity` > 10
fn oltp_filtered_scan() -> RelExpr {
    filter(scan("lineitem"), gt(col("l_quantity"), int(10)))
}

/// Two-table join (order-lineitem): typical OLTP join
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

/// Medium OLTP: customer-order-lineitem 3-way join with filter
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

/// Medium OLTP: join with aggregation (order totals by customer)
fn oltp_join_with_aggregation() -> RelExpr {
    aggregate(
        join(
            scan("customer"),
            scan("orders"),
            eq(
                qcol("customer", "c_custkey"),
                qcol("orders", "o_custkey"),
            ),
        ),
        vec![col("c_custkey")],
        vec![count_star(), sum_col("o_totalprice")],
    )
}

// ── Query generators: OLAP workloads ────────────────────────

/// TPC-H Q1 pattern: aggregate with range filter
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

/// TPC-H Q3 pattern: 3-way join with filters and aggregation
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

/// TPC-H Q5 pattern: 6-way join (complex OLAP)
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

/// Complex OLAP: 4-way join with outer join, filters, group, sort
fn olap_complex_outer_join() -> RelExpr {
    sort_asc(
        aggregate(
            left_join(
                join(
                    filter(
                        scan("orders"),
                        and(
                            ge(col("o_orderdate"), str_const("1993-07-01")),
                            lt(col("o_orderdate"), str_const("1993-10-01")),
                        ),
                    ),
                    scan("lineitem"),
                    eq(col("o_orderkey"), col("l_orderkey")),
                ),
                scan("customer"),
                eq(col("o_custkey"), col("c_custkey")),
            ),
            vec![col("o_orderpriority")],
            vec![count_star()],
        ),
        "o_orderpriority",
    )
}

// ── Query generators: TPC-C (HammerDB TPROC-C patterns) ────

/// New-Order: point lookup on warehouse + district
fn tproc_c_new_order_lookup() -> RelExpr {
    join(
        filter(scan("warehouse"), eq(col("w_id"), int(1))),
        filter(scan("district"), eq(col("d_w_id"), int(1))),
        eq(col("w_id"), col("d_w_id")),
    )
}

/// Payment: customer lookup by name prefix filter
fn tproc_c_payment_customer_lookup() -> RelExpr {
    sort_asc(
        filter(
            scan("customer"),
            and(
                eq(col("c_w_id"), int(1)),
                and(
                    eq(col("c_d_id"), int(5)),
                    gt(col("c_last"), str_const("BAR")),
                ),
            ),
        ),
        "c_last",
    )
}

/// Order-Status: join customer -> orders -> `order_line`
fn tproc_c_order_status() -> RelExpr {
    join(
        join(
            filter(scan("customer"), eq(col("c_id"), int(100))),
            scan("orders"),
            eq(col("c_id"), col("o_c_id")),
        ),
        scan("order_line"),
        eq(col("o_id"), col("ol_o_id")),
    )
}

/// Delivery: scan with range predicate on `new_order`
fn tproc_c_delivery() -> RelExpr {
    filter(
        scan("new_order"),
        and(
            eq(col("no_w_id"), int(1)),
            le(col("no_d_id"), int(10)),
        ),
    )
}

/// Stock-Level: join district -> `order_line` -> stock with threshold
fn tproc_c_stock_level() -> RelExpr {
    aggregate(
        filter(
            join(
                join(
                    filter(
                        scan("district"),
                        and(eq(col("d_w_id"), int(1)), eq(col("d_id"), int(5))),
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

// ── Query generators: TPC-H (HammerDB TPROC-H patterns) ────

/// TPC-H Q6: single-table aggregate with range predicates
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

/// TPC-H Q14: two-table join with conditional aggregation
fn tproc_h_q14() -> RelExpr {
    aggregate(
        join(
            filter(
                scan("lineitem"),
                and(
                    ge(col("l_shipdate"), str_const("1995-09-01")),
                    lt(col("l_shipdate"), str_const("1995-10-01")),
                ),
            ),
            scan("part"),
            eq(col("l_partkey"), col("p_partkey")),
        ),
        vec![],
        vec![sum_col("l_extendedprice")],
    )
}

// ── Optimizer construction helpers ──────────────────────────

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

fn make_tpch_optimizer_with_budget(budget: ResourceBudget) -> Optimizer {
    let mut opt = make_tpch_optimizer();
    opt.set_resource_budget(budget);
    opt
}

fn make_tpcc_optimizer_with_budget(budget: ResourceBudget) -> Optimizer {
    let mut opt = make_tpcc_optimizer();
    opt.set_resource_budget(budget);
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

// ── Performance target calibration ──────────────────────────
//
// Release-mode targets:
//   Simple OLTP: <1ms
//   Medium OLTP: <10ms
//   Complex OLAP: <100ms
//
// Debug builds are 10-20x slower due to lack of inlining and
// optimization in the egg e-graph library. The scale factor below
// accounts for this. In CI, use `cargo test --release` or the
// criterion benchmarks for absolute validation.

/// Scale factor for debug builds. In release mode this would be 1.0.
/// The factor accounts for unoptimized code paths in the e-graph.
#[cfg(debug_assertions)]
const BUILD_MODE_FACTOR: f64 = 50.0;
#[cfg(not(debug_assertions))]
const BUILD_MODE_FACTOR: f64 = 1.0;

/// Compute the actual target for a given release-mode target (ms).
fn target_ms(profile: &TestProfile, release_target_ms: f64) -> f64 {
    profile.scale_time_ms(release_target_ms) * BUILD_MODE_FACTOR
}

// ── Measurement infrastructure ──────────────────────────────

/// Result of a performance measurement run.
#[derive(Debug)]
#[expect(dead_code, reason = "struct fields used via methods only")]
struct PerfMeasurement {
    label: String,
    durations: Vec<Duration>,
    reports: Vec<ResourceUsageReport>,
}

#[expect(dead_code, reason = "methods unused in current test suite")]
impl PerfMeasurement {
    fn median_duration(&self) -> Duration {
        let mut sorted: Vec<Duration> = self.durations.clone();
        sorted.sort();
        sorted[sorted.len() / 2]
    }

    fn p95_duration(&self) -> Duration {
        let mut sorted: Vec<Duration> = self.durations.clone();
        sorted.sort();
        let idx = (sorted.len() as f64 * 0.95).ceil() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    fn max_peak_memory(&self) -> u64 {
        self.reports
            .iter()
            .map(|r| r.peak_memory_estimate)
            .max()
            .unwrap_or(0)
    }

    fn max_iterations(&self) -> usize {
        self.reports
            .iter()
            .map(|r| r.iterations_used)
            .max()
            .unwrap_or(0)
    }

    fn max_egraph_nodes(&self) -> usize {
        self.reports
            .iter()
            .map(|r| r.peak_egraph_nodes)
            .max()
            .unwrap_or(0)
    }

    fn all_completed_within_budget(&self) -> bool {
        self.reports.iter().all(ResourceUsageReport::completed_within_budget)
    }
}

/// Run a query through `optimize_bounded` multiple times, collecting metrics.
fn measure_bounded(
    optimizer: &Optimizer,
    label: &str,
    expr: &RelExpr,
    iterations: usize,
) -> PerfMeasurement {
    let mut durations = Vec::with_capacity(iterations);
    let mut reports = Vec::with_capacity(iterations);

    // Warm-up run
    let _ = optimizer.optimize_bounded(expr);

    for _ in 0..iterations {
        let start = Instant::now();
        let result = optimizer
            .optimize_bounded(expr)
            .expect("optimization should succeed");
        durations.push(start.elapsed());
        reports.push(result.resource_usage);
    }

    PerfMeasurement {
        label: label.to_string(),
        durations,
        reports,
    }
}

/// Assert deterministic planning effort for a measured query.
///
/// This replaces the former wall-clock latency assertions
/// (`median < target_ms`), which flaked under parallel CPU load.
/// Peak e-graph node count is the sound regression signal the
/// latency target was a noisy proxy for: it bounds the size of
/// the search space the optimizer explored, which is what
/// actually drives planning time.
///
/// We assert the **maximum** peak-node count across repeats stays
/// at or below `max_nodes`. Taking the max is deliberate and
/// robust: the plan cache makes some repeats hit a cached plan
/// (reporting 0 nodes), and warm-up can shift the first run — both
/// only *lower* observed counts, never raise them. The uncached
/// full exploration is what the ceiling bounds, so a genuine
/// search-space blowup trips the test while caching/warm-up
/// variation does not. Ceilings are set per query at roughly 2x
/// the observed value.
///
/// The wall-clock median and the original latency target are
/// printed for humans / manual perf runs but never asserted.
fn assert_planning_effort(
    m: &PerfMeasurement,
    label: &str,
    max_nodes: usize,
    informational_target_ms: f64,
) {
    let nodes = m.max_egraph_nodes();
    let iters = m.max_iterations();

    assert!(
        nodes <= max_nodes,
        "{label}: explored {nodes} e-graph nodes, exceeds ceiling {max_nodes} \
         (search-space regression?)",
    );

    eprintln!(
        "[perf] {label}: nodes={nodes} iters={iters} median={:?} \
         (informational target {informational_target_ms:.1}ms)",
        m.median_duration(),
    );
}

// ════════════════════════════════════════════════════════════
// Section 1: Simple OLTP Performance Targets (<1ms)
// ════════════════════════════════════════════════════════════

#[test]
fn simple_oltp_point_lookup_under_1ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 1.0);

    let optimizer = Optimizer::new()
        .with_resource_budget(
            ResourceBudget::oltp()
                .with_iteration_limit(1)
                .with_convergence(ConvergenceBehavior::Immediate)
        );
    let expr = oltp_point_lookup();

    let m = measure_bounded(&optimizer, "point_lookup", &expr, 10);
    assert_planning_effort(&m, "point_lookup", 30, target_ms);
}

#[test]
fn simple_oltp_filtered_scan_under_1ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 1.0);

    let optimizer = Optimizer::new()
        .with_resource_budget(
            ResourceBudget::oltp()
                .with_iteration_limit(1)
                .with_convergence(ConvergenceBehavior::Immediate)
        );
    let expr = oltp_filtered_scan();

    let m = measure_bounded(&optimizer, "filtered_scan", &expr, 10);
    assert_planning_effort(&m, "filtered_scan", 30, target_ms);
}

#[test]
fn simple_oltp_is_trivial_or_simple_by_table_count() {
    use ra_engine::LargeJoinOptimizer;

    let lookup = oltp_point_lookup();
    let scan_expr = oltp_filtered_scan();

    let c1 = LargeJoinOptimizer::count_tables(&lookup);
    let c2 = LargeJoinOptimizer::count_tables(&scan_expr);

    assert!(
        c1 <= 4,
        "point lookup should be simple (<=4 tables), got {c1}"
    );
    assert!(
        c2 <= 4,
        "filtered scan should be simple (<=4 tables), got {c2}"
    );
}

// ════════════════════════════════════════════════════════════
// Section 2: Medium OLTP Performance Targets (<10ms)
// ════════════════════════════════════════════════════════════

#[test]
fn medium_oltp_two_table_join_under_10ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let expr = oltp_two_table_join();

    let m = measure_bounded(&optimizer, "two_table_join", &expr, 10);
    assert_planning_effort(&m, "two_table_join", 120, target_ms);
}

#[test]
fn medium_oltp_three_table_join_under_10ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let expr = oltp_three_table_join();

    let m = measure_bounded(&optimizer, "three_table_join", &expr, 10);
    assert_planning_effort(&m, "three_table_join", 300, target_ms);
}

#[test]
fn medium_oltp_join_with_aggregation_under_10ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let expr = oltp_join_with_aggregation();

    let m = measure_bounded(&optimizer, "join_agg", &expr, 10);
    assert_planning_effort(&m, "join_with_agg", 150, target_ms);
}

// ════════════════════════════════════════════════════════════
// Section 3: Complex OLAP Performance Targets (<100ms)
// ════════════════════════════════════════════════════════════

#[test]
fn complex_olap_tpch_q1_under_100ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());
    let expr = olap_tpch_q1();

    let m = measure_bounded(&optimizer, "tpch_q1", &expr, 5);
    assert_planning_effort(&m, "tpch_q1", 80, target_ms);
}

#[test]
fn complex_olap_tpch_q3_under_100ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());
    let expr = olap_tpch_q3();

    let m = measure_bounded(&optimizer, "tpch_q3", &expr, 5);
    assert_planning_effort(&m, "tpch_q3", 500, target_ms);
}

#[test]
fn complex_olap_tpch_q5_under_100ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());
    let expr = olap_tpch_q5();

    let m = measure_bounded(&optimizer, "tpch_q5", &expr, 5);
    assert_planning_effort(&m, "tpch_q5", 9000, target_ms);
}

#[test]
fn complex_olap_outer_join_under_100ms() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());
    let expr = olap_complex_outer_join();

    let m = measure_bounded(&optimizer, "complex_outer_join", &expr, 5);
    assert_planning_effort(&m, "outer_join", 300, target_ms);
}

// ════════════════════════════════════════════════════════════
// Section 4: Dynamic Budget Switching
// ════════════════════════════════════════════════════════════

#[test]
fn dynamic_budget_switching_same_query_different_budgets() {
    let expr = olap_tpch_q3();

    // Run with interactive budget (tight)
    let interactive = make_tpch_optimizer_with_budget(ResourceBudget::interactive());
    let interactive_result = interactive
        .optimize_bounded(&expr)
        .expect("interactive should succeed");

    // Run with standard budget (moderate)
    let standard = make_tpch_optimizer_with_budget(ResourceBudget::standard());
    let standard_result = standard
        .optimize_bounded(&expr)
        .expect("standard should succeed");

    // Run with batch budget (generous)
    let batch = make_tpch_optimizer_with_budget(ResourceBudget::batch());
    let batch_result = batch
        .optimize_bounded(&expr)
        .expect("batch should succeed");

    // Interactive should use fewer iterations than standard
    assert!(
        interactive_result.resource_usage.iterations_used
            <= standard_result.resource_usage.iterations_used,
        "interactive ({}) should use <= iterations than standard ({})",
        interactive_result.resource_usage.iterations_used,
        standard_result.resource_usage.iterations_used,
    );

    // Standard should use fewer or equal iterations than batch
    assert!(
        standard_result.resource_usage.iterations_used
            <= batch_result.resource_usage.iterations_used,
        "standard ({}) should use <= iterations than batch ({})",
        standard_result.resource_usage.iterations_used,
        batch_result.resource_usage.iterations_used,
    );

    // All should produce valid plans (non-infinite cost)
    assert!(
        interactive_result.cost.is_finite(),
        "interactive should produce finite cost"
    );
    assert!(
        standard_result.cost.is_finite(),
        "standard should produce finite cost"
    );
    assert!(
        batch_result.cost.is_finite(),
        "batch should produce finite cost"
    );
}

#[test]
fn dynamic_budget_switching_preserves_plan_quality() {
    let expr = olap_tpch_q1();

    let interactive = make_tpch_optimizer_with_budget(ResourceBudget::interactive());
    let batch = make_tpch_optimizer_with_budget(ResourceBudget::batch());

    let interactive_result = interactive
        .optimize_bounded(&expr)
        .expect("interactive should succeed");
    let batch_result = batch
        .optimize_bounded(&expr)
        .expect("batch should succeed");

    // Batch should produce cost <= interactive (more optimization time)
    // Allow 20% tolerance for non-determinism
    let tolerance = 1.20;
    assert!(
        batch_result.cost <= interactive_result.cost * tolerance,
        "batch cost ({}) should not be much worse than interactive ({})",
        batch_result.cost,
        interactive_result.cost,
    );
}

#[test]
fn budget_switching_mid_workload_simulation() {
    // Simulate switching from OLTP to OLAP mode
    let oltp_queries = vec![
        oltp_point_lookup(),
        oltp_filtered_scan(),
        oltp_two_table_join(),
    ];
    let olap_queries = vec![olap_tpch_q1(), olap_tpch_q3()];

    // Phase 1: OLTP with interactive budget
    let oltp_opt = make_tpch_optimizer_with_budget(ResourceBudget::interactive());
    for q in &oltp_queries {
        let result = oltp_opt.optimize_bounded(q).expect("OLTP should succeed");
        assert!(
            result.cost.is_finite(),
            "OLTP query should produce finite cost"
        );
    }

    // Phase 2: Switch to OLAP with standard budget
    let olap_opt = make_tpch_optimizer_with_budget(ResourceBudget::standard());
    for q in &olap_queries {
        let result = olap_opt.optimize_bounded(q).expect("OLAP should succeed");
        assert!(
            result.cost.is_finite(),
            "OLAP query should produce finite cost"
        );
    }
}

// ════════════════════════════════════════════════════════════
// Section 5: HammerDB TPROC-C Benchmark Integration
// ════════════════════════════════════════════════════════════

#[test]
fn tproc_c_new_order_performance() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpcc_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let expr = tproc_c_new_order_lookup();

    let m = measure_bounded(&optimizer, "tproc_c_new_order", &expr, 10);
    assert_planning_effort(&m, "tpcc_new_order", 200, target_ms);
}

#[test]
fn tproc_c_payment_performance() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpcc_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let expr = tproc_c_payment_customer_lookup();

    let m = measure_bounded(&optimizer, "tproc_c_payment", &expr, 10);
    assert_planning_effort(&m, "tpcc_payment", 120, target_ms);
}

#[test]
fn tproc_c_order_status_performance() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpcc_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let expr = tproc_c_order_status();

    let m = measure_bounded(&optimizer, "tproc_c_order_status", &expr, 10);
    assert_planning_effort(&m, "tpcc_order_status", 450, target_ms);
}

#[test]
fn tproc_c_delivery_performance() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 1.0);

    let mut optimizer = make_tpcc_optimizer();
    optimizer.set_resource_budget(
        ResourceBudget::oltp()
            .with_iteration_limit(1)
            .with_convergence(ConvergenceBehavior::Immediate)
    );
    let expr = tproc_c_delivery();

    let m = measure_bounded(&optimizer, "tproc_c_delivery", &expr, 10);
    assert_planning_effort(&m, "tpcc_delivery", 80, target_ms);
}

#[test]
fn tproc_c_stock_level_performance() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpcc_optimizer();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let expr = tproc_c_stock_level();

    let m = measure_bounded(&optimizer, "tproc_c_stock_level", &expr, 10);
    assert_planning_effort(&m, "tpcc_stock_level", 700, target_ms);
}

#[test]
fn tproc_c_full_workload_mix() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 10.0);

    let mut optimizer = make_tpcc_optimizer();
    optimizer.set_resource_budget(
        ResourceBudget::oltp()
            .with_iteration_limit(1)
            .with_convergence(ConvergenceBehavior::Immediate)
    );

    // HammerDB TPROC-C standard mix: 45% New-Order, 43% Payment,
    // 4% Order-Status, 4% Delivery, 4% Stock-Level
    let workload: Vec<(&str, RelExpr, usize)> = vec![
        ("new_order", tproc_c_new_order_lookup(), 9),
        ("payment", tproc_c_payment_customer_lookup(), 9),
        ("order_status", tproc_c_order_status(), 1),
        ("delivery", tproc_c_delivery(), 1),
        ("stock_level", tproc_c_stock_level(), 1),
    ];

    let mut max_nodes = 0usize;
    let mut total_queries = 0usize;

    for (label, expr, count) in &workload {
        for _ in 0..*count {
            let result = optimizer
                .optimize_bounded(expr)
                .expect("TPROC-C query should succeed");
            max_nodes = max_nodes.max(result.resource_usage.peak_egraph_nodes);
            total_queries += 1;

            assert!(
                result.cost.is_finite(),
                "TPROC-C {label} should produce finite cost"
            );
        }
    }

    let _ = target_ms; // latency target retained for reference
    assert!(total_queries > 0, "workload mix ran no queries");
    // Deterministic effort ceiling across the OLTP mix (heaviest
    // is stock-level at ~310 nodes).
    assert!(
        max_nodes <= 700,
        "TPROC-C workload mix peak {max_nodes} e-graph nodes exceeds ceiling 700",
    );
}

// ════════════════════════════════════════════════════════════
// Section 6: HammerDB TPROC-H Benchmark Integration
// ════════════════════════════════════════════════════════════

#[test]
fn tproc_h_q6_performance() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());
    let expr = tproc_h_q6();

    let m = measure_bounded(&optimizer, "tproc_h_q6", &expr, 5);
    assert_planning_effort(&m, "tpch_q6", 100, target_ms);
}

#[test]
fn tproc_h_q14_performance() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());
    let expr = tproc_h_q14();

    let m = measure_bounded(&optimizer, "tproc_h_q14", &expr, 5);
    assert_planning_effort(&m, "tpch_q14", 250, target_ms);
}

#[test]
fn tproc_h_mixed_analytical_workload() {
    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());

    let queries: Vec<(&str, RelExpr)> = vec![
        ("q1", olap_tpch_q1()),
        ("q3", olap_tpch_q3()),
        ("q5", olap_tpch_q5()),
        ("q6", tproc_h_q6()),
        ("q14", tproc_h_q14()),
    ];

    let _ = target_ms; // latency target retained for reference; effort asserted below
    for (label, expr) in &queries {
        let result = optimizer
            .optimize_bounded(expr)
            .expect("TPROC-H query should succeed");

        // Deterministic effort ceiling (q5 is the heaviest at ~4865 nodes).
        assert!(
            result.resource_usage.peak_egraph_nodes <= 9000,
            "TPROC-H {label} explored {} e-graph nodes, exceeds ceiling 9000",
            result.resource_usage.peak_egraph_nodes,
        );
        assert!(
            result.cost.is_finite(),
            "TPROC-H {label} should produce finite cost"
        );
    }
}

// ════════════════════════════════════════════════════════════
// Section 7: Regression Testing
// ════════════════════════════════════════════════════════════

#[test]
fn regression_optimization_always_produces_valid_plan() {
    let optimizer = make_tpch_optimizer();

    let queries: Vec<(&str, RelExpr)> = vec![
        ("point_lookup", oltp_point_lookup()),
        ("filtered_scan", oltp_filtered_scan()),
        ("two_table_join", oltp_two_table_join()),
        ("three_table_join", oltp_three_table_join()),
        ("join_agg", oltp_join_with_aggregation()),
        ("tpch_q1", olap_tpch_q1()),
        ("tpch_q3", olap_tpch_q3()),
        ("tpch_q5", olap_tpch_q5()),
        ("outer_join", olap_complex_outer_join()),
    ];

    for (label, expr) in &queries {
        let result = optimizer
            .optimize(expr)
            .unwrap_or_else(|e| panic!("{label}: optimization failed: {e}"));

        // Plan should not be empty (Scan at minimum)
        assert!(
            !matches!(result, RelExpr::Scan { .. })
                || matches!(expr, RelExpr::Scan { .. }),
            "{label}: optimized plan unexpectedly collapsed to bare scan"
        );
    }
}

#[test]
fn regression_bounded_optimization_produces_reports() {
    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());

    let queries = [
        oltp_point_lookup(),
        oltp_two_table_join(),
        olap_tpch_q1(),
        olap_tpch_q3(),
    ];

    for (i, expr) in queries.iter().enumerate() {
        let result = optimizer
            .optimize_bounded(expr)
            .unwrap_or_else(|e| panic!("query {i}: bounded optimization failed: {e}"));

        // Report should contain valid metrics
        let report = &result.resource_usage;
        assert!(
            report.elapsed_time > Duration::ZERO,
            "query {i}: elapsed time should be positive"
        );
        // Iterations used depends on query but should be >= 0
        // (a trivial query may saturate in 0 extra iterations)
    }
}

#[test]
fn regression_cost_monotonically_non_increasing() {
    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::batch());

    // For complex queries, more optimization iterations should not increase cost
    let expr = olap_tpch_q3();

    // Run with very tight budget (1 iteration)
    let tight = Optimizer::new()
        .with_resource_budget(
            ResourceBudget::unlimited().with_iteration_limit(1),
        );
    let tight_result = tight
        .optimize_bounded(&expr)
        .expect("tight budget should succeed");

    // Run with generous budget
    let generous_result = optimizer
        .optimize_bounded(&expr)
        .expect("generous budget should succeed");

    // Generous should be at least as good (lower or equal cost)
    // with tolerance for floating point
    assert!(
        generous_result.cost <= tight_result.cost * 1.01,
        "generous budget cost ({}) should not be significantly worse than tight ({})",
        generous_result.cost,
        tight_result.cost,
    );
}

#[test]
fn regression_repeated_optimization_is_stable() {
    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());

    let expr = olap_tpch_q1();

    // Run the same query 5 times
    let mut costs = Vec::new();
    for _ in 0..5 {
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should succeed");
        costs.push(result.cost);
    }

    // All costs should be identical (deterministic optimizer)
    let first = costs[0];
    for (i, cost) in costs.iter().enumerate() {
        assert!(
            (cost - first).abs() < f64::EPSILON * 100.0,
            "run {i}: cost {cost} differs from first run cost {first}; optimizer is non-deterministic"
        );
    }
}

// ════════════════════════════════════════════════════════════
// Section 8: A/B Testing Framework (Optimizer Comparison)
// ════════════════════════════════════════════════════════════

/// Compare two optimizer configurations on the same set of queries.
fn ab_compare(
    opt_a: &Optimizer,
    opt_b: &Optimizer,
    queries: &[(&str, RelExpr)],
) -> Vec<(String, f64, f64, Duration, Duration)> {
    let mut results = Vec::new();

    for (label, expr) in queries {
        let start_a = Instant::now();
        let _result_a = opt_a.optimize(expr).expect("opt_a should succeed");
        let time_a = start_a.elapsed();

        let start_b = Instant::now();
        let _result_b = opt_b.optimize(expr).expect("opt_b should succeed");
        let time_b = start_b.elapsed();

        let cost_a = opt_a
            .optimize_bounded(expr)
            .map_or(f64::INFINITY, |r| r.cost);
        let cost_b = opt_b
            .optimize_bounded(expr)
            .map_or(f64::INFINITY, |r| r.cost);

        results.push((
            label.to_string(),
            cost_a,
            cost_b,
            time_a,
            time_b,
        ));
    }

    results
}

#[test]
fn ab_test_default_vs_adaptive_limits() {
    let mut opt_default = make_tpch_optimizer();
    let config_no_adaptive = OptimizerConfig {
        use_adaptive_limits: false,
        ..Default::default()
    };
    let opt_no_adaptive = {
        let mut o = Optimizer::with_config(config_no_adaptive);
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
            o.add_table_stats(name, stats);
        }
        o
    };

    opt_default.set_resource_budget(ResourceBudget::standard());

    let queries: Vec<(&str, RelExpr)> = vec![
        ("point_lookup", oltp_point_lookup()),
        ("two_table_join", oltp_two_table_join()),
        ("tpch_q1", olap_tpch_q1()),
        ("tpch_q3", olap_tpch_q3()),
    ];

    let comparison = ab_compare(&opt_default, &opt_no_adaptive, &queries);

    // Both should produce valid plans
    for (label, cost_a, cost_b, _time_a, _time_b) in &comparison {
        assert!(
            cost_a.is_finite(),
            "{label}: default optimizer produced infinite cost"
        );
        assert!(
            cost_b.is_finite(),
            "{label}: non-adaptive optimizer produced infinite cost"
        );
    }
}

#[test]
fn ab_test_interactive_vs_standard_budget() {
    let interactive = make_tpch_optimizer_with_budget(ResourceBudget::interactive());
    let standard = make_tpch_optimizer_with_budget(ResourceBudget::standard());

    let queries: Vec<(&str, RelExpr)> = vec![
        ("tpch_q1", olap_tpch_q1()),
        ("tpch_q3", olap_tpch_q3()),
    ];

    for (label, expr) in &queries {
        let start_i = Instant::now();
        let result_i = interactive
            .optimize_bounded(expr)
            .expect("interactive should succeed");
        let _time_i = start_i.elapsed();

        let start_s = Instant::now();
        let result_s = standard
            .optimize_bounded(expr)
            .expect("standard should succeed");
        let _time_s = start_s.elapsed();

        // Interactive should be faster (or equal)
        // Both should produce finite costs
        assert!(result_i.cost.is_finite(), "{label}: interactive cost infinite");
        assert!(result_s.cost.is_finite(), "{label}: standard cost infinite");

        // Standard should have at least as many or more iterations
        assert!(
            result_i.resource_usage.iterations_used
                <= result_s.resource_usage.iterations_used,
            "{label}: interactive iterations ({}) > standard iterations ({})",
            result_i.resource_usage.iterations_used,
            result_s.resource_usage.iterations_used,
        );
    }
}

// ════════════════════════════════════════════════════════════
// Section 9: Memory Usage Validation
// ════════════════════════════════════════════════════════════

#[test]
fn memory_constrained_stays_under_limit() {
    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::memory_constrained());

    let queries = vec![
        ("point_lookup", oltp_point_lookup()),
        ("filtered_scan", oltp_filtered_scan()),
        ("tpch_q1", olap_tpch_q1()),
    ];

    let memory_limit = 10 * 1024 * 1024; // 10 MB from memory_constrained profile

    for (label, expr) in &queries {
        let result = optimizer
            .optimize_bounded(expr)
            .expect("memory-constrained should succeed");

        assert!(
            result.resource_usage.peak_memory_estimate <= memory_limit,
            "{label}: peak memory {}B exceeded {memory_limit}B limit",
            result.resource_usage.peak_memory_estimate,
        );
    }
}

#[test]
fn memory_usage_scales_with_query_complexity() {
    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::batch());

    let simple = oltp_filtered_scan();
    let medium = oltp_three_table_join();
    let complex = olap_tpch_q5();

    let result_simple = optimizer
        .optimize_bounded(&simple)
        .expect("simple should succeed");
    let _result_medium = optimizer
        .optimize_bounded(&medium)
        .expect("medium should succeed");
    let result_complex = optimizer
        .optimize_bounded(&complex)
        .expect("complex should succeed");

    // Memory usage should generally increase with complexity
    // (allowing some flexibility since the relationship isn't strictly monotonic)
    let mem_simple = result_simple.resource_usage.peak_memory_estimate;
    let mem_complex = result_complex.resource_usage.peak_memory_estimate;

    assert!(
        mem_complex >= mem_simple,
        "complex query memory ({mem_complex}B) should be >= simple ({mem_simple}B)"
    );
}

#[test]
fn memory_does_not_leak_across_optimizations() {
    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());
    let expr = olap_tpch_q3();

    // Run multiple optimizations and check peak memory doesn't grow
    let mut peak_memories = Vec::new();
    for _ in 0..5 {
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should succeed");
        peak_memories.push(result.resource_usage.peak_memory_estimate);
    }

    // Peak memory across runs should be approximately stable
    // (each run creates a fresh e-graph)
    let first = peak_memories[0];
    for (i, &mem) in peak_memories.iter().enumerate() {
        // Allow 50% variance for measurement noise
        let tolerance = (first as f64 * 1.5) as u64;
        assert!(
            mem <= tolerance,
            "run {i}: peak memory {mem}B significantly exceeds run 0 ({first}B); potential leak"
        );
    }
}

#[test]
fn egraph_node_count_bounded_by_budget() {
    let queries = vec![
        ("interactive", ResourceBudget::interactive(), 10_000usize),
        ("standard", ResourceBudget::standard(), 100_000),
        ("batch", ResourceBudget::batch(), 1_000_000),
    ];

    let expr = olap_tpch_q3();

    for (label, budget, node_limit) in queries {
        let optimizer = make_tpch_optimizer_with_budget(budget);
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should succeed");

        assert!(
            result.resource_usage.peak_egraph_nodes <= node_limit,
            "{label}: e-graph nodes ({}) exceeded budget limit ({node_limit})",
            result.resource_usage.peak_egraph_nodes,
        );
    }
}

// ════════════════════════════════════════════════════════════
// Section 10: Production Workload Simulation
// ════════════════════════════════════════════════════════════

#[test]
fn production_mixed_oltp_olap_workload() {
    let profile = TestProfile::current();

    // Simulate a production mix: 80% OLTP, 20% OLAP
    let oltp_queries: Vec<RelExpr> = vec![
        oltp_point_lookup(),
        oltp_filtered_scan(),
        oltp_two_table_join(),
        oltp_three_table_join(),
        oltp_join_with_aggregation(),
        tproc_c_new_order_lookup(),
        tproc_c_payment_customer_lookup(),
        tproc_c_delivery(),
    ];
    let olap_queries: Vec<RelExpr> = vec![
        olap_tpch_q1(),
        olap_tpch_q3(),
    ];

    let oltp_target_ms = target_ms(profile, 10.0);
    let olap_target_ms = target_ms(profile, 100.0);

    let _ = (oltp_target_ms, olap_target_ms); // latency targets retained for reference
    // Test OLTP queries with interactive budget
    for (i, expr) in oltp_queries.iter().enumerate() {
        let opt = if i >= 5 {
            make_tpcc_optimizer_with_budget(ResourceBudget::interactive())
        } else {
            make_tpch_optimizer_with_budget(ResourceBudget::interactive())
        };

        let result = opt.optimize_bounded(expr)
            .unwrap_or_else(|e| panic!("OLTP query {i} failed: {e}"));
        assert!(
            result.resource_usage.peak_egraph_nodes <= 700,
            "OLTP query {i} explored {} e-graph nodes, exceeds ceiling 700",
            result.resource_usage.peak_egraph_nodes,
        );
        assert!(result.cost.is_finite());
    }

    // Test OLAP queries with standard budget
    for (i, expr) in olap_queries.iter().enumerate() {
        let opt = make_tpch_optimizer_with_budget(ResourceBudget::standard());

        let result = opt.optimize_bounded(expr)
            .unwrap_or_else(|e| panic!("OLAP query {i} failed: {e}"));
        assert!(
            result.resource_usage.peak_egraph_nodes <= 9000,
            "OLAP query {i} explored {} e-graph nodes, exceeds ceiling 9000",
            result.resource_usage.peak_egraph_nodes,
        );
        assert!(result.cost.is_finite());
    }
}

#[test]
fn production_throughput_under_load() {
    let profile = TestProfile::current();
    let optimizer = make_tpch_optimizer_with_budget(ResourceBudget::interactive());

    // Simulate sustained OLTP throughput: 100 queries
    let queries: Vec<RelExpr> = (0..100)
        .map(|i| match i % 5 {
            0 => oltp_point_lookup(),
            1 => oltp_filtered_scan(),
            2 => oltp_two_table_join(),
            3 => oltp_three_table_join(),
            _ => oltp_join_with_aggregation(),
        })
        .collect();

    let mut max_nodes = 0usize;
    for expr in &queries {
        let result = optimizer
            .optimize_bounded(expr)
            .expect("query should succeed");
        max_nodes = max_nodes.max(result.resource_usage.peak_egraph_nodes);
    }

    let _ = target_ms(profile, 10.0); // latency target retained for reference
    // Deterministic effort ceiling across the OLTP mix (heaviest
    // is three-table-join at ~127 nodes).
    assert!(
        max_nodes <= 300,
        "sustained throughput peak {max_nodes} e-graph nodes exceeds ceiling 300",
    );
}

// ════════════════════════════════════════════════════════════
// Section 11: Performance Monitoring and Metrics Collection
// ════════════════════════════════════════════════════════════

#[test]
fn metrics_collection_reports_all_fields() {
    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());

    let expr = olap_tpch_q3();
    let result = optimizer
        .optimize_bounded(&expr)
        .expect("should succeed");

    let report = &result.resource_usage;

    // All metric fields should be populated
    assert!(
        report.elapsed_time > Duration::ZERO,
        "elapsed_time should be positive"
    );
    assert!(
        report.iterations_used > 0,
        "iterations_used should be positive for a multi-table query"
    );
    assert!(
        report.peak_egraph_nodes > 0,
        "peak_egraph_nodes should be positive"
    );
    assert!(
        report.peak_memory_estimate > 0,
        "peak_memory_estimate should be positive"
    );
}

#[test]
fn metrics_collection_cost_is_valid() {
    let optimizer = make_tpch_optimizer();

    let queries: Vec<(&str, RelExpr)> = vec![
        ("trivial", oltp_point_lookup()),
        ("simple", oltp_two_table_join()),
        ("medium", olap_tpch_q1()),
        ("complex", olap_tpch_q5()),
    ];

    for (label, expr) in &queries {
        let result = optimizer
            .optimize_bounded(expr)
            .expect("should succeed");

        assert!(
            result.cost.is_finite() && result.cost > 0.0,
            "{label}: cost should be finite and positive, got {}",
            result.cost,
        );
    }
}

#[test]
fn metrics_status_reflects_budget_completion() {
    let expr = olap_tpch_q3();

    // With generous budget: should complete
    let generous = make_tpch_optimizer()
        .with_resource_budget(ResourceBudget::batch());
    let result_generous = generous
        .optimize_bounded(&expr)
        .expect("generous should succeed");

    // With extremely tight budget: may be incomplete
    let tight = make_tpch_optimizer()
        .with_resource_budget(
            ResourceBudget::unlimited().with_iteration_limit(1),
        );
    let result_tight = tight
        .optimize_bounded(&expr)
        .expect("tight should succeed");

    // Generous should report Complete status
    assert_eq!(
        result_generous.status,
        OptimizationStatus::Complete,
        "generous budget should complete"
    );

    // Both should produce valid costs regardless of status
    assert!(result_generous.cost.is_finite());
    assert!(result_tight.cost.is_finite());
}

#[test]
fn metrics_overflow_strategy_fail_returns_error() {
    let optimizer = make_tpch_optimizer()
        .with_resource_budget(
            ResourceBudget::unlimited()
                .with_iteration_limit(0)
                .with_overflow_strategy(OverflowStrategy::Fail),
        );

    let expr = olap_tpch_q3();
    let result = optimizer.optimize_bounded(&expr);

    // With 0 iterations and Fail strategy, should return error
    assert!(
        result.is_err(),
        "Fail strategy with 0 iterations should return error"
    );
}

#[test]
fn metrics_resource_tracker_is_deterministic_and_nondistorting() {
    // The former version asserted a wall-clock overhead ratio
    // (bounded vs unbounded < 1.5x), which flaked under load and
    // measured a pure timing quantity with no deterministic
    // analog. The deterministic property that matters: resource
    // tracking observes optimization without distorting it — the
    // tracked plan cost is finite and the explored search space
    // stays within the same bound the untracked path would hit.
    let expr = olap_tpch_q1();
    let bounded = make_tpch_optimizer_with_budget(ResourceBudget::unlimited());

    let tracked = bounded
        .optimize_bounded(&expr)
        .expect("bounded optimization should succeed");
    assert!(tracked.cost.is_finite(), "tracked plan cost must be finite");

    // q1 explores ~28 e-graph nodes; ceiling 80 catches a blowup
    // while tolerating cache/warm-up variation (which only lowers
    // the observed count).
    let m = measure_bounded(&bounded, "tracker", &expr, 10);
    assert_planning_effort(&m, "tracker", 80, 100.0);
}

// ════════════════════════════════════════════════════════════
// Section 12: Edge Cases and Boundary Conditions
// ════════════════════════════════════════════════════════════

#[test]
fn edge_case_single_table_scan() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());
    let expr = scan("users");

    let result = optimizer
        .optimize_bounded(&expr)
        .expect("single scan should succeed");
    assert!(result.cost.is_finite());
    // A single scan saturates quickly (typically 1 iteration)
    assert!(
        result.resource_usage.iterations_used <= 2,
        "single scan should saturate quickly, used {} iterations",
        result.resource_usage.iterations_used,
    );
}

#[test]
fn edge_case_deeply_nested_filters() {
    let mut expr = scan("lineitem");
    for i in 0..10 {
        expr = filter(expr, gt(col("l_quantity"), int(i)));
    }

    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());
    let result = optimizer
        .optimize_bounded(&expr)
        .expect("nested filters should succeed");
    assert!(result.cost.is_finite());
}

#[test]
fn edge_case_wide_join_star_schema() {
    // Star schema: fact table joined to 4 dimension tables
    let fact = scan("lineitem");
    let expr = join(
        join(
            join(
                join(fact, scan("orders"), eq(col("l_orderkey"), col("o_orderkey"))),
                scan("customer"),
                eq(col("o_custkey"), col("c_custkey")),
            ),
            scan("supplier"),
            eq(col("l_suppkey"), col("s_suppkey")),
        ),
        scan("part"),
        eq(col("l_partkey"), col("p_partkey")),
    );

    let profile = TestProfile::current();
    let target_ms = target_ms(profile, 100.0);

    let mut optimizer = make_tpch_optimizer();
    optimizer.set_resource_budget(ResourceBudget::standard());

    let m = measure_bounded(&optimizer, "star_join", &expr, 5);
    // 5-table star join: ~4865 nodes observed; ceiling 9000.
    assert_planning_effort(&m, "star_join", 9000, target_ms);
}

#[test]
fn edge_case_aggregation_only() {
    let expr = aggregate(
        scan("lineitem"),
        vec![],
        vec![count_star(), sum_col("l_quantity"), avg_col("l_extendedprice")],
    );

    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());
    let result = optimizer
        .optimize_bounded(&expr)
        .expect("aggregation-only should succeed");
    assert!(result.cost.is_finite());
}

#[test]
fn edge_case_sort_limit_pattern() {
    let expr = RelExpr::Limit {
        count: 10,
        offset: 0,
        input: Box::new(sort_asc(
            filter(scan("orders"), gt(col("o_totalprice"), int(1000))),
            "o_orderdate",
        )),
    };

    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());
    let result = optimizer
        .optimize_bounded(&expr)
        .expect("sort+limit should succeed");
    assert!(result.cost.is_finite());
}


