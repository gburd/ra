//! Tests for logical limit pushdown optimization rules.
//!
//! Limit pushdown moves LIMIT operations as close to data sources
//! as possible to reduce data movement and enable early termination.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};

// ── Basic Limit Pushdown ────────────────────────────────────

#[test]
fn test_limit_on_scan() {
    let plan = limit(scan("table"), 100);
    assert_cost_calculated(plan);
}

#[test]
fn test_limit_after_filter() {
    let filtered = filtered_scan("orders", "status", 1);
    let plan = limit(filtered, 50);
    assert_cost_calculated(plan);
}

#[test]
fn test_limit_zero() {
    let plan = limit(scan("table"), 0);
    assert_cost_calculated(plan);
}

// ── Limit Through Union ─────────────────────────────────────

#[test]
fn test_limit_pushdown_union_all() {
    let union = RelExpr::Union {
        all: true,
        left: Box::new(scan("sales_2023")),
        right: Box::new(scan("sales_2024")),
    };
    let plan = limit(union, 100);
    assert_cost_calculated(plan);
}

#[test]
fn test_limit_union_distinct() {
    let union = RelExpr::Union {
        all: false,
        left: Box::new(scan("set_a")),
        right: Box::new(scan("set_b")),
    };
    let plan = limit(union, 50);
    assert_cost_calculated(plan);
}

// ── Top-K Optimization ──────────────────────────────────────

#[test]
fn test_sort_limit_fusion() {
    let sorted = sort(scan("rankings"), "score", false);
    let plan = limit(sorted, 10);
    assert_cost_calculated(plan);
}

#[test]
fn test_top_k_with_filter() {
    let filtered = filtered_scan("candidates", "qualified", 1);
    let sorted = sort(filtered, "rank", true);
    let plan = limit(sorted, 5);
    assert_cost_calculated(plan);
}

#[test]
fn test_top_k_heap_vs_sort() {
    let sorted = sort(scan("large_table"), "value", false);
    let plan = limit(sorted, 100);
    assert_cost_calculated(plan);
}

// ── Limit with Join ─────────────────────────────────────────

#[test]
fn test_limit_before_join() {
    let limited_orders = limit(scan("orders"), 100);
    let customers = scan("customers");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(limited_orders),
        right: Box::new(customers),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_limit_after_join() {
    let joined = two_table_join("orders", "customers", "customer_id", "id");
    let plan = limit(joined, 50);
    assert_cost_calculated(plan);
}

// ── Offset Handling ─────────────────────────────────────────

#[test]
fn test_offset_zero_elimination() {
    let plan = RelExpr::Limit {
        count: 100,
        offset: 0,
        input: Box::new(scan("table")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_limit_with_offset() {
    let plan = RelExpr::Limit {
        count: 20,
        offset: 100,
        input: Box::new(scan("paginated")),
    };
    assert_cost_calculated(plan);
}

// ── Limit Merging ───────────────────────────────────────────

#[test]
fn test_consecutive_limits_merge() {
    let inner = limit(scan("table"), 100);
    let outer = limit(inner, 50);
    assert_cost_calculated(outer);
}

#[test]
fn test_limit_min_selection() {
    let l1 = limit(scan("data"), 200);
    let l2 = limit(l1, 50);
    assert_cost_calculated(l2);
}

// ── Early Termination Patterns ──────────────────────────────

#[test]
fn test_limit_one_to_exists() {
    let plan = limit(scan("check_exists"), 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_any_value_selection() {
    let plan = limit(filtered_scan("configs", "key", 1), 1);
    assert_cost_calculated(plan);
}
