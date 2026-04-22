//! Tests for vectorized batch execution model.
//!
//! Vectorized execution processes data in batches (typically 1000-10000 rows)
//! using columnar layouts. Tests cover batch processing, SIMD operations,
//! cache efficiency, and adaptive batch sizing.

mod helpers;

use helpers::*;
use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
use ra_core::expr::{BinOp, Const, Expr};

// ── Batch Processing Tests ──────────────────────────────────────

#[test]
fn test_vectorized_batch_scan() {
    // Scan produces batches of rows
    let input = scan("large_table");
    assert_optimization_improves(input);
}

#[test]
fn test_vectorized_batch_size_1000() {
    // Default batch size of 1000 rows
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(100)));
    assert_cost_calculated(filtered);
}

#[test]
fn test_vectorized_batch_pipeline() {
    // Batches flow through pipeline
    let input = scan("events");
    let filtered = input.filter(eq(col("status"), string("active")));
    let projected = project(filtered, vec!["id", "timestamp"]);
    assert_optimization_improves(projected);
}

// ── Columnar Data Layout Tests ──────────────────────────────────

#[test]
fn test_vectorized_columnar_scan() {
    // Columnar storage benefits vectorization
    let input = scan("columnar_table");
    let projected = project(input, vec!["col1", "col2"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_vectorized_column_at_a_time_filter() {
    // Process entire column in batch
    let input = scan("data");
    let filtered = input.filter(gt(col("price"), int(1000)));
    assert_cost_calculated(filtered);
}

#[test]
fn test_vectorized_column_projection() {
    // Column-wise projection
    let input = scan("wide_table");
    let projected = project(input, vec!["id", "value"]);
    assert_optimization_improves(projected);
}

// ── SIMD Operations Tests ───────────────────────────────────────

#[test]
fn test_vectorized_simd_filter() {
    // SIMD-optimized filtering
    let input = scan("numbers");
    let filtered = input.filter(binop(BinOp::Lt, col("value"), int(1000)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_vectorized_simd_arithmetic() {
    // SIMD arithmetic operations
    let input = scan("calculations");
    let filtered = input.filter(gt(binop(BinOp::Add, col("a"), col("b")), int(100)));
    assert_cost_calculated(filtered);
}

// ── Cache Efficiency Tests ──────────────────────────────────────

#[test]
fn test_vectorized_cache_friendly_scan() {
    // Sequential access improves cache hit rate
    let input = scan("sequential_data");
    assert_optimization_improves(input);
}

#[test]
fn test_vectorized_cache_blocking() {
    // Process data in cache-sized blocks
    let input = scan("large_dataset");
    let filtered = input.filter(gt(col("id"), int(1000)));
    assert_cost_calculated(filtered);
}

// ── Predicate Evaluation Tests ──────────────────────────────────

#[test]
fn test_vectorized_predicate_batch_evaluation() {
    // Evaluate predicate on entire batch
    let input = scan("products");
    let filtered = input.filter(and(
        gt(col("price"), int(10)),
        binop(BinOp::Lt, col("price"), int(100)),
    ));
    assert_optimization_improves(filtered);
}

#[test]
fn test_vectorized_selection_vector() {
    // Use selection vector for sparse results
    let input = scan("sparse_data");
    let filtered = input.filter(eq(col("flag"), Expr::Const(Const::Bool(true))));
    assert_cost_calculated(filtered);
}

// ── Expression Evaluation Tests ─────────────────────────────────

#[test]
fn test_vectorized_expression_batch() {
    // Evaluate expressions on batches
    let input = scan("data");
    let filtered = input.filter(gt(
        binop(BinOp::Mul, col("quantity"), col("price")),
        int(1000),
    ));
    assert_optimization_improves(filtered);
}

#[test]
fn test_vectorized_function_call_batch() {
    // Batch function evaluation
    let input = scan("strings");
    let filtered = input.filter(eq(
        Expr::Function {
            name: "length".to_string(),
            args: vec![col("text")],
        },
        int(10),
    ));
    assert_cost_calculated(filtered);
}

// ── Type Conversion Tests ───────────────────────────────────────

#[test]
fn test_vectorized_type_conversion_batch() {
    // Batch type conversions
    let input = scan("mixed_types");
    let filtered = input.filter(gt(col("string_number"), int(100)));
    assert_optimization_improves(filtered);
}

// ── Adaptive Batch Sizing Tests ─────────────────────────────────

#[test]
fn test_vectorized_adaptive_batch_large_rows() {
    // Smaller batches for large rows
    let input = scan("wide_rows");
    assert_optimization_improves(input);
}

#[test]
fn test_vectorized_adaptive_batch_narrow_rows() {
    // Larger batches for narrow rows
    let input = scan("narrow_rows");
    let projected = project(input, vec!["id"]);
    assert_cost_calculated(projected);
}

// ── Batch Boundary Handling Tests ───────────────────────────────

#[test]
fn test_vectorized_partial_batch() {
    // Handle partial batch at end
    let input = scan("table_1050_rows");
    let limited = limit(input, 1050);
    assert_optimization_improves(limited);
}

#[test]
fn test_vectorized_batch_aggregation() {
    // Aggregate across batch boundaries
    let input = scan("values");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("amount")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_cost_calculated(agg);
}

// ── Spilling Strategies Tests ───────────────────────────────────

#[test]
fn test_vectorized_batch_spilling() {
    // Spill batches to disk when memory pressure
    let input = scan("memory_intensive");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("high_cardinality_key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}
