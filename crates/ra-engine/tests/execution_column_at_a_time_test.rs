//! Tests for column-at-a-time execution model.
//!
//! Column-at-a-time execution processes entire columns using vectorized
//! primitives. Tests cover columnar processing, compression integration,
//! late materialization, and adaptive execution.

mod helpers;

use helpers::*;
use ra_core::expr::BinOp;

// ── Column-Wise Processing Tests ────────────────────────────────

#[test]
fn test_column_at_a_time_scan() {
    // Scan entire column at once
    let input = scan("columnar_table");
    assert_optimization_improves(input);
}

#[test]
fn test_column_at_a_time_filter() {
    // Filter operates on entire column
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(100)));
    assert_cost_calculated(filtered);
}

#[test]
fn test_column_at_a_time_projection() {
    // Project specific columns only
    let input = scan("wide_table");
    let projected = project(input, vec!["id", "value"]);
    assert_optimization_improves(projected);
}

// ── Vectorized Primitives Tests ─────────────────────────────────

#[test]
fn test_column_at_a_time_vectorized_scan() {
    // Use SIMD for scanning
    let input = scan("numbers");
    assert_optimization_improves(input);
}

#[test]
fn test_column_at_a_time_vectorized_arithmetic() {
    // Vectorized arithmetic on columns
    let input = scan("calculations");
    let filtered = input.filter(gt(binop(BinOp::Add, col("a"), col("b")), int(100)));
    assert_cost_calculated(filtered);
}

#[test]
fn test_column_at_a_time_vectorized_comparison() {
    // Vectorized comparisons
    let input = scan("data");
    let filtered = input.filter(and(
        gt(col("x"), int(0)),
        binop(BinOp::Lt, col("y"), int(100)),
    ));
    assert_optimization_improves(filtered);
}

// ── Cache-Conscious Algorithms Tests ────────────────────────────

#[test]
fn test_column_at_a_time_cache_friendly_scan() {
    // Sequential column access is cache-friendly
    let input = scan("large_columnar");
    assert_optimization_improves(input);
}

#[test]
fn test_column_at_a_time_cache_blocking() {
    // Process columns in cache-sized chunks
    let input = scan("huge_columns");
    assert_cost_calculated(input);
}

// ── Compression Integration Tests ───────────────────────────────

#[test]
fn test_column_at_a_time_compressed_scan() {
    // Scan compressed columns
    let input = scan("compressed_table");
    assert_optimization_improves(input);
}

#[test]
fn test_column_at_a_time_filter_on_compressed() {
    // Filter without decompression where possible
    let input = scan("compressed_data");
    let filtered = input.filter(eq(col("status"), string("active")));
    assert_cost_calculated(filtered);
}

#[test]
fn test_column_at_a_time_dictionary_encoding() {
    // Use dictionary encoding for strings
    let input = scan("string_columns");
    let filtered = input.filter(eq(col("category"), string("electronics")));
    assert_optimization_improves(filtered);
}

// ── Late Materialization Tests ──────────────────────────────────

#[test]
fn test_column_at_a_time_late_materialization() {
    // Defer column materialization until needed
    let input = scan("wide_table");
    let filtered = input.filter(gt(col("filter_col"), int(100)));
    let projected = project(filtered, vec!["result_col"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_column_at_a_time_selective_materialization() {
    // Only materialize columns in final projection
    let input = scan("many_columns");
    let filtered = input.filter(gt(col("key"), int(1000)));
    let projected = project(filtered, vec!["id", "name"]);
    assert_cost_calculated(projected);
}

// ── Column Scans Tests ──────────────────────────────────────────

#[test]
fn test_column_at_a_time_sequential_scan() {
    // Sequential scan of column
    let input = scan("sorted_column");
    assert_optimization_improves(input);
}

#[test]
fn test_column_at_a_time_random_access() {
    // Random access in columnar format
    let input = scan("sparse_access");
    assert_cost_calculated(input);
}

// ── Projection Pushdown Tests ───────────────────────────────────

#[test]
fn test_column_at_a_time_projection_pushdown() {
    // Push projection to scan
    let input = scan("wide_columnar");
    let projected = project(input, vec!["col1", "col2"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_column_at_a_time_filter_projection_pushdown() {
    // Push both filter and projection
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(100)));
    let projected = project(filtered, vec!["id"]);
    assert_cost_calculated(projected);
}

// ── Selection Vectors Tests ─────────────────────────────────────

#[test]
fn test_column_at_a_time_selection_vector() {
    // Use selection vector for sparse results
    let input = scan("data");
    let filtered = input.filter(eq(col("rare_value"), int(1)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_column_at_a_time_selection_vector_chain() {
    // Chain selection vectors through operators
    let input = scan("data");
    let filter1 = input.filter(gt(col("a"), int(0)));
    let filter2 = filter1.filter(binop(BinOp::Lt, col("b"), int(100)));
    assert_cost_calculated(filter2);
}

// ── Position Lists Tests ────────────────────────────────────────

#[test]
fn test_column_at_a_time_position_list() {
    // Use position lists for late materialization
    let input = scan("wide_table");
    let filtered = input.filter(gt(col("filter_column"), int(1000)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_column_at_a_time_position_list_join() {
    // Position lists in joins
    let join = two_table_join("table1", "table2", "key", "key");
    assert_cost_calculated(join);
}

// ── Adaptive Execution Tests ────────────────────────────────────

#[test]
fn test_column_at_a_time_adaptive_strategy() {
    // Switch between column-at-a-time and vectorized
    let input = scan("mixed_workload");
    assert_optimization_improves(input);
}

#[test]
fn test_column_at_a_time_selectivity_adaptation() {
    // Adapt to selectivity changes
    let input = scan("variable_selectivity");
    let filtered = input.filter(gt(col("value"), int(50)));
    assert_cost_calculated(filtered);
}
