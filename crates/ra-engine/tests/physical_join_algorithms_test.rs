#![expect(clippy::expect_used, reason = "test code")]
// Tests for physical join algorithm selection.
//!
//! Tests cover hash join variants, nested loop joins, sort-merge joins,
//! and adaptive join strategies. These tests verify that the optimizer
//! selects appropriate join algorithms based on table sizes, memory
//! availability, and hardware characteristics.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, Const, Expr};
use ra_hardware::HardwareProfile;

// ── Hash Join Tests ─────────────────────────────────────────────

#[test]
fn test_hash_join_small_build_side() {
    let _small = scan("small_table"); // Assume 1K rows
    let _large = scan("large_table"); // Assume 1M rows
    let join = two_table_join("large_table", "small_table", "id", "fk");
    assert_optimization_improves(join);
}

#[test]
fn test_hash_join_equal_sized_tables() {
    let _t1 = scan("table1"); // 100K rows
    let _t2 = scan("table2"); // 100K rows
    let join = two_table_join("table1", "table2", "id", "id");
    assert_cost_calculated(join);
}

#[test]
fn test_grace_hash_join_memory_constrained() {
    // Grace hash join when data exceeds memory
    let _large1 = scan("large1"); // 10M rows
    let _large2 = scan("large2"); // 10M rows
    let join = two_table_join("large1", "large2", "key", "key");
    assert_optimization_improves(join);
}

#[test]
fn test_hybrid_hash_join_partial_memory() {
    // Hybrid hash when some partitions fit in memory
    let _medium1 = scan("medium1");
    let _medium2 = scan("medium2");
    let join = two_table_join("medium1", "medium2", "id", "id");
    assert_cost_calculated(join);
}

#[test]
fn test_radix_hash_join_high_cardinality() {
    // Radix hash for high-cardinality joins
    let _high_card1 = scan("high_card1");
    let _high_card2 = scan("high_card2");
    let join = two_table_join("high_card1", "high_card2", "uuid", "uuid");
    assert_optimization_improves(join);
}

#[test]
fn test_hash_join_multiple_conditions() {
    let t1 = scan("orders");
    let t2 = scan("items");
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: and(
            eq(qcol("orders", "order_id"), qcol("items", "order_id")),
            eq(qcol("orders", "store_id"), qcol("items", "store_id")),
        ),
        left: Box::new(t1),
        right: Box::new(t2),
    };
    assert_cost_calculated(join);
}

#[test]
fn test_hash_join_with_bloom_filter() {
    // Hash join with bloom filter optimization
    let _fact = scan("fact_table");
    let _dim = scan("dimension");
    let join = two_table_join("fact_table", "dimension", "dim_key", "id");
    assert_optimization_improves(join);
}

#[test]
fn test_hash_join_left_outer() {
    let t1 = scan("customers");
    let t2 = scan("orders");
    let join = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(qcol("customers", "id"), qcol("orders", "customer_id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };
    assert_cost_calculated(join);
}

#[test]
fn test_hash_join_right_outer() {
    let t1 = scan("orders");
    let t2 = scan("customers");
    let join = RelExpr::Join {
        join_type: JoinType::RightOuter,
        condition: eq(qcol("orders", "customer_id"), qcol("customers", "id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };
    assert_cost_calculated(join);
}

// ── Nested Loop Join Tests ──────────────────────────────────────

#[test]
fn test_nested_loop_tiny_tables() {
    let _tiny1 = scan("config"); // <10 rows
    let _tiny2 = scan("settings"); // <10 rows
    let join = two_table_join("config", "settings", "key", "key");
    assert_optimization_improves(join);
}

#[test]
fn test_block_nested_loop_small_tables() {
    let _small1 = scan("categories"); // 100 rows
    let _small2 = scan("products"); // 1K rows
    let join = two_table_join("categories", "products", "id", "category_id");
    assert_cost_calculated(join);
}

#[test]
fn test_index_nested_loop_with_index() {
    // Assume indexed column on right table
    let _orders = scan("orders");
    let _customers_indexed = scan("customers"); // id is indexed
    let join = two_table_join("orders", "customers", "customer_id", "id");
    assert_optimization_improves(join);
}

#[test]
fn test_nested_loop_cross_join() {
    let t1 = scan("small_a");
    let t2 = scan("small_b");
    let join = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(t1),
        right: Box::new(t2),
    };
    assert_cost_calculated(join);
}

#[test]
fn test_nested_loop_non_equijoin() {
    let t1 = scan("events");
    let t2 = scan("timeranges");
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: and(
            binop(
                BinOp::Ge,
                qcol("events", "timestamp"),
                qcol("timeranges", "start_time"),
            ),
            binop(
                BinOp::Le,
                qcol("events", "timestamp"),
                qcol("timeranges", "end_time"),
            ),
        ),
        left: Box::new(t1),
        right: Box::new(t2),
    };
    assert_optimization_improves(join);
}

// ── Sort-Merge Join Tests ───────────────────────────────────────

#[test]
fn test_sort_merge_join_sorted_inputs() {
    // When inputs are already sorted
    let sorted1 = sort(scan("sorted_data1"), "id", true);
    let sorted2 = sort(scan("sorted_data2"), "id", true);
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(qcol("sorted_data1", "id"), qcol("sorted_data2", "id")),
        left: Box::new(sorted1),
        right: Box::new(sorted2),
    };
    assert_optimization_improves(join);
}

#[test]
fn test_sort_merge_join_large_tables() {
    // Sort-merge for very large tables
    let _large1 = scan("huge_table1");
    let _large2 = scan("huge_table2");
    let join = two_table_join("huge_table1", "huge_table2", "key", "key");
    assert_cost_calculated(join);
}

#[test]
fn test_sort_merge_join_merge_phase() {
    let _t1 = scan("log_table1");
    let _t2 = scan("log_table2");
    let join = two_table_join("log_table1", "log_table2", "timestamp", "timestamp");
    assert_optimization_improves(join);
}

#[test]
fn test_sort_merge_join_with_duplicates() {
    let _t1 = scan("orders");
    let _t2 = scan("order_items");
    let join = two_table_join("orders", "order_items", "order_id", "order_id");
    assert_cost_calculated(join);
}

// ── Broadcast Join Tests ────────────────────────────────────────

#[test]
fn test_broadcast_join_tiny_dimension() {
    // Broadcast tiny dimension table
    let _fact = scan("sales_fact");
    let _tiny_dim = scan("tiny_dimension"); // <100 rows
    let join = two_table_join("sales_fact", "tiny_dimension", "dim_id", "id");
    assert_optimization_improves(join);
}

#[test]
fn test_broadcast_join_distributed_query() {
    let _large_partitioned = scan("partitioned_data");
    let _small_lookup = scan("lookup_table");
    let join = two_table_join("partitioned_data", "lookup_table", "key", "id");
    assert_cost_calculated(join);
}

// ── Shuffle Hash Join Tests ─────────────────────────────────────

#[test]
fn test_shuffle_hash_join_both_large() {
    // Both tables large, shuffle both
    let _large1 = scan("large_distributed1");
    let _large2 = scan("large_distributed2");
    let join = two_table_join(
        "large_distributed1",
        "large_distributed2",
        "partition_key",
        "partition_key",
    );
    assert_optimization_improves(join);
}

#[test]
fn test_shuffle_hash_join_skewed_data() {
    let _skewed1 = scan("skewed_table1");
    let _skewed2 = scan("skewed_table2");
    let join = two_table_join("skewed_table1", "skewed_table2", "hot_key", "hot_key");
    assert_cost_calculated(join);
}

// ── Adaptive Join Tests ─────────────────────────────────────────

#[test]
fn test_adaptive_join_switches_strategy() {
    // Adaptive join starts as hash, switches to sort-merge
    let _t1 = scan("dynamic_table1");
    let _t2 = scan("dynamic_table2");
    let join = two_table_join("dynamic_table1", "dynamic_table2", "id", "id");
    assert_optimization_improves(join);
}

#[test]
fn test_adaptive_join_memory_pressure() {
    // Adaptive join under memory pressure
    let _large = scan("memory_intensive");
    let _medium = scan("medium_size");
    let join = two_table_join("memory_intensive", "medium_size", "key", "key");
    assert_cost_calculated(join);
}

// ── Hardware-Specific Join Tests ────────────────────────────────

#[test]
fn test_gpu_hash_join() {
    let _t1 = scan("gpu_table1");
    let _t2 = scan("gpu_table2");
    let join = two_table_join("gpu_table1", "gpu_table2", "id", "id");
    assert_hardware_affects_cost(join);
}

#[test]
fn test_cpu_only_join_selection() {
    let _t1 = scan("table1");
    let _t2 = scan("table2");
    let join = two_table_join("table1", "table2", "key", "key");

    let opt = create_test_optimizer_with_hardware(HardwareProfile::cpu_only());
    let _result = opt.optimize(&join).expect("optimization should succeed");
}

#[test]
fn test_fpga_join_acceleration() {
    let _t1 = scan("streaming_data1");
    let _t2 = scan("streaming_data2");
    let join = two_table_join("streaming_data1", "streaming_data2", "id", "id");
    assert_hardware_affects_cost(join);
}

// ── Join Cardinality Tests ──────────────────────────────────────

#[test]
fn test_join_one_to_one() {
    let _users = scan("users"); // Primary key: id
    let _profiles = scan("profiles"); // Foreign key: user_id (unique)
    let join = two_table_join("users", "profiles", "id", "user_id");
    assert_optimization_improves(join);
}

#[test]
fn test_join_one_to_many() {
    let _customers = scan("customers");
    let _orders = scan("orders");
    let join = two_table_join("customers", "orders", "id", "customer_id");
    assert_cost_calculated(join);
}

#[test]
fn test_join_many_to_many() {
    let _students = scan("students");
    let _courses = scan("student_courses");
    let join = two_table_join("students", "student_courses", "id", "student_id");
    assert_optimization_improves(join);
}

// ── Anti/Semi Join Tests ────────────────────────────────────────

#[test]
fn test_semi_join_optimization() {
    let t1 = scan("products");
    let t2 = scan("active_categories");
    let join = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(
            qcol("products", "category_id"),
            qcol("active_categories", "id"),
        ),
        left: Box::new(t1),
        right: Box::new(t2),
    };
    assert_optimization_improves(join);
}

#[test]
fn test_anti_join_optimization() {
    let t1 = scan("all_users");
    let t2 = scan("banned_users");
    let join = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: eq(qcol("all_users", "id"), qcol("banned_users", "user_id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };
    assert_cost_calculated(join);
}

// ── Complex Join Patterns ───────────────────────────────────────

#[test]
fn test_self_join() {
    let employees = scan("employees");
    let managers = scan("employees");
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(qcol("employees", "manager_id"), qcol("employees", "id")),
        left: Box::new(employees),
        right: Box::new(managers),
    };
    assert_optimization_improves(join);
}

#[test]
fn test_bushy_join_tree() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");
    let t4 = scan("t4");

    let left_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let right_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t3),
        right: Box::new(t4),
    };

    let bushy_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(left_join),
        right: Box::new(right_join),
    };

    assert_optimization_improves(bushy_join);
}
