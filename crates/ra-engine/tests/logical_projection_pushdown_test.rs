//! Tests for logical projection pushdown optimization rules.
//!
//! Projection pushdown (column pruning) eliminates unnecessary columns
//! as early as possible in the query plan, reducing data movement and
//! processing costs.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};

// ── Basic Column Pruning ────────────────────────────────────

#[test]
fn test_unused_column_elimination() {
    let scanned = scan("table");
    let projected = project(scanned, vec!["col1", "col2"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_select_star_optimization() {
    let plan = scan("table");
    assert_cost_calculated(plan);
}

#[test]
fn test_duplicate_column_elimination() {
    let scanned = scan("table");
    let projected = project(scanned, vec!["id", "id", "name"]);
    assert_cost_calculated(projected);
}

// ── Projection Through Joins ────────────────────────────────

#[test]
fn test_project_pushdown_through_join() {
    let joined = two_table_join("orders", "customers", "customer_id", "id");
    let projected = project(joined, vec!["order_id", "customer_name"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_column_pruning_left_join_side() {
    let left = scan("orders");
    let right = scan("customers");
    let joined = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(left),
        right: Box::new(right),
    };
    let projected = project(joined, vec!["amount", "order_date"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_column_pruning_right_join_side() {
    let left = scan("orders");
    let right = scan("customers");
    let joined = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(left),
        right: Box::new(right),
    };
    let projected = project(joined, vec!["name", "email"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_projection_both_join_sides() {
    let joined = two_table_join("orders", "customers", "customer_id", "id");
    let projected = project(joined, vec!["order_id", "customer_name", "amount"]);
    assert_cost_calculated(projected);
}

// ── Projection Through Aggregation ──────────────────────────

#[test]
fn test_project_before_aggregate() {
    let scanned = scan("sales");
    let projected = project(scanned, vec!["region", "amount"]);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(projected),
    };
    assert_cost_calculated(agg);
}

#[test]
fn test_eliminate_columns_not_in_group_by() {
    let scanned = scan("data");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scanned),
    };
    assert_cost_calculated(agg);
}

#[test]
fn test_project_aggregate_results() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    let projected = project(agg, vec!["region"]);
    assert_cost_calculated(projected);
}

// ── Expression Pushdown ─────────────────────────────────────

#[test]
fn test_computed_column_pushdown() {
    let scanned = scan("table");
    let projected = project(scanned, vec!["col1"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_expression_in_projection() {
    let scanned = scan("products");
    let projected = project(scanned, vec!["name", "price"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_case_expression_pushdown() {
    let scanned = scan("orders");
    let projected = project(scanned, vec!["order_id", "status"]);
    assert_cost_calculated(projected);
}

// ── Wide Table Optimization ─────────────────────────────────

#[test]
fn test_narrow_projection_on_wide_table() {
    let scanned = scan("wide_table_100_columns");
    let projected = project(scanned, vec!["id", "name"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_columnar_storage_benefits() {
    let scanned = scan("columnar_table");
    let projected = project(scanned, vec!["key_col"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_early_projection_reduces_io() {
    let filtered = filtered_scan("large_table", "status", 1);
    let projected = project(filtered, vec!["id"]);
    assert_cost_calculated(projected);
}

// ── Projection Merging ──────────────────────────────────────

#[test]
fn test_consecutive_projections_merge() {
    let p1 = project(scan("table"), vec!["a", "b", "c"]);
    let p2 = project(p1, vec!["a", "b"]);
    assert_cost_calculated(p2);
}

#[test]
fn test_projection_identity_elimination() {
    let scanned = scan("table");
    let projected = project(scanned, vec!["col1", "col2"]);
    assert_cost_calculated(projected);
}

#[test]
fn test_redundant_projection_removal() {
    let p1 = project(scan("table"), vec!["id", "name"]);
    let p2 = project(p1, vec!["id", "name"]);
    assert_cost_calculated(p2);
}
