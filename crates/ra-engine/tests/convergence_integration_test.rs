//! Integration tests for convergence detection (Task #244).
//!
//! Validates that early termination works correctly in practice.

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
fn test_convergence_terminates_early() {
    // Create a query that will converge quickly (2-3 iterations)
    let query = join(
        scan("users"),
        scan("orders"),
        eq(col("users.id"), col("orders.user_id")),
    );

    let optimizer = make_optimizer_with_stats(&["users", "orders"]);

    let result = optimizer.optimize(&query);
    assert!(result.is_ok());

    // Note: We can't easily assert the exact iteration count here without
    // modifying the optimizer to return metrics. But the test validates
    // that convergence detection doesn't break correctness.
}

#[test]
fn test_convergence_respects_complexity() {
    // Trivial query should converge very quickly
    let trivial_query = scan("users");

    let mut opt_trivial = Optimizer::new();
    let mut stats = Statistics::new(10000.0);
    stats.avg_row_size = 100;
    stats.total_size = 1_000_000;
    opt_trivial.add_table_stats("users", stats);

    let result = opt_trivial.optimize(&trivial_query);
    assert!(result.is_ok());

    // Medium query should take more iterations but still converge early
    let medium_query = join(
        join(
            join(scan("t1"), scan("t2"), eq(col("t1.id"), col("t2.id"))),
            join(scan("t3"), scan("t4"), eq(col("t3.id"), col("t4.id"))),
            eq(col("t2.id"), col("t3.id")),
        ),
        join(scan("t5"), scan("t6"), eq(col("t5.id"), col("t6.id"))),
        eq(col("t4.id"), col("t5.id")),
    );

    let optimizer_medium = make_optimizer_with_stats(&["t1", "t2", "t3", "t4", "t5", "t6"]);
    let result = optimizer_medium.optimize(&medium_query);
    assert!(result.is_ok());
}

#[test]
fn test_convergence_produces_valid_plans() {
    // Verify that early termination doesn't produce invalid plans
    let query = join(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        scan("c"),
        eq(col("b.id"), col("c.id")),
    );

    let optimizer = make_optimizer_with_stats(&["a", "b", "c"]);

    let result = optimizer.optimize(&query);
    assert!(result.is_ok());

    let optimized = result.unwrap();

    // Validate the plan structure (should still have joins)
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

    assert!(
        contains_join(&optimized),
        "Optimized plan should still contain joins"
    );
}

#[test]
fn test_convergence_can_be_disabled() {
    // Test with convergence detection disabled (fallback to fixed iterations)
    let query = join(
        scan("users"),
        scan("orders"),
        eq(col("users.id"), col("orders.user_id")),
    );

    let mut config = OptimizerConfig::default();
    config.use_adaptive_limits = false;
    config.iter_limit = 5;

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
fn test_convergence_with_filters() {
    // Test that convergence works with predicate pushdown opportunities
    let query = RelExpr::Filter {
        predicate: eq(
            col("orders.status"),
            Expr::Const(ra_core::expr::Const::String("completed".to_string())),
        ),
        input: Box::new(join(
            scan("users"),
            scan("orders"),
            eq(col("users.id"), col("orders.user_id")),
        )),
    };

    let optimizer = make_optimizer_with_stats(&["users", "orders"]);

    let result = optimizer.optimize(&query);
    assert!(result.is_ok());
}
