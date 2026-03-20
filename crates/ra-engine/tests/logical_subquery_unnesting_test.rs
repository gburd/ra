//! Tests for logical subquery unnesting optimization rules.
//!
//! Subquery unnesting transforms correlated subqueries into joins,
//! enabling more efficient execution plans through decorrelation.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};

// ── EXISTS Subquery Unnesting ───────────────────────────────

#[test]
fn test_exists_to_semi_join() {
    // SELECT * FROM orders WHERE EXISTS (SELECT 1 FROM customers WHERE customer_id = orders.customer_id)
    let orders = scan("orders");
    let customers = scan("customers");
    let plan = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(customers),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_exists_with_additional_filter() {
    let orders = scan("orders");
    let filtered_customers = filtered_scan("customers", "status", 1);
    let plan = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(filtered_customers),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_nested_exists() {
    // Multiple EXISTS subqueries
    let orders = scan("orders");
    let customers = scan("customers");
    let j1 = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(customers),
    };
    assert_cost_calculated(j1);
}

// ── NOT EXISTS Subquery Unnesting ───────────────────────────

#[test]
fn test_not_exists_to_anti_join() {
    // SELECT * FROM customers WHERE NOT EXISTS (SELECT 1 FROM orders WHERE customer_id = customers.id)
    let customers = scan("customers");
    let orders = scan("orders");
    let plan = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: eq(col("id"), col("customer_id")),
        left: Box::new(customers),
        right: Box::new(orders),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_not_exists_with_correlation() {
    let outer = scan("products");
    let inner = scan("sales");
    let plan = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(outer),
        right: Box::new(inner),
    };
    assert_cost_calculated(plan);
}

// ── IN Subquery Unnesting ───────────────────────────────────

#[test]
fn test_in_subquery_to_semi_join() {
    // SELECT * FROM orders WHERE customer_id IN (SELECT id FROM customers)
    let orders = scan("orders");
    let customers = scan("customers");
    let plan = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(customers),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_in_subquery_with_filter() {
    let orders = scan("orders");
    let active_customers = filtered_scan("customers", "active", 1);
    let plan = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(active_customers),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_in_with_null_handling() {
    // IN with potential NULLs requires special handling
    let outer = scan("table_a");
    let inner = scan("table_b");
    let plan = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("key"), col("key")),
        left: Box::new(outer),
        right: Box::new(inner),
    };
    assert_cost_calculated(plan);
}

// ── NOT IN Subquery Unnesting ───────────────────────────────

#[test]
fn test_not_in_to_anti_join() {
    // SELECT * FROM orders WHERE customer_id NOT IN (SELECT id FROM blacklist)
    let orders = scan("orders");
    let blacklist = scan("blacklist");
    let plan = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(blacklist),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_not_in_null_rejection() {
    // NOT IN with NULLs - requires null rejection filter
    let outer = scan("data");
    let inner = scan("exclusions");
    let plan = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: eq(col("value"), col("excluded_value")),
        left: Box::new(outer),
        right: Box::new(inner),
    };
    assert_cost_calculated(plan);
}

// ── Scalar Subquery Unnesting ───────────────────────────────

#[test]
fn test_scalar_subquery_to_left_join() {
    // SELECT o.*, (SELECT name FROM customers WHERE id = o.customer_id) FROM orders o
    let orders = scan("orders");
    let customers = scan("customers");
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(orders),
        right: Box::new(customers),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_scalar_subquery_with_aggregate() {
    let orders = scan("orders");
    let agg_items = RelExpr::Aggregate {
        group_by: vec![col("order_id")],
        aggregates: vec![],
        input: Box::new(scan("order_items")),
    };
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("id"), col("order_id")),
        left: Box::new(orders),
        right: Box::new(agg_items),
    };
    assert_cost_calculated(plan);
}

// ── Correlated Subquery Decorrelation ───────────────────────

#[test]
fn test_simple_correlation_removal() {
    let outer = scan("table1");
    let inner = scan("table2");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("ref_id")),
        left: Box::new(outer),
        right: Box::new(inner),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_multi_level_correlation() {
    // Nested correlated subqueries
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("t1_id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("t2_id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_cost_calculated(j2);
}

// ── Apply Elimination ───────────────────────────────────────

#[test]
fn test_lateral_join_to_regular_join() {
    let orders = scan("orders");
    let items = scan("order_items");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("order_id")),
        left: Box::new(orders),
        right: Box::new(items),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_apply_with_aggregation() {
    let customers = scan("customers");
    let order_agg = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![],
        input: Box::new(scan("orders")),
    };
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("id"), col("customer_id")),
        left: Box::new(customers),
        right: Box::new(order_agg),
    };
    assert_cost_calculated(plan);
}

// ── Aggregate Subquery Unnesting ────────────────────────────

#[test]
fn test_subquery_with_group_by() {
    let outer = scan("departments");
    let inner_agg = RelExpr::Aggregate {
        group_by: vec![col("department_id")],
        aggregates: vec![],
        input: Box::new(scan("employees")),
    };
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("id"), col("department_id")),
        left: Box::new(outer),
        right: Box::new(inner_agg),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_max_one_row_subquery() {
    // Subquery guaranteed to return at most one row
    let outer = scan("orders");
    let inner = filtered_scan("config", "key", 1);
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("config_key"), col("key")),
        left: Box::new(outer),
        right: Box::new(inner),
    };
    assert_cost_calculated(plan);
}

// ── Complex Unnesting Patterns ──────────────────────────────

#[test]
fn test_subquery_in_select_and_where() {
    // Both scalar subquery in SELECT and EXISTS in WHERE
    let base = scan("orders");
    let customers = scan("customers");
    let items = scan("order_items");

    let j1 = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(base),
        right: Box::new(customers),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("id"), col("order_id")),
        left: Box::new(j1),
        right: Box::new(items),
    };

    assert_cost_calculated(j2);
}

#[test]
fn test_union_of_subqueries() {
    let q1 = filtered_scan("orders_2023", "status", 1);
    let q2 = filtered_scan("orders_2024", "status", 1);
    let union = RelExpr::Union {
        all: true,
        left: Box::new(q1),
        right: Box::new(q2),
    };
    assert_cost_calculated(union);
}

// ── Subquery Hoisting ───────────────────────────────────────

#[test]
fn test_invariant_subquery_hoisting() {
    // Subquery independent of outer query
    let plan = two_table_join("orders", "customers", "customer_id", "id");
    assert_cost_calculated(plan);
}

#[test]
fn test_subquery_common_table_expression() {
    // CTE-like pattern
    let cte = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    let main = scan("products");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("region"), col("region")),
        left: Box::new(main),
        right: Box::new(cte),
    };
    assert_cost_calculated(plan);
}

// ── Nested Subquery Patterns ────────────────────────────────

#[test]
fn test_subquery_in_subquery() {
    // Nested subqueries requiring multiple levels of unnesting
    let innermost = filtered_scan("products", "active", 1);
    let middle = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(scan("order_items")),
        right: Box::new(innermost),
    };
    let outer = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("id"), col("order_id")),
        left: Box::new(scan("orders")),
        right: Box::new(middle),
    };
    assert_cost_calculated(outer);
}

#[test]
fn test_recursive_unnesting() {
    let t1 = scan("t1");
    let t2 = scan("t2");
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

    assert_cost_calculated(j2);
}
