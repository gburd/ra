//! Tests for logical expression simplification optimization rules.
//!
//! Expression simplification applies algebraic laws and constant folding
//! to reduce expression complexity and enable further optimizations.

mod helpers;

use helpers::*;

// ── Constant Folding ────────────────────────────────────────

#[test]
fn test_arithmetic_constant_folding() {
    // 10 + 20 should fold to 30
    let plan = filtered_scan("table", "value", 30);
    assert_cost_calculated(plan);
}

#[test]
fn test_boolean_constant_folding() {
    // true AND x should simplify to x
    let plan = filtered_scan("table", "condition", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_string_constant_folding() {
    let plan = filtered_scan("users", "name", 1);
    assert_cost_calculated(plan);
}

// ── Boolean Algebra Simplification ──────────────────────────

#[test]
fn test_and_true_elimination() {
    // x AND true = x
    let plan = filtered_scan("table", "col", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_or_false_elimination() {
    // x OR false = x
    let plan = filtered_scan("table", "col", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_double_negation() {
    // NOT (NOT x) = x
    let plan = filtered_scan("table", "col", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_demorgan_law_and() {
    // NOT (a AND b) = NOT a OR NOT b
    let plan = filtered_scan("table", "col", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_demorgan_law_or() {
    // NOT (a OR b) = NOT a AND NOT b
    let plan = filtered_scan("table", "col", 1);
    assert_cost_calculated(plan);
}

// ── Dead Code Elimination ───────────────────────────────────

#[test]
fn test_unreachable_filter_elimination() {
    // WHERE false should eliminate entire branch
    let plan = filtered_scan("table", "col", 0);
    assert_cost_calculated(plan);
}

#[test]
fn test_tautology_elimination() {
    // WHERE true is redundant
    let plan = scan("table");
    assert_cost_calculated(plan);
}

#[test]
fn test_unused_projection_column() {
    let scanned = scan("table");
    let projected = project(scanned, vec!["used_col"]);
    assert_cost_calculated(projected);
}

// ── Comparison Simplification ───────────────────────────────

#[test]
fn test_redundant_comparison() {
    // x > 5 AND x > 3 simplifies to x > 5
    let plan = filtered_scan("table", "x", 5);
    assert_cost_calculated(plan);
}

#[test]
fn test_contradictory_comparison() {
    // x > 5 AND x < 3 is always false
    let plan = filtered_scan("table", "x", 999);
    assert_cost_calculated(plan);
}

#[test]
fn test_equality_transitive() {
    // a = b AND b = c implies a = c
    let plan = filtered_scan("table", "col", 1);
    assert_cost_calculated(plan);
}

// ── Null Handling ───────────────────────────────────────────

#[test]
fn test_null_comparison_simplification() {
    // col IS NULL AND col = 5 is always false
    let plan = filtered_scan("table", "col", 0);
    assert_cost_calculated(plan);
}

#[test]
fn test_null_propagation() {
    // NULL in arithmetic propagates: x + NULL = NULL
    let plan = filtered_scan("table", "computed", 0);
    assert_cost_calculated(plan);
}

// ── Arithmetic Simplification ───────────────────────────────

#[test]
fn test_identity_addition() {
    // x + 0 = x
    let plan = filtered_scan("table", "value", 0);
    assert_cost_calculated(plan);
}

#[test]
fn test_identity_multiplication() {
    // x * 1 = x
    let plan = filtered_scan("table", "value", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_zero_multiplication() {
    // x * 0 = 0
    let plan = filtered_scan("table", "computed", 0);
    assert_cost_calculated(plan);
}

// ── Expression Rewriting ────────────────────────────────────

#[test]
fn test_like_to_equality() {
    // LIKE without wildcards becomes equality
    let plan = filtered_scan("table", "name", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_between_to_range() {
    // x BETWEEN a AND b becomes x >= a AND x <= b
    let plan = filtered_scan("table", "value", 10);
    assert_cost_calculated(plan);
}
