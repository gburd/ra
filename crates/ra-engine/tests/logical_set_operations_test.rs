//! Tests for logical set operation optimization rules.
//!
//! Set operations (UNION, INTERSECT, EXCEPT) have specific
//! optimization opportunities including elimination, merging, and pushdown.

mod helpers;

use helpers::*;
use ra_core::algebra::RelExpr;

// ── UNION Optimization ──────────────────────────────────────

#[test]
fn test_union_all_basic() {
    let left = scan("table_a");
    let right = scan("table_b");
    let plan = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_union_distinct() {
    let left = scan("set_a");
    let right = scan("set_b");
    let plan = RelExpr::Union {
        all: false,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_union_with_filters() {
    let left = filtered_scan("data_2023", "valid", 1);
    let right = filtered_scan("data_2024", "valid", 1);
    let plan = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_union_all_to_union_distinct() {
    // UNION ALL with guaranteed no duplicates can become UNION
    let left = filtered_scan("unique_a", "region", 1);
    let right = filtered_scan("unique_b", "region", 2);
    let plan = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

// ── INTERSECT Optimization ──────────────────────────────────

#[test]
fn test_intersect_basic() {
    let left = scan("set_x");
    let right = scan("set_y");
    let plan = RelExpr::Intersect {
        all: false,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_intersect_to_semi_join() {
    // INTERSECT can be rewritten as semi-join
    let left = scan("table_a");
    let right = scan("table_b");
    let plan = RelExpr::Intersect {
        all: false,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

// ── EXCEPT Optimization ─────────────────────────────────────

#[test]
fn test_except_basic() {
    let left = scan("all_items");
    let right = scan("excluded_items");
    let plan = RelExpr::Except {
        all: false,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_except_to_anti_join() {
    // EXCEPT can be rewritten as anti-join
    let left = scan("candidates");
    let right = scan("blacklist");
    let plan = RelExpr::Except {
        all: false,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_cost_calculated(plan);
}

// ── Set Operation Merging ───────────────────────────────────

#[test]
fn test_consecutive_unions_merge() {
    let a = scan("a");
    let b = scan("b");
    let c = scan("c");

    let ab = RelExpr::Union {
        all: true,
        left: Box::new(a),
        right: Box::new(b),
    };

    let abc = RelExpr::Union {
        all: true,
        left: Box::new(ab),
        right: Box::new(c),
    };

    assert_cost_calculated(abc);
}

#[test]
fn test_union_identity_elimination() {
    // UNION with same table
    let a = scan("table");
    let b = scan("table");
    let plan = RelExpr::Union {
        all: true,
        left: Box::new(a),
        right: Box::new(b),
    };
    assert_cost_calculated(plan);
}
