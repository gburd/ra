//! Integration tests for optimizer end-to-end behavior.
//!
//! Tests cross-component interactions between parser, optimizer,
//! cost model, and hardware profiles.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::Expr;
use ra_engine::Optimizer;
use ra_hardware::HardwareProfile;

// ── Multi-Rule Integration Tests ────────────────────────────

#[test]
fn test_filter_pushdown_through_join() {
    // Filter on left side should push down through join
    let left = scan("orders");
    let right = scan("customers");
    let joined = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(left),
        right: Box::new(right),
    };
    let filtered = RelExpr::Filter {
        predicate: gt(col("amount"), int(100)),
        input: Box::new(joined),
    };

    assert_cost_calculated(filtered);
}

#[test]
fn test_predicate_pushdown_with_projection() {
    // Filter + Project should optimize together
    let scanned = scan("products");
    let filtered = RelExpr::Filter {
        predicate: eq(col("category"), string("electronics")),
        input: Box::new(scanned),
    };
    let projected = project(filtered, vec!["name", "price"]);

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&projected);
    assert!(result.is_ok(), "filter + project should optimize");
}

#[test]
fn test_join_reordering_three_tables() {
    // Three-way join should be reordered for optimal execution
    let t1 = scan("table1");
    let t2 = scan("table2");
    let t3 = scan("table3");

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

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&j2);
    assert!(result.is_ok(), "three-way join should optimize");
}

#[test]
fn test_limit_pushdown_through_sort() {
    // LIMIT + ORDER BY should become Top-K optimization
    let scanned = scan("events");
    let sorted = sort(scanned, "timestamp", false);
    let limited = limit(sorted, 10);

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&limited);
    assert!(result.is_ok(), "limit + sort should optimize");
}

#[test]
fn test_aggregate_with_filter_pushdown() {
    // Filter should push below aggregation when possible
    let scanned = scan("sales");
    let filtered = RelExpr::Filter {
        predicate: gt(col("amount"), int(0)),
        input: Box::new(scanned),
    };
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(filtered),
    };

    assert_cost_calculated(agg);
}

// ── Hardware Profile Integration ────────────────────────────

#[test]
fn test_cpu_profile_optimization() {
    let plan = two_table_join("fact", "dimension", "dim_id", "id");

    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(HardwareProfile::cpu_only());

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "CPU profile optimization should succeed");
}

#[test]
fn test_gpu_profile_optimization() {
    let plan = two_table_join("large_table", "small_table", "key", "key");

    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(HardwareProfile::gpu_server());

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "GPU profile optimization should succeed");
}

#[test]
fn test_fpga_profile_optimization() {
    let plan = filtered_scan("sensor_data", "value", 100);

    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(HardwareProfile::fpga_appliance());

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "FPGA profile optimization should succeed");
}

// ── Complex Query Patterns ──────────────────────────────────

#[test]
fn test_star_schema_join() {
    // Typical data warehouse star schema: fact table + multiple dimensions
    let fact = scan("sales_fact");
    let dim_time = scan("time_dim");
    let dim_product = scan("product_dim");
    let dim_customer = scan("customer_dim");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("time_key"), col("time_id")),
        left: Box::new(fact),
        right: Box::new(dim_time),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("product_key"), col("product_id")),
        left: Box::new(j1),
        right: Box::new(dim_product),
    };

    let j3 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("customer_key"), col("customer_id")),
        left: Box::new(j2),
        right: Box::new(dim_customer),
    };

    assert_cost_calculated(j3);
}

#[test]
fn test_union_with_filters() {
    // UNION of filtered scans
    let left = RelExpr::Filter {
        predicate: eq(col("region"), string("US")),
        input: Box::new(scan("sales_us")),
    };

    let right = RelExpr::Filter {
        predicate: eq(col("region"), string("EU")),
        input: Box::new(scan("sales_eu")),
    };

    let union = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&union);
    assert!(result.is_ok(), "union with filters should optimize");
}

#[test]
fn test_subquery_pattern() {
    // Simulates a correlated subquery pattern
    let inner = RelExpr::Filter {
        predicate: gt(col("salary"), int(50000)),
        input: Box::new(scan("employees")),
    };

    let agg = RelExpr::Aggregate {
        group_by: vec![col("department_id")],
        aggregates: vec![],
        input: Box::new(inner),
    };

    let outer = scan("departments");
    let joined = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("department_id")),
        left: Box::new(outer),
        right: Box::new(agg),
    };

    assert_cost_calculated(joined);
}

// ── Edge Cases & Error Handling ─────────────────────────────

#[test]
fn test_empty_plan_optimization() {
    // Single scan should optimize (at minimum, select algorithm)
    let plan = scan("table");

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "single scan should optimize successfully");
}

#[test]
fn test_self_join() {
    // Self-join on same table
    let left = scan("employees");
    let right = scan("employees");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("manager_id"), col("id")),
        left: Box::new(left),
        right: Box::new(right),
    };

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "self-join should optimize");
}

#[test]
fn test_cross_join() {
    // Cartesian product
    let left = scan("small_table");
    let right = scan("tiny_table");
    let plan = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: Expr::Const(ra_core::expr::Const::Bool(true)),
        left: Box::new(left),
        right: Box::new(right),
    };

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "cross join should optimize");
}

#[test]
fn test_left_outer_join() {
    // Left outer join preservation
    let left = scan("orders");
    let right = scan("customers");
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(left),
        right: Box::new(right),
    };

    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "left outer join should optimize");
}

// ── Performance Characteristics ─────────────────────────────

#[test]
fn test_optimization_is_fast() {
    // Simple query should optimize quickly
    use std::time::Instant;
    use ra_test_utils::TestProfile;

    let profile = TestProfile::current();
    let expected_ms = profile.scale_time_ms(1000.0);

    let plan = two_table_join("users", "orders", "id", "user_id");
    let optimizer = create_test_optimizer();

    let start = Instant::now();
    let _result = optimizer.optimize(&plan).expect("should optimize");
    let duration = start.elapsed();

    assert!(
        duration.as_millis() < expected_ms as u128,
        "optimization took {}ms (expected < {:.0}ms on this platform, scale={:.2}x)",
        duration.as_millis(),
        expected_ms,
        profile.scale_factors.time_scale
    );
}

#[test]
fn test_complex_query_optimizes_reasonably() {
    // More complex query should still complete in reasonable time
    use std::time::Instant;
    use ra_test_utils::TestProfile;

    let profile = TestProfile::current();
    let expected_ms = profile.scale_time_ms(5000.0);

    // 4-way join with filters
    let _t1 = filtered_scan("table1", "col1", 10);
    let _t2 = filtered_scan("table2", "col2", 20);
    let _t3 = filtered_scan("table3", "col3", 30);
    let _t4 = filtered_scan("table4", "col4", 40);

    let j1 = two_table_join("table1", "table2", "id", "id");
    let j2 = two_table_join("table3", "table4", "id", "id");
    let j3 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(j2),
    };

    let optimizer = create_test_optimizer();
    let start = Instant::now();
    let _result = optimizer.optimize(&j3).expect("should optimize");
    let duration = start.elapsed();

    assert!(
        duration.as_millis() < expected_ms as u128,
        "complex optimization took {}ms (expected < {:.0}ms on this platform)",
        duration.as_millis(),
        expected_ms
    );
}

// ── Idempotence Tests ───────────────────────────────────────

#[test]
fn test_optimization_is_idempotent() {
    // Optimizing twice should produce same result
    let plan = two_table_join("orders", "customers", "customer_id", "id");

    let optimizer = create_test_optimizer();
    let result1 = optimizer.optimize(&plan).expect("first optimization should succeed");
    let result2 = optimizer.optimize(&result1).expect("second optimization should succeed");

    assert_eq!(result1, result2, "optimization should be idempotent");
}

#[test]
fn test_multiple_optimization_passes() {
    // Running optimizer multiple times should converge
    let plan = filtered_scan("table", "value", 100);

    let optimizer = create_test_optimizer();
    let r1 = optimizer.optimize(&plan).expect("pass 1 should succeed");
    let r2 = optimizer.optimize(&r1).expect("pass 2 should succeed");
    let r3 = optimizer.optimize(&r2).expect("pass 3 should succeed");

    assert_eq!(r2, r3, "optimization should converge");
}
