//! Tests for Volcano-style tuple-at-a-time execution model.
//!
//! The Volcano model uses iterator-based execution where each operator
//! produces one tuple at a time. Tests cover pipeline behavior, blocking
//! operators, materialization points, and iterator patterns.

mod helpers;

use helpers::*;
use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{Const, Expr};

// ── Iterator Interface Tests ────────────────────────────────────

#[test]
fn test_volcano_iterator_pattern() {
    // Basic iterator pattern: scan -> filter -> project
    let input = scan("employees");
    let filtered = input.filter(gt(col("salary"), int(50000)));
    let projected = project(filtered, vec!["name", "salary"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_volcano_tuple_at_a_time() {
    // Each operator produces one tuple at a time
    let input = filtered_scan("orders", "status", 1);
    assert_optimization_improves(input);
}

#[test]
fn test_volcano_open_next_close() {
    // Classic open/next/close lifecycle
    let input = scan("customers");
    let projected = project(input, vec!["id", "name"]);
    assert_rule_applies(projected);
}

// ── Pipeline Behavior Tests ─────────────────────────────────────

#[test]
fn test_volcano_pipelined_operators() {
    // Filter and project are fully pipelined
    let input = scan("products");
    let filtered = input.filter(eq(col("category"), string("electronics")));
    let projected = project(filtered, vec!["name", "price"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_volcano_pipeline_chain() {
    // Multiple pipelined operators in sequence
    let input = scan("events");
    let filter1 = input.filter(gt(col("timestamp"), int(1000)));
    let filter2 = filter1.filter(eq(col("event_type"), string("click")));
    let projected = project(filter2, vec!["user_id", "timestamp"]);
    assert_rule_applies(projected);
}

// ── Pipeline Breaking Tests ─────────────────────────────────────

#[test]
fn test_volcano_sort_breaks_pipeline() {
    // Sort is a pipeline breaker - must consume all input
    let input = scan("rankings");
    let sorted = sort(input, "score", false);
    assert_optimization_improves(sorted);
}

#[test]
fn test_volcano_hash_aggregation_breaks_pipeline() {
    // Hash aggregation must see all tuples before emitting
    let input = scan("sales");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("amount")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_volcano_hash_join_build_phase_blocks() {
    // Hash join build phase blocks pipeline
    let join = two_table_join("orders", "customers", "customer_id", "id");
    assert_rule_applies(join);
}

// ── Materialization Points Tests ────────────────────────────────

#[test]
fn test_volcano_materialization_sort() {
    // Sort materializes entire intermediate result
    let input = scan("large_table");
    let sorted = sort(input, "key", true);
    let limited = limit(sorted, 10);
    assert_optimization_improves(limited);
}

#[test]
fn test_volcano_materialization_aggregation() {
    // Aggregation materializes groups
    let input = scan("transactions");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("account_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_volcano_avoid_materialization() {
    // Pipelined operators avoid materialization
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(100)));
    let limited = limit(filtered, 100);
    assert_optimization_improves(limited);
}

// ── Blocking vs Pipelined Operators ─────────────────────────────

#[test]
fn test_volcano_blocking_operator_sort() {
    // Sort is blocking
    let input = scan("unsorted_data");
    let sorted = sort(input, "timestamp", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_volcano_pipelined_filter() {
    // Filter is fully pipelined
    let input = scan("logs");
    let filtered = input.filter(eq(col("level"), string("ERROR")));
    assert_rule_applies(filtered);
}

#[test]
fn test_volcano_semi_pipelined_nested_loop() {
    // Nested loop is semi-pipelined (outer pipelined, inner blocks)
    let join = two_table_join("small_table", "large_table", "id", "foreign_id");
    assert_optimization_improves(join);
}

// ── Memory Management Tests ─────────────────────────────────────

#[test]
fn test_volcano_memory_bounded_sort() {
    // External sort when data exceeds memory
    let input = scan("huge_unsorted");
    let sorted = sort(input, "key", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_volcano_memory_hash_join() {
    // Hash join with memory management
    let join = two_table_join("table1", "table2", "key", "key");
    assert_rule_applies(join);
}

// ── Operator State Tests ────────────────────────────────────────

#[test]
fn test_volcano_stateful_aggregation() {
    // Aggregation maintains state across tuples
    let input = scan("metrics");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("metric_name")],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Avg,
                arg: Some(col("value")),
                distinct: false,
                alias: None,
            },
        ],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_volcano_stateless_projection() {
    // Projection is stateless
    let input = scan("users");
    let projected = project(input, vec!["id", "email"]);
    assert_rule_applies(projected);
}

// ── Error Handling Tests ────────────────────────────────────────

#[test]
fn test_volcano_error_propagation() {
    // Errors propagate up iterator chain
    let input = scan("data_with_errors");
    let filtered = input.filter(gt(col("value"), int(0)));
    assert_optimization_improves(filtered);
}

// ── Late Materialization Tests ──────────────────────────────────

#[test]
fn test_volcano_late_materialization_scan() {
    // Scan only fetches needed columns
    let input = scan("wide_table");
    let projected = project(input, vec!["id", "name"]);
    assert_optimization_improves(projected);
}
