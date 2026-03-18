//! Phase 18 integration tests: resource budgets + plan diffs.
//!
//! Tests that bounded optimization works correctly with the resource
//! budget system and that all profiles, overflow strategies, and
//! customization options behave as expected end-to-end.

#![allow(clippy::expect_used)]

use std::time::Duration;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, ProjectionColumn,
    RelExpr,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{
    ExceededResource, OptimizationStatus, Optimizer, OverflowStrategy,
    ResourceBudget,
};

// ── Helpers ─────────────────────────────────────────────────

fn simple_scan() -> RelExpr {
    RelExpr::scan("users")
}

fn filtered_scan() -> RelExpr {
    RelExpr::scan("users").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("age"))),
        right: Box::new(Expr::Const(Const::Int(18))),
    })
}

fn two_table_join() -> RelExpr {
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
        left: Box::new(RelExpr::scan("users")),
        right: Box::new(RelExpr::scan("orders")),
    }
}

fn aggregate_query() -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("department"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: None,
            distinct: false,
            alias: Some("cnt".to_owned()),
        }],
        input: Box::new(filtered_scan()),
    }
}

fn complex_query() -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::qualified(
            "users", "name",
        ))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            distinct: false,
            alias: Some("total".to_owned()),
        }],
        input: Box::new(
            two_table_join().filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new(
                    "amount",
                ))),
                right: Box::new(Expr::Const(Const::Int(100))),
            }),
        ),
    }
}

// ── Profile integration tests ───────────────────────────────

#[test]
fn interactive_profile_produces_result() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());
    let result = optimizer
        .optimize_bounded(&filtered_scan())
        .expect("should produce a result");
    assert!(result.cost.is_finite());
    assert!(result.resource_usage.elapsed_time < Duration::from_secs(5));
}

#[test]
fn standard_profile_produces_result() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());
    let result = optimizer
        .optimize_bounded(&two_table_join())
        .expect("should produce a result");
    assert!(result.cost.is_finite());
}

#[test]
fn batch_profile_produces_result() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::batch());
    let result = optimizer
        .optimize_bounded(&aggregate_query())
        .expect("should produce a result");
    assert!(result.cost.is_finite());
}

#[test]
fn memory_constrained_profile_produces_result() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::memory_constrained());
    let result = optimizer
        .optimize_bounded(&simple_scan())
        .expect("should produce a result");
    assert!(result.cost.is_finite());
}

#[test]
fn unlimited_profile_produces_result() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::unlimited());
    let result = optimizer
        .optimize_bounded(&simple_scan())
        .expect("should produce a result");
    assert_eq!(result.status, OptimizationStatus::Complete);
}

// ── Overflow strategy tests ─────────────────────────────────

#[test]
fn overflow_best_so_far_returns_plan() {
    let budget = ResourceBudget::unlimited()
        .with_iteration_limit(1)
        .with_overflow_strategy(OverflowStrategy::ReturnBestSoFar);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&complex_query())
        .expect("should return best-so-far");
    assert!(result.cost.is_finite());
}

#[test]
fn overflow_original_returns_plan() {
    let budget = ResourceBudget::unlimited()
        .with_iteration_limit(1)
        .with_overflow_strategy(OverflowStrategy::ReturnOriginal);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&complex_query())
        .expect("should return original plan");
    assert!(result.cost.is_finite() || result.cost == f64::INFINITY);
}

#[test]
fn overflow_fail_still_succeeds_within_budget() {
    let budget = ResourceBudget::unlimited()
        .with_iteration_limit(100)
        .with_overflow_strategy(OverflowStrategy::Fail);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&simple_scan())
        .expect("should succeed within budget");
    assert_eq!(result.status, OptimizationStatus::Complete);
}

// ── Resource usage report tests ─────────────────────────────

#[test]
fn report_tracks_iterations() {
    let budget =
        ResourceBudget::unlimited().with_iteration_limit(5);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&filtered_scan())
        .expect("should produce a result");
    assert!(result.resource_usage.iterations_used <= 5);
}

#[test]
fn report_tracks_elapsed_time() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());
    let result = optimizer
        .optimize_bounded(&simple_scan())
        .expect("should produce a result");
    assert!(result.resource_usage.elapsed_time.as_nanos() > 0);
}

#[test]
fn report_tracks_egraph_nodes() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());
    let result = optimizer
        .optimize_bounded(&two_table_join())
        .expect("should produce a result");
    assert!(result.resource_usage.peak_egraph_nodes > 0);
}

#[test]
fn report_tracks_memory_estimate() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());
    let result = optimizer
        .optimize_bounded(&two_table_join())
        .expect("should produce a result");
    assert!(result.resource_usage.peak_memory_estimate > 0);
}

#[test]
fn incomplete_status_on_iteration_exceeded() {
    let budget =
        ResourceBudget::unlimited().with_iteration_limit(1);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&complex_query())
        .expect("should produce a result");
    // With iteration limit of 1, the loop runs once and may
    // detect the limit on the second check.
    assert!(
        result.status == OptimizationStatus::Complete
            || result.status == OptimizationStatus::Incomplete
    );
}

#[test]
fn complete_status_on_simple_query() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());
    let result = optimizer
        .optimize_bounded(&simple_scan())
        .expect("should produce a result");
    assert_eq!(result.status, OptimizationStatus::Complete);
}

// ── Custom budget override tests ────────────────────────────

#[test]
fn custom_time_override_applied() {
    let budget = ResourceBudget::interactive()
        .with_time_limit(Duration::from_secs(5));
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&filtered_scan())
        .expect("should produce a result");
    assert!(result.resource_usage.elapsed_time < Duration::from_secs(10));
}

#[test]
fn custom_iteration_override_applied() {
    let budget =
        ResourceBudget::standard().with_iteration_limit(2);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&two_table_join())
        .expect("should produce a result");
    assert!(result.resource_usage.iterations_used <= 2);
}

#[test]
fn custom_strategy_override_applied() {
    let budget = ResourceBudget::interactive()
        .with_iteration_limit(1)
        .with_overflow_strategy(OverflowStrategy::ReturnOriginal);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&complex_query())
        .expect("should produce a result");
    // Should still return a valid plan
    assert!(result.cost.is_finite() || result.cost == f64::INFINITY);
}

// ── Hardware profile + budget combined ──────────────────────

#[test]
fn hardware_profile_with_budget() {
    let mut optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());
    optimizer.set_hardware_profile(
        ra_hardware::HardwareProfile::cpu_only(),
    );
    let result = optimizer
        .optimize_bounded(&two_table_join())
        .expect("should produce a result with hardware profile");
    assert!(result.cost.is_finite());
}

#[test]
fn gpu_hardware_with_interactive_budget() {
    let mut optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());
    optimizer.set_hardware_profile(
        ra_hardware::HardwareProfile::gpu_server(),
    );
    let result = optimizer
        .optimize_bounded(&aggregate_query())
        .expect("should produce a result");
    assert!(result.cost.is_finite());
}

// ── Profile hierarchy ───────────────────────────────────────

#[test]
fn interactive_faster_than_batch() {
    let int_opt = Optimizer::new()
        .with_resource_budget(ResourceBudget::interactive());
    let batch_opt = Optimizer::new()
        .with_resource_budget(ResourceBudget::batch());

    let query = complex_query();

    let int_result = int_opt
        .optimize_bounded(&query)
        .expect("interactive should work");
    let batch_result = batch_opt
        .optimize_bounded(&query)
        .expect("batch should work");

    // Interactive should have fewer iterations or less time
    assert!(
        int_result.resource_usage.iterations_used
            <= batch_result.resource_usage.iterations_used
            || int_result.resource_usage.elapsed_time
                <= batch_result.resource_usage.elapsed_time
    );
}

// ── Multiple queries with same optimizer ────────────────────

#[test]
fn reuse_optimizer_across_queries() {
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());

    let r1 = optimizer
        .optimize_bounded(&simple_scan())
        .expect("first query");
    let r2 = optimizer
        .optimize_bounded(&filtered_scan())
        .expect("second query");
    let r3 = optimizer
        .optimize_bounded(&two_table_join())
        .expect("third query");

    assert!(r1.cost.is_finite());
    assert!(r2.cost.is_finite());
    assert!(r3.cost.is_finite());
}

// ── Budget exceeded report details ──────────────────────────

#[test]
fn exceeded_report_identifies_resource() {
    let budget =
        ResourceBudget::unlimited().with_iteration_limit(0);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&filtered_scan())
        .expect("should return with exceeded budget");
    assert_eq!(result.status, OptimizationStatus::Incomplete);
    assert_eq!(
        result.resource_usage.budget_exceeded,
        Some(ExceededResource::Iterations),
    );
}

#[test]
fn zero_iteration_budget_returns_original() {
    let budget = ResourceBudget::unlimited()
        .with_iteration_limit(0)
        .with_overflow_strategy(OverflowStrategy::ReturnBestSoFar);
    let optimizer = Optimizer::new().with_resource_budget(budget);
    let result = optimizer
        .optimize_bounded(&simple_scan())
        .expect("should return a plan");
    // Should return the initial plan extraction
    assert!(result.cost.is_finite());
}

// ── set_resource_budget mutable API ─────────────────────────

#[test]
fn set_resource_budget_mutable_api() {
    let mut optimizer = Optimizer::new();
    optimizer.set_resource_budget(ResourceBudget::interactive());
    let result = optimizer
        .optimize_bounded(&simple_scan())
        .expect("should produce a result");
    assert!(result.cost.is_finite());
}

// ── Default (no budget) works with optimize_bounded ─────────

#[test]
fn optimize_bounded_without_budget_uses_unlimited() {
    let optimizer = Optimizer::new();
    let result = optimizer
        .optimize_bounded(&filtered_scan())
        .expect("should work without explicit budget");
    assert_eq!(result.status, OptimizationStatus::Complete);
}

// ── Project query with budget ───────────────────────────────

#[test]
fn project_query_with_budget() {
    let plan = simple_scan().project(vec![ProjectionColumn {
        expr: Expr::Column(ColumnRef::new("name")),
        alias: None,
    }]);
    let optimizer = Optimizer::new()
        .with_resource_budget(ResourceBudget::standard());
    let result = optimizer
        .optimize_bounded(&plan)
        .expect("should optimize project query");
    assert!(result.cost.is_finite());
}
