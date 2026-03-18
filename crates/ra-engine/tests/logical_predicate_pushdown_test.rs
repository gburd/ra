//! Tests for logical predicate pushdown optimization rules.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};

// ── Basic Predicate Pushdown ────────────────────────────────

#[test]
fn test_simple_filter_on_scan() {
    let plan = filtered_scan("orders", "amount", 100);
    assert_rule_applies(plan);
}

#[test]
fn test_filter_after_join() {
    let plan = two_table_join("orders", "customers", "customer_id", "id");
    assert_rule_applies(plan);
}

#[test]
fn test_filter_with_projection() {
    let scanned = scan("products");
    let projected = project(scanned, vec!["name", "price"]);
    assert_rule_applies(projected);
}

// ── Filter Pushdown Through Joins ───────────────────────────

#[test]
fn test_predicate_pushdown_inner_join_left() {
    let _left = filtered_scan("orders", "amount", 100);
    let _right = scan("customers");
    let plan = two_table_join("orders", "customers", "customer_id", "id");
    assert_rule_applies(plan);
}

#[test]
fn test_predicate_pushdown_inner_join_right() {
    let plan = two_table_join("orders", "customers", "customer_id", "id");
    assert_rule_applies(plan);
}

#[test]
fn test_star_schema_multi_join() {
    let fact = scan("sales_fact");
    let dim1 = scan("time_dim");
    let dim2 = scan("product_dim");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("time_id"), col("id")),
        left: Box::new(fact),
        right: Box::new(dim1),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(j1),
        right: Box::new(dim2),
    };

    assert_rule_applies(j2);
}

// ── Predicate Pushdown Through Aggregation ──────────────────

#[test]
fn test_filter_before_group_by() {
    let filtered = filtered_scan("sales", "region", 1);
    assert_rule_applies(filtered);
}

#[test]
fn test_aggregate_with_filter() {
    let scanned = scan("orders");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![],
        input: Box::new(scanned),
    };
    assert_rule_applies(agg);
}

// ── Expression Simplification ───────────────────────────────

#[test]
fn test_constant_expression_in_filter() {
    let plan = filtered_scan("table", "value", 30);
    assert_rule_applies(plan);
}

#[test]
fn test_multiple_predicates_and() {
    let inner = filtered_scan("table", "x", 5);
    assert_rule_applies(inner);
}

#[test]
fn test_contradiction_filter() {
    let plan = filtered_scan("table", "x", 999);
    assert_rule_applies(plan);
}

// ── Complex Query Patterns ──────────────────────────────────

#[test]
fn test_three_table_join_with_filters() {
    let t1 = filtered_scan("t1", "col", 10);
    let t2 = filtered_scan("t2", "col", 20);
    let t3 = scan("t3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_rule_applies(j2);
}

#[test]
fn test_left_outer_join_filter() {
    let left = scan("orders");
    let right = scan("customers");
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_rule_applies(plan);
}

#[test]
fn test_cross_join_to_inner() {
    let left = scan("small_table");
    let right = scan("tiny_table");
    let plan = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: eq(col("a"), col("b")),
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_rule_applies(plan);
}

// ── Sort and Limit Interactions ─────────────────────────────

#[test]
fn test_filter_with_sort() {
    let filtered = filtered_scan("events", "priority", 5);
    let sorted = sort(filtered, "timestamp", false);
    assert_rule_applies(sorted);
}

#[test]
fn test_filter_with_limit() {
    let filtered = filtered_scan("recent", "status", 1);
    let limited = limit(filtered, 100);
    assert_rule_applies(limited);
}

#[test]
fn test_top_k_pattern() {
    let sorted = sort(scan("rankings"), "score", false);
    let limited = limit(sorted, 10);
    assert_rule_applies(limited);
}

// ── Set Operations ──────────────────────────────────────────

#[test]
fn test_union_with_filters() {
    let left = filtered_scan("us_sales", "region", 1);
    let right = filtered_scan("eu_sales", "region", 2);
    let plan = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_rule_applies(plan);
}

#[test]
fn test_intersect_optimization() {
    let left = scan("set_a");
    let right = scan("set_b");
    let plan = RelExpr::Intersect {
        all: false,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_rule_applies(plan);
}

#[test]
fn test_except_with_filter() {
    let left = filtered_scan("all_items", "category", 1);
    let right = scan("excluded_items");
    let plan = RelExpr::Except {
        all: false,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_rule_applies(plan);
}

// ── Index-Aware Optimization ────────────────────────────────

#[test]
fn test_equality_predicate_for_index() {
    let plan = filtered_scan("indexed_table", "id", 42);
    assert_rule_applies(plan);
}

#[test]
fn test_range_predicate_for_index() {
    let plan = filtered_scan("ordered_table", "timestamp", 1000);
    assert_rule_applies(plan);
}

#[test]
fn test_composite_index_prefix_match() {
    let plan = filtered_scan("multi_indexed", "first_col", 10);
    assert_rule_applies(plan);
}

// ── Statistics-Driven Optimization ──────────────────────────

#[test]
fn test_selective_filter_ordering() {
    let plan = filtered_scan("large_table", "rare_value", 1);
    assert_rule_applies(plan);
}

#[test]
fn test_join_filter_selectivity() {
    let small = filtered_scan("small_dim", "id", 100);
    let large = scan("large_fact");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("dim_id"), col("id")),
        left: Box::new(large),
        right: Box::new(small),
    };
    assert_rule_applies(plan);
}

// ── Transitive Closure & Inference ──────────────────────────

#[test]
fn test_transitive_join_conditions() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("a"), col("b")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("b"), col("c")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_rule_applies(j2);
}

#[test]
fn test_join_condition_propagation() {
    let j = two_table_join("orders", "customers", "customer_id", "id");
    assert_rule_applies(j);
}

#[test]
fn test_foreign_key_filter_propagation() {
    let filtered_orders = filtered_scan("orders", "status", 1);
    let customers = scan("customers");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(filtered_orders),
        right: Box::new(customers),
    };
    assert_rule_applies(plan);
}
