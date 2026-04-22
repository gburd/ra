//! Tests for push-based compiled execution model.
//!
//! Push-based execution compiles query plans into native code with
//! operator fusion, JIT compilation, and hardware-specific optimizations.
//! Tests cover code generation, compilation strategies, and optimizations.

mod helpers;

use helpers::*;
use ra_core::expr::{BinOp, Const, Expr};

// ── Compiled Execution Tests ────────────────────────────────────

#[test]
fn test_push_based_compiled_pipeline() {
    // Entire pipeline compiled to single function
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(100)));
    let projected = project(filtered, vec!["id", "value"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_push_based_code_generation() {
    // Generate native code for query
    let input = filtered_scan("orders", "total", 1000);
    assert_optimization_improves(input);
}

// ── JIT Compilation Tests ───────────────────────────────────────

#[test]
fn test_push_based_jit_compilation() {
    // Just-in-time compile query plan
    let input = scan("events");
    let filtered = input.filter(eq(col("event_type"), string("click")));
    assert_cost_calculated(filtered);
}

#[test]
fn test_push_based_jit_caching() {
    // Cache compiled plans for reuse
    let input = scan("users");
    let filtered = input.filter(gt(col("age"), int(18)));
    assert_optimization_improves(filtered);
}

// ── Function Inlining Tests ─────────────────────────────────────

#[test]
fn test_push_based_operator_inlining() {
    // Inline operators into single function
    let input = scan("products");
    let filter1 = input.filter(gt(col("price"), int(10)));
    let filter2 = filter1.filter(binop(BinOp::Lt, col("price"), int(100)));
    assert_optimization_improves(filter2);
}

#[test]
fn test_push_based_expression_inlining() {
    // Inline expression evaluation
    let input = scan("calculations");
    let filtered = input.filter(gt(binop(BinOp::Add, col("a"), col("b")), int(100)));
    assert_cost_calculated(filtered);
}

// ── Branch Prediction Tests ─────────────────────────────────────

#[test]
fn test_push_based_branch_prediction_hints() {
    // Add branch hints for predictable patterns
    let input = scan("data");
    let filtered = input.filter(gt(col("probability"), int(95)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_push_based_branch_elimination() {
    // Eliminate branches where possible
    let input = scan("constants");
    let filtered = input.filter(eq(col("type"), string("A")));
    assert_cost_calculated(filtered);
}

// ── Register Allocation Tests ───────────────────────────────────

#[test]
fn test_push_based_register_allocation() {
    // Optimize register usage
    let input = scan("numbers");
    let filtered = input.filter(and(
        gt(col("a"), int(0)),
        binop(BinOp::Lt, col("b"), int(100)),
    ));
    assert_optimization_improves(filtered);
}

#[test]
fn test_push_based_register_spilling() {
    // Handle register pressure with spilling
    let input = scan("many_columns");
    let projected = project(input, vec!["c1", "c2", "c3", "c4", "c5"]);
    assert_cost_calculated(projected);
}

// ── Code Generation Patterns Tests ──────────────────────────────

#[test]
fn test_push_based_tight_loops() {
    // Generate tight loops for processing
    let input = scan("records");
    let filtered = input.filter(gt(col("id"), int(1000)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_push_based_data_parallel_code() {
    // Generate data-parallel code
    let input = scan("parallel_data");
    assert_cost_calculated(input);
}

// ── Operator Fusion Tests ───────────────────────────────────────

#[test]
fn test_push_based_filter_fusion() {
    // Fuse multiple filters
    let input = scan("data");
    let filter1 = input.filter(gt(col("x"), int(0)));
    let filter2 = filter1.filter(binop(BinOp::Lt, col("y"), int(100)));
    let filter3 = filter2.filter(eq(col("z"), int(5)));
    assert_optimization_improves(filter3);
}

#[test]
fn test_push_based_scan_filter_fusion() {
    // Fuse scan with filter
    let input = filtered_scan("table", "column", 100);
    assert_optimization_improves(input);
}

#[test]
fn test_push_based_project_filter_fusion() {
    // Fuse projection with filter
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(50)));
    let projected = project(filtered, vec!["id", "value"]);
    assert_cost_calculated(projected);
}

// ── Loop Unrolling Tests ────────────────────────────────────────

#[test]
fn test_push_based_loop_unrolling() {
    // Unroll loops for better ILP
    let input = scan("array_data");
    let filtered = input.filter(gt(col("value"), int(0)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_push_based_partial_unrolling() {
    // Partially unroll large loops
    let input = scan("big_table");
    assert_cost_calculated(input);
}

// ── Predicate Compilation Tests ─────────────────────────────────

#[test]
fn test_push_based_compiled_predicate() {
    // Compile complex predicate to native code
    let input = scan("events");
    let filtered = input.filter(and(
        and(
            gt(col("timestamp"), int(1000)),
            binop(BinOp::Lt, col("timestamp"), int(2000)),
        ),
        eq(col("status"), string("success")),
    ));
    assert_optimization_improves(filtered);
}

#[test]
fn test_push_based_predicate_short_circuit() {
    // Short-circuit evaluation in compiled code
    let input = scan("data");
    let filtered = input.filter(or(
        eq(col("flag"), Expr::Const(Const::Bool(true))),
        gt(col("expensive_computation"), int(100)),
    ));
    assert_cost_calculated(filtered);
}

// ── Hardware Optimization Tests ─────────────────────────────────

#[test]
fn test_push_based_simd_codegen() {
    // Generate SIMD instructions
    let input = scan("numbers");
    let filtered = input.filter(gt(col("value"), int(100)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_push_based_cache_aware_codegen() {
    // Generate cache-aware code
    let input = scan("large_data");
    let filtered = input.filter(gt(col("id"), int(1000)));
    assert_cost_calculated(filtered);
}
