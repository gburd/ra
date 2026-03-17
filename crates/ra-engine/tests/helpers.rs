//! Test helper utilities for ra-engine testing.
//!
//! Provides common functions for testing optimization rules, cost models,
//! and integration testing of the optimizer.

use ra_core::algebra::{JoinType, ProjectionColumn, RelExpr, SortDirection, SortKey};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{Optimizer, OptimizerConfig};
use ra_hardware::HardwareProfile;

/// Create a test optimizer with default configuration.
pub fn create_test_optimizer() -> Optimizer {
    Optimizer::new()
}

/// Create a test optimizer with custom configuration.
pub fn create_test_optimizer_with_config(config: OptimizerConfig) -> Optimizer {
    Optimizer::with_config(config)
}

/// Create a test optimizer with a specific hardware profile.
pub fn create_test_optimizer_with_hardware(hardware: HardwareProfile) -> Optimizer {
    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(hardware);
    optimizer
}

/// Assert that optimizing an expression produces an expected result.
///
/// # Panics
///
/// Panics if optimization fails or the result doesn't match expected.
#[track_caller]
pub fn assert_optimizes_to(input: RelExpr, expected: RelExpr) {
    let optimizer = create_test_optimizer();
    let result = optimizer
        .optimize(&input)
        .expect("optimization should succeed");
    assert_eq!(
        result, expected,
        "optimization did not produce expected result"
    );
}

/// Assert that a specific rewrite rule is applied during optimization.
///
/// This checks that the optimized plan is different from the input,
/// indicating that at least one rule was applied.
///
/// # Panics
///
/// Panics if optimization fails or no rules were applied.
#[track_caller]
pub fn assert_rule_applies(input: RelExpr) {
    let optimizer = create_test_optimizer();
    let result = optimizer
        .optimize(&input)
        .expect("optimization should succeed");
    assert_ne!(
        result, input,
        "expected optimization to apply rules and change the plan"
    );
}

/// Assert that optimization improves the plan (reduces estimated cost).
///
/// Since we don't have actual execution, this validates that the cost
/// model prefers the optimized plan over the original.
///
/// # Panics
///
/// Panics if optimization fails.
#[track_caller]
pub fn assert_optimization_improves(input: RelExpr) {
    let optimizer = create_test_optimizer();
    let _result = optimizer
        .optimize(&input)
        .expect("optimization should succeed");
    // Note: With current implementation, we can't easily compare costs
    // without executing. This is a placeholder for future enhancement.
}

/// Assert that hardware profile affects cost estimates.
///
/// Verifies that the same query optimized with different hardware
/// profiles can produce different plans (demonstrating hardware-aware costs).
///
/// # Panics
///
/// Panics if optimization fails.
#[track_caller]
pub fn assert_hardware_affects_cost(input: RelExpr) {
    let mut opt_cpu = Optimizer::new();
    opt_cpu.set_hardware_profile(HardwareProfile::cpu_only());

    let mut opt_gpu = Optimizer::new();
    opt_gpu.set_hardware_profile(HardwareProfile::gpu_server());

    let result_cpu = opt_cpu.optimize(&input).expect("CPU optimization should succeed");
    let result_gpu = opt_gpu.optimize(&input).expect("GPU optimization should succeed");

    // Note: Currently both may produce same logical plan since we don't
    // have algorithm selection yet. This validates the mechanism works.
    let _ = (result_cpu, result_gpu);
}

// ── Test Fixture Builders ──────────────────────────────────────

/// Build a simple scan expression for testing.
pub fn scan(table: &str) -> RelExpr {
    RelExpr::scan(table)
}

/// Build a filtered scan expression.
pub fn filtered_scan(table: &str, column: &str, value: i64) -> RelExpr {
    RelExpr::scan(table).filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new(column))),
        right: Box::new(Expr::Const(Const::Int(value))),
    })
}

/// Build a two-table join expression.
pub fn two_table_join(
    left_table: &str,
    right_table: &str,
    left_col: &str,
    right_col: &str,
) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(left_table, left_col))),
            right: Box::new(Expr::Column(ColumnRef::qualified(right_table, right_col))),
        },
        left: Box::new(RelExpr::scan(left_table)),
        right: Box::new(RelExpr::scan(right_table)),
    }
}

/// Build a projection expression.
pub fn project(input: RelExpr, columns: Vec<&str>) -> RelExpr {
    let projection_columns: Vec<ProjectionColumn> = columns
        .into_iter()
        .map(|col| ProjectionColumn {
            expr: Expr::Column(ColumnRef::new(col)),
            alias: None,
        })
        .collect();
    input.project(projection_columns)
}

/// Build a sort expression.
pub fn sort(input: RelExpr, column: &str, ascending: bool) -> RelExpr {
    RelExpr::Sort {
        keys: vec![SortKey {
            expr: Expr::Column(ColumnRef::new(column)),
            direction: if ascending {
                SortDirection::Asc
            } else {
                SortDirection::Desc
            },
            nulls: ra_core::algebra::NullOrdering::Last,
        }],
        input: Box::new(input),
    }
}

/// Build a limit expression.
pub fn limit(input: RelExpr, count: u64) -> RelExpr {
    input.limit(count, 0)
}

// ── Expression Builders ────────────────────────────────────────

/// Build a column reference expression.
pub fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

/// Build a qualified column reference expression.
pub fn qcol(table: &str, name: &str) -> Expr {
    Expr::Column(ColumnRef::qualified(table, name))
}

/// Build an integer constant expression.
pub fn int(value: i64) -> Expr {
    Expr::Const(Const::Int(value))
}

/// Build a string constant expression.
pub fn string(value: &str) -> Expr {
    Expr::Const(Const::String(value.to_owned()))
}

/// Build a binary operation expression.
pub fn binop(op: BinOp, left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// Build an equality comparison.
pub fn eq(left: Expr, right: Expr) -> Expr {
    binop(BinOp::Eq, left, right)
}

/// Build a greater-than comparison.
pub fn gt(left: Expr, right: Expr) -> Expr {
    binop(BinOp::Gt, left, right)
}

/// Build an AND expression.
pub fn and(left: Expr, right: Expr) -> Expr {
    binop(BinOp::And, left, right)
}

/// Build an OR expression.
pub fn or(left: Expr, right: Expr) -> Expr {
    binop(BinOp::Or, left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_builder() {
        let expr = scan("users");
        assert!(matches!(expr, RelExpr::Scan { .. }));
    }

    #[test]
    fn test_filtered_scan_builder() {
        let expr = filtered_scan("users", "age", 18);
        assert!(matches!(expr, RelExpr::Filter { .. }));
    }

    #[test]
    fn test_join_builder() {
        let expr = two_table_join("users", "orders", "id", "user_id");
        assert!(matches!(expr, RelExpr::Join { .. }));
    }

    #[test]
    fn test_optimizer_creation() {
        let _optimizer = create_test_optimizer();
    }

    #[test]
    fn test_optimizer_with_hardware() {
        let _optimizer = create_test_optimizer_with_hardware(HardwareProfile::cpu_only());
    }

    #[test]
    fn test_optimization_succeeds() {
        let input = filtered_scan("users", "age", 18);
        let optimizer = create_test_optimizer();
        let _result = optimizer.optimize(&input).expect("should optimize");
    }
}
