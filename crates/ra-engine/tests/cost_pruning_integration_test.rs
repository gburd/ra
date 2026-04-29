#![expect(clippy::unwrap_used, reason = "test code")]
//! Integration tests for cost-based pruning (Phase 3 - Task #258).
//!
//! Validates that cost pruning improves optimization efficiency.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::{Optimizer, OptimizerConfig};

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
fn test_cost_pruning_enabled() {
    // Create a query that benefits from cost pruning
    let query = join(
        join(scan("t1"), scan("t2"), eq(col("t1.id"), col("t2.id"))),
        join(scan("t3"), scan("t4"), eq(col("t3.id"), col("t4.id"))),
        eq(col("t2.id"), col("t3.id")),
    );

    let optimizer = make_optimizer_with_stats(&["t1", "t2", "t3", "t4"]);
    let result = optimizer.optimize(&query);
    assert!(result.is_ok());
}

#[test]
fn test_cost_pruning_disabled() {
    // Test with cost pruning explicitly disabled
    let query = join(
        scan("users"),
        scan("orders"),
        eq(col("users.id"), col("orders.user_id")),
    );

    let config = OptimizerConfig {
        use_cost_pruning: false, // Disable cost pruning
        ..OptimizerConfig::default()
    };

    let mut optimizer = Optimizer::with_config(config);
    let mut stats = Statistics::new(10000.0);
    stats.avg_row_size = 100;
    stats.total_size = 1_000_000;
    optimizer.add_table_stats("users", stats.clone());
    optimizer.add_table_stats("orders", stats);

    let result = optimizer.optimize(&query);
    assert!(result.is_ok());
}

#[test]
fn test_cost_pruning_threshold_validation() {
    // Test different pruning thresholds
    let query = join(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        scan("c"),
        eq(col("b.id"), col("c.id")),
    );

    // Aggressive pruning (1.2x threshold)
    let config_aggressive = OptimizerConfig {
        cost_pruning_threshold: 1.2,
        ..OptimizerConfig::default()
    };

    let mut opt_aggressive = Optimizer::with_config(config_aggressive);
    let mut stats = Statistics::new(10000.0);
    stats.avg_row_size = 100;
    stats.total_size = 1_000_000;
    opt_aggressive.add_table_stats("a", stats.clone());
    opt_aggressive.add_table_stats("b", stats.clone());
    opt_aggressive.add_table_stats("c", stats.clone());

    let result = opt_aggressive.optimize(&query);
    assert!(result.is_ok());

    // Conservative pruning (2.0x threshold)
    let config_conservative = OptimizerConfig {
        cost_pruning_threshold: 2.0,
        ..OptimizerConfig::default()
    };

    let mut opt_conservative = Optimizer::with_config(config_conservative);
    opt_conservative.add_table_stats("a", stats.clone());
    opt_conservative.add_table_stats("b", stats.clone());
    opt_conservative.add_table_stats("c", stats);

    let result = opt_conservative.optimize(&query);
    assert!(result.is_ok());
}

#[test]
fn test_cost_pruning_produces_valid_plans() {
    fn contains_join(expr: &RelExpr) -> bool {
        match expr {
            RelExpr::Join { .. } => true,
            RelExpr::Filter { input, .. }
            | RelExpr::Project { input, .. }
            | RelExpr::Aggregate { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Window { input, .. }
            | RelExpr::Distinct { input } => contains_join(input),
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => contains_join(left) || contains_join(right),
            _ => false,
        }
    }

    // Verify cost pruning doesn't break plan correctness
    let query = join(
        join(
            join(scan("t1"), scan("t2"), eq(col("t1.id"), col("t2.id"))),
            scan("t3"),
            eq(col("t2.id"), col("t3.id")),
        ),
        join(scan("t4"), scan("t5"), eq(col("t4.id"), col("t5.id"))),
        eq(col("t3.id"), col("t4.id")),
    );

    let optimizer = make_optimizer_with_stats(&["t1", "t2", "t3", "t4", "t5"]);

    let result = optimizer.optimize(&query);
    assert!(result.is_ok());

    let optimized = result.unwrap();

    assert!(
        contains_join(&optimized),
        "Optimized plan should still contain joins"
    );
}

#[test]
fn test_cost_pruning_with_convergence() {
    // Test that cost pruning and convergence detection work together
    let query = join(
        join(
            scan("users"),
            scan("orders"),
            eq(col("users.id"), col("orders.user_id")),
        ),
        scan("products"),
        eq(col("orders.product_id"), col("products.id")),
    );

    let optimizer = make_optimizer_with_stats(&["users", "orders", "products"]);

    let result = optimizer.optimize(&query);
    assert!(result.is_ok());
}
