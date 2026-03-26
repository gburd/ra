//! Tests for adaptive iteration limits (Task #246).

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::{Optimizer, OptimizerConfig};
use std::time::Instant;

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
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

fn make_optimizer_with_stats(table_names: &[&str]) -> Optimizer {
    let mut opt = Optimizer::new();
    for name in table_names {
        let mut stats = Statistics::new(10000.0);
        stats.avg_row_size = 100;
        stats.total_size = 1_000_000;
        opt.add_table_stats(*name, stats);
    }
    opt
}

#[test]
fn test_adaptive_limits_simple_query() {
    // Simple query: 2-4 tables
    let query = join(
        scan("users"),
        scan("orders"),
        eq(col("users.id"), col("orders.user_id")),
    );

    let optimizer = make_optimizer_with_stats(&["users", "orders"]);

    let start = Instant::now();
    let result = optimizer.optimize(&query);
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    println!("Simple query (2 tables) optimized in {:?}", elapsed);

    // Should be much faster than 1000ms baseline
    assert!(
        elapsed.as_millis() < 500,
        "Simple query took {}ms (expected <500ms)",
        elapsed.as_millis()
    );
}

#[test]
fn test_adaptive_limits_medium_query() {
    // Medium query: 5-7 tables
    let query = join(
        join(
            join(scan("t1"), scan("t2"), eq(col("t1.id"), col("t2.id"))),
            join(scan("t3"), scan("t4"), eq(col("t3.id"), col("t4.id"))),
            eq(col("t2.id"), col("t3.id")),
        ),
        join(scan("t5"), scan("t6"), eq(col("t5.id"), col("t6.id"))),
        eq(col("t4.id"), col("t5.id")),
    );

    let optimizer = make_optimizer_with_stats(&["t1", "t2", "t3", "t4", "t5", "t6"]);

    let start = Instant::now();
    let result = optimizer.optimize(&query);
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    println!("Medium query (6 tables) optimized in {:?}", elapsed);

    // Should be faster than 770ms baseline for 7-table query
    assert!(
        elapsed.as_millis() < 400,
        "Medium query took {}ms (expected <400ms)",
        elapsed.as_millis()
    );
}

#[test]
#[ignore] // Performance test - timing assertions fail under coverage instrumentation
fn test_adaptive_limits_vs_fixed() {
    // Compare adaptive vs fixed 30 iterations
    let query = join(
        join(
            join(scan("t1"), scan("t2"), eq(col("t1.id"), col("t2.id"))),
            scan("t3"),
            eq(col("t2.id"), col("t3.id")),
        ),
        join(scan("t4"), scan("t5"), eq(col("t4.id"), col("t5.id"))),
        eq(col("t3.id"), col("t4.id")),
    );

    let tables = ["t1", "t2", "t3", "t4", "t5"];

    // Adaptive limits (default)
    let optimizer_adaptive = make_optimizer_with_stats(&tables);
    let start_adaptive = Instant::now();
    let result_adaptive = optimizer_adaptive.optimize(&query);
    let elapsed_adaptive = start_adaptive.elapsed();
    assert!(result_adaptive.is_ok());

    // Fixed 30 iterations (old behavior)
    let mut config_fixed = OptimizerConfig::default();
    config_fixed.use_adaptive_limits = false;
    config_fixed.iter_limit = 30;

    let mut optimizer_fixed = Optimizer::with_config(config_fixed);
    for name in &tables {
        let mut stats = Statistics::new(10000.0);
        stats.avg_row_size = 100;
        stats.total_size = 1_000_000;
        optimizer_fixed.add_table_stats(*name, stats);
    }

    let start_fixed = Instant::now();
    let result_fixed = optimizer_fixed.optimize(&query);
    let elapsed_fixed = start_fixed.elapsed();
    assert!(result_fixed.is_ok());

    println!(
        "Adaptive: {:?}, Fixed: {:?}, Speedup: {:.2}x",
        elapsed_adaptive,
        elapsed_fixed,
        elapsed_fixed.as_secs_f64() / elapsed_adaptive.as_secs_f64()
    );

    // Adaptive should be significantly faster
    assert!(
        elapsed_adaptive < elapsed_fixed,
        "Adaptive ({:?}) should be faster than fixed ({:?})",
        elapsed_adaptive,
        elapsed_fixed
    );

    // Should see at least 1.5x speedup
    let speedup = elapsed_fixed.as_secs_f64() / elapsed_adaptive.as_secs_f64();
    assert!(
        speedup > 1.5,
        "Expected >1.5x speedup, got {:.2}x",
        speedup
    );
}

#[test]
fn test_query_complexity_classification() {
    use ra_engine::query_complexity::QueryComplexity;

    // Trivial (1 table)
    let query = scan("users");
    assert_eq!(
        QueryComplexity::from_expr(&query),
        QueryComplexity::Trivial
    );

    // Simple (2 tables)
    let query = join(scan("users"), scan("orders"), eq(col("a"), col("b")));
    assert_eq!(QueryComplexity::from_expr(&query), QueryComplexity::Simple);

    // Medium (5 tables)
    let mut query = join(scan("t1"), scan("t2"), eq(col("a"), col("b")));
    for i in 3..=5 {
        query = join(query, scan(&format!("t{}", i)), eq(col("a"), col("b")));
    }
    assert_eq!(QueryComplexity::from_expr(&query), QueryComplexity::Medium);

    // Complex (8 tables)
    let mut query = join(scan("t1"), scan("t2"), eq(col("a"), col("b")));
    for i in 3..=8 {
        query = join(query, scan(&format!("t{}", i)), eq(col("a"), col("b")));
    }
    assert_eq!(
        QueryComplexity::from_expr(&query),
        QueryComplexity::Complex
    );
}
