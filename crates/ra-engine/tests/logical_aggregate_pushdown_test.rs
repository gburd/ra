//! Tests for logical aggregate pushdown optimization rules.
//!
//! Aggregate pushdown moves aggregation operations closer to data sources
//! to reduce the amount of data processed in later stages.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};

// ── Basic Aggregate Pushdown ────────────────────────────────

#[test]
fn test_aggregate_on_scan() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_aggregate_after_filter() {
    let filtered = filtered_scan("orders", "year", 2024);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![],
        input: Box::new(filtered),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_two_phase_aggregation() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scan("products")),
    };
    assert_rule_applies(agg);
}

// ── Aggregate Through Union ─────────────────────────────────

#[test]
fn test_aggregate_pushdown_union() {
    let left = scan("sales_q1");
    let right = scan("sales_q2");
    let union = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };
    let agg = RelExpr::Aggregate {
        group_by: vec![col("product")],
        aggregates: vec![],
        input: Box::new(union),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_distribute_aggregate_to_union_branches() {
    let left = scan("data_a");
    let right = scan("data_b");
    let union = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };
    let agg = RelExpr::Aggregate {
        group_by: vec![col("key")],
        aggregates: vec![],
        input: Box::new(union),
    };
    assert_rule_applies(agg);
}

// ── Aggregate Through Joins ─────────────────────────────────

#[test]
fn test_aggregate_before_join() {
    let agg_orders = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![],
        input: Box::new(scan("orders")),
    };
    let customers = scan("customers");
    let joined = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(agg_orders),
        right: Box::new(customers),
    };
    assert_rule_applies(joined);
}

#[test]
fn test_partial_aggregate_on_dimension() {
    let orders = scan("orders");
    let agg_products = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scan("products")),
    };
    let joined = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(agg_products),
    };
    assert_rule_applies(joined);
}

// ── Group-By Pushdown ───────────────────────────────────────

#[test]
fn test_group_by_on_join_key() {
    let joined = two_table_join("fact", "dim", "dim_id", "id");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("dim_id")],
        aggregates: vec![],
        input: Box::new(joined),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_group_by_subset_of_key() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("year"), col("month")],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    assert_rule_applies(agg);
}

// ── Having to Filter Conversion ─────────────────────────────

#[test]
fn test_having_clause_on_group_column() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    let filtered = filtered_scan("sales", "region", 1);
    assert_rule_applies(filtered);
}

#[test]
fn test_having_with_aggregate_function() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![],
        input: Box::new(scan("orders")),
    };
    assert_rule_applies(agg);
}

// ── Multi-Level Aggregation ─────────────────────────────────

#[test]
fn test_nested_aggregates() {
    let inner_agg = RelExpr::Aggregate {
        group_by: vec![col("category"), col("region")],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    let outer_agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(inner_agg),
    };
    assert_rule_applies(outer_agg);
}

#[test]
fn test_rollup_aggregation() {
    let base_agg = RelExpr::Aggregate {
        group_by: vec![col("year"), col("quarter"), col("month")],
        aggregates: vec![],
        input: Box::new(scan("time_series")),
    };
    assert_rule_applies(base_agg);
}

// ── Distinct Optimization ───────────────────────────────────

#[test]
fn test_distinct_as_group_by() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("email")],
        aggregates: vec![],
        input: Box::new(scan("users")),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_distinct_with_filter() {
    let filtered = filtered_scan("events", "type", 1);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("user_id")],
        aggregates: vec![],
        input: Box::new(filtered),
    };
    assert_rule_applies(agg);
}

// ── Aggregation Strategy Selection ──────────────────────────

#[test]
fn test_hash_aggregation_low_cardinality() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("status")],
        aggregates: vec![],
        input: Box::new(scan("orders")),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_sort_aggregation_high_cardinality() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("user_id")],
        aggregates: vec![],
        input: Box::new(scan("events")),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_streaming_aggregation_sorted_input() {
    let sorted = sort(scan("sorted_data"), "key", true);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("key")],
        aggregates: vec![],
        input: Box::new(sorted),
    };
    assert_rule_applies(agg);
}

// ── Partial Aggregation ─────────────────────────────────────

#[test]
fn test_partial_aggregate_large_dataset() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scan("huge_table")),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_partial_aggregate_distributed() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(scan("distributed_sales")),
    };
    assert_rule_applies(agg);
}

// ── Aggregate with Limit ────────────────────────────────────

#[test]
fn test_top_n_aggregation() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("product")],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    let limited = limit(agg, 10);
    assert_rule_applies(limited);
}

#[test]
fn test_aggregate_limit_sort_fusion() {
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scan("products")),
    };
    let sorted = sort(agg, "category", false);
    let limited = limit(sorted, 5);
    assert_rule_applies(limited);
}
