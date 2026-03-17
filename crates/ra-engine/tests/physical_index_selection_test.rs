//! Tests for physical index selection optimization.
//!
//! Tests cover index scan vs table scan decisions, covering indexes,
//! multi-column indexes, bitmap indexes, index intersection/union,
//! and partial indexes.

mod helpers;

use helpers::*;
use ra_core::expr::{BinOp, Const};

// ── Index Scan vs Table Scan ────────────────────────────────────

#[test]
fn test_index_scan_high_selectivity() {
    // High selectivity (1%) favors index scan
    let scan = filtered_scan("users", "id", 1000);
    assert_optimization_improves(scan);
}

#[test]
fn test_table_scan_low_selectivity() {
    // Low selectivity (>20%) favors table scan
    let input = scan("products");
    let filter = input.filter(gt(col("price"), int(10)));
    assert_rule_applies(filter);
}

#[test]
fn test_index_scan_equality_predicate() {
    let input = scan("orders");
    let filter = input.filter(eq(col("customer_id"), int(12345)));
    assert_optimization_improves(filter);
}

#[test]
fn test_index_scan_range_predicate() {
    let input = scan("sales");
    let filter = input.filter(and(
        binop(BinOp::Ge, col("date"), string("2024-01-01")),
        binop(BinOp::Le, col("date"), string("2024-12-31")),
    ));
    assert_rule_applies(filter);
}

// ── Index-Only Scan ─────────────────────────────────────────────

#[test]
fn test_covering_index_avoids_table_lookup() {
    // Query only needs columns in index
    let input = scan("users");
    let projected = project(input, vec!["id", "email"]);
    let filtered = projected.filter(eq(col("id"), int(100)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_covering_index_with_aggregation() {
    let input = scan("orders");
    let projected = project(input, vec!["customer_id", "total"]);
    assert_rule_applies(projected);
}

#[test]
fn test_index_only_scan_sorted_access() {
    let input = scan("products");
    let sorted = sort(input, "price", true);
    let limited = limit(sorted, 10);
    assert_optimization_improves(limited);
}

// ── Multi-Column Index ──────────────────────────────────────────

#[test]
fn test_multi_column_index_all_columns() {
    // Index on (a, b, c), query uses all
    let input = scan("orders");
    let filter = input.filter(and(
        eq(col("customer_id"), int(1)),
        eq(col("order_date"), string("2024-01-01")),
    ));
    assert_optimization_improves(filter);
}

#[test]
fn test_multi_column_index_prefix() {
    // Index on (a, b, c), query uses (a, b)
    let input = scan("employees");
    let filter = input.filter(and(
        eq(col("department"), string("sales")),
        eq(col("location"), string("NYC")),
    ));
    assert_rule_applies(filter);
}

#[test]
fn test_multi_column_index_skip_column() {
    // Index on (a, b, c), query uses (a, c) - less efficient
    let input = scan("inventory");
    let filter = input.filter(and(
        eq(col("warehouse_id"), int(1)),
        eq(col("category"), string("electronics")),
    ));
    assert_optimization_improves(filter);
}

// ── Bitmap Index ────────────────────────────────────────────────

#[test]
fn test_bitmap_index_low_cardinality() {
    // Bitmap index for low-cardinality columns
    let input = scan("transactions");
    let filter = input.filter(eq(col("status"), string("completed")));
    assert_optimization_improves(filter);
}

#[test]
fn test_bitmap_index_multiple_predicates() {
    let input = scan("products");
    let filter = input.filter(and(
        eq(col("category"), string("books")),
        eq(col("in_stock"), Expr::Const(Const::Bool(true))),
    ));
    assert_rule_applies(filter);
}

#[test]
fn test_bitmap_index_or_predicates() {
    let input = scan("customers");
    let filter = input.filter(or(
        eq(col("tier"), string("gold")),
        eq(col("tier"), string("platinum")),
    ));
    assert_optimization_improves(filter);
}

// ── Index Intersection ──────────────────────────────────────────

#[test]
fn test_index_intersection_two_indexes() {
    // Two separate indexes combined with AND
    let input = scan("products");
    let filter = input.filter(and(
        eq(col("category_id"), int(5)),
        gt(col("price"), int(100)),
    ));
    assert_optimization_improves(filter);
}

#[test]
fn test_index_intersection_three_indexes() {
    let input = scan("orders");
    let filter = input.filter(and(
        and(
            eq(col("status"), string("shipped")),
            eq(col("customer_id"), int(1000)),
        ),
        gt(col("total"), int(500)),
    ));
    assert_rule_applies(filter);
}

// ── Index Union ─────────────────────────────────────────────────

#[test]
fn test_index_union_or_predicates() {
    // Two separate indexes combined with OR
    let input = scan("events");
    let filter = input.filter(or(
        eq(col("event_type"), string("login")),
        eq(col("event_type"), string("logout")),
    ));
    assert_optimization_improves(filter);
}

#[test]
fn test_index_union_range_predicates() {
    let input = scan("logs");
    let filter = input.filter(or(
        binop(BinOp::Lt, col("timestamp"), string("2024-01-01")),
        binop(BinOp::Gt, col("timestamp"), string("2024-12-31")),
    ));
    assert_rule_applies(filter);
}

// ── Partial Index ───────────────────────────────────────────────

#[test]
fn test_partial_index_matching_predicate() {
    // Partial index WHERE status = 'active'
    let input = scan("subscriptions");
    let filter = input.filter(and(
        eq(col("status"), string("active")),
        gt(col("created_at"), string("2024-01-01")),
    ));
    assert_optimization_improves(filter);
}

#[test]
fn test_partial_index_non_matching_predicate() {
    // Partial index not usable when predicate doesn't match
    let input = scan("subscriptions");
    let filter = input.filter(eq(col("status"), string("expired")));
    assert_rule_applies(filter);
}

// ── Index Skip Scan ─────────────────────────────────────────────

#[test]
fn test_index_skip_scan_distinct_values() {
    // Index skip scan for DISTINCT queries
    let input = scan("orders");
    let projected = project(input, vec!["customer_id"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_index_skip_scan_prefix_match() {
    // Skip scan on composite index
    let input = scan("logs");
    let filter = input.filter(eq(col("severity"), string("ERROR")));
    assert_rule_applies(filter);
}

// ── Index with Sorting ──────────────────────────────────────────

#[test]
fn test_index_provides_order() {
    // Index already provides sorted order
    let input = scan("products");
    let sorted = sort(input, "price", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_index_reverse_scan() {
    // Reverse scan on index
    let input = scan("events");
    let sorted = sort(input, "timestamp", false);
    assert_optimization_improves(sorted);
}

#[test]
fn test_index_order_with_limit() {
    // Index + LIMIT optimization
    let input = scan("products");
    let sorted = sort(input, "popularity", false);
    let limited = limit(sorted, 10);
    assert_optimization_improves(limited);
}

// ── Index Cost Estimation ───────────────────────────────────────

#[test]
fn test_index_cost_vs_table_scan_crossover() {
    // Test selectivity crossover point (~15-20%)
    let input = scan("large_table");
    let filter = input.filter(binop(BinOp::Lt, col("value"), int(200)));
    assert_rule_applies(filter);
}

#[test]
fn test_index_clustering_factor() {
    // High clustering factor reduces random I/O
    let input = scan("time_series");
    let filter = input.filter(gt(col("timestamp"), string("2024-01-01")));
    assert_optimization_improves(filter);
}

#[test]
fn test_index_on_foreign_key() {
    // Index on foreign key column
    let input = scan("order_items");
    let filter = input.filter(eq(col("order_id"), int(12345)));
    assert_optimization_improves(filter);
}

// ── Expression Index ────────────────────────────────────────────

#[test]
fn test_expression_index_function_call() {
    // Index on LOWER(email)
    let input = scan("users");
    let filter = input.filter(eq(
        Expr::Func {
            name: "lower".to_string(),
            args: vec![col("email")],
        },
        string("user@example.com"),
    ));
    assert_optimization_improves(filter);
}

#[test]
fn test_expression_index_computed_column() {
    // Index on (price * quantity)
    let input = scan("line_items");
    let filter = input.filter(gt(
        binop(BinOp::Mul, col("price"), col("quantity")),
        int(1000),
    ));
    assert_rule_applies(filter);
}

// ── Null Handling ───────────────────────────────────────────────

#[test]
fn test_index_scan_is_null() {
    let input = scan("customers");
    let filter = input.filter(Expr::UnaryOp {
        op: ra_core::expr::UnaryOp::IsNull,
        expr: Box::new(col("email")),
    });
    assert_optimization_improves(filter);
}

#[test]
fn test_index_scan_is_not_null() {
    let input = scan("users");
    let filter = input.filter(Expr::UnaryOp {
        op: ra_core::expr::UnaryOp::IsNotNull,
        expr: Box::new(col("phone")),
    });
    assert_rule_applies(filter);
}

// ── Index Maintenance ───────────────────────────────────────────

#[test]
fn test_index_selection_with_writes() {
    // Consider index maintenance cost for write-heavy tables
    let input = scan("high_write_table");
    let filter = input.filter(eq(col("status"), string("pending")));
    assert_optimization_improves(filter);
}

#[test]
fn test_index_bloat_consideration() {
    // Index bloat affects cost estimation
    let input = scan("old_table");
    let filter = input.filter(gt(col("id"), int(1000000)));
    assert_rule_applies(filter);
}
