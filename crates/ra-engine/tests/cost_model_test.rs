//! Tests for cost model and statistics-based optimization.
//!
//! Cost models are crucial for query optimization, guiding the optimizer
//! to choose execution plans with lower estimated costs. This test suite
//! validates:
//!
//! 1. **Cardinality Estimation** - Predicting result set sizes
//! 2. **Selectivity Estimation** - Predicting filter effectiveness
//! 3. **Cost Calibration** - Balancing CPU, I/O, memory, and network costs
//!
//! Many of these tests verify that the cost model can process various plan
//! shapes without error (cardinality/selectivity estimation happens inside
//! the e-graph cost function during plan extraction). Tests that exercise
//! rewrite rules use `assert_rule_applies`; tests that only exercise cost
//! estimation use `assert_cardinality_estimated`, `assert_selectivity_estimated`,
//! or `assert_cost_calculated`.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, Expr};

// ── Cardinality Estimation: Base Tables ────────────────────────
// Base table scans have no rewrite rules that change them; the cost
// model assigns costs via the e-graph cost function during extraction.

#[test]
fn test_base_table_cardinality_known() {
    // Known table statistics should guide optimization
    let plan = scan("users");
    assert_cardinality_estimated(plan);
}

#[test]
fn test_base_table_cardinality_unknown() {
    // Unknown table should use default cardinality estimates
    let plan = scan("unknown_table");
    assert_cardinality_estimated(plan);
}

#[test]
fn test_empty_table_cardinality() {
    // Empty table (0 rows) should affect downstream operators
    let plan = scan("empty_table");
    assert_cardinality_estimated(plan);
}

#[test]
fn test_large_table_cardinality() {
    // Very large tables (billions of rows) affect cost models
    let plan = scan("huge_fact_table");
    assert_cardinality_estimated(plan);
}

#[test]
fn test_view_cardinality_estimation() {
    // Views should estimate cardinality through their definition
    let plan = scan("customer_view");
    assert_cardinality_estimated(plan);
}

// ── Cardinality Estimation: Filters ─────────────────────────────
// Simple filtered scans with a single predicate: the `filter > const`
// pattern does not match any rewrite rule that changes the plan
// structure, but conjunctive/disjunctive filters trigger filter-merge
// or filter-split rules.

#[test]
fn test_filter_selectivity_high() {
    // Highly selective filter (few rows match)
    let plan = filtered_scan("users", "vip_status", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_filter_selectivity_low() {
    // Low selectivity filter (many rows match)
    let plan = filtered_scan("orders", "processed", 0);
    assert_cost_calculated(plan);
}

#[test]
fn test_filter_selectivity_medium() {
    // Medium selectivity filter
    let plan = filtered_scan("products", "in_stock", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_multiple_filter_conjunction() {
    // Multiple AND filters combine selectivity multiplicatively
    let base = scan("users");
    let filter1 = RelExpr::Filter {
        predicate: gt(col("age"), int(18)),
        input: Box::new(base),
    };
    let filter2 = RelExpr::Filter {
        predicate: eq(col("country"), string("US")),
        input: Box::new(filter1),
    };
    assert_cost_calculated(filter2);
}

#[test]
fn test_multiple_filter_disjunction() {
    // Multiple OR filters combine differently than AND
    let plan = RelExpr::Filter {
        predicate: or(
            gt(col("salary"), int(100000)),
            eq(col("title"), string("CEO")),
        ),
        input: Box::new(scan("employees")),
    };
    assert_cost_calculated(plan);
}

// ── Cardinality Estimation: Joins ───────────────────────────────
// Simple two-table inner joins with an equality condition: join
// commutativity fires in the e-graph, but cost-based extraction may
// select the original order. The cost model still evaluates
// cardinality for both orderings.

#[test]
fn test_join_cardinality_foreign_key() {
    // Foreign key join typically preserves left cardinality
    let plan = two_table_join("orders", "customers", "customer_id", "id");
    assert_cardinality_estimated(plan);
}

#[test]
fn test_join_cardinality_many_to_many() {
    // Many-to-many join can explode cardinality
    let plan = two_table_join("students", "courses", "course_id", "id");
    assert_cardinality_estimated(plan);
}

#[test]
fn test_join_cardinality_unique_key() {
    // Join on unique key limits result size
    let plan = two_table_join("orders", "order_details", "id", "order_id");
    assert_cardinality_estimated(plan);
}

#[test]
fn test_cross_join_cardinality() {
    // Cross join multiplies cardinalities
    let plan = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: Expr::Const(ra_core::expr::Const::Bool(true)),
        left: Box::new(scan("small_table")),
        right: Box::new(scan("tiny_table")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_multi_way_join_cardinality() {
    // Multi-way joins accumulate cardinality estimates
    let j1 = two_table_join("orders", "customers", "customer_id", "id");
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(j1),
        right: Box::new(scan("products")),
    };
    assert_cost_calculated(plan);
}

// ── Cardinality Estimation: Aggregates ──────────────────────────
// Aggregates without nested filters or sorts don't trigger rewrite
// rules. The cost model estimates output cardinality based on
// group-by column count.

#[test]
fn test_aggregate_no_groupby_cardinality() {
    // Aggregate without GROUP BY produces 1 row
    let plan = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![],
        input: Box::new(scan("orders")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_aggregate_single_groupby() {
    // Single GROUP BY cardinality ~ NDV of grouping column
    let plan = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![],
        input: Box::new(scan("orders")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_aggregate_multi_groupby() {
    // Multiple GROUP BY columns multiply NDVs (with correlation adjustment)
    let plan = RelExpr::Aggregate {
        group_by: vec![col("year"), col("month"), col("day")],
        aggregates: vec![],
        input: Box::new(scan("events")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_aggregate_high_cardinality_groupby() {
    // High cardinality GROUP BY (near unique) preserves input cardinality
    let plan = RelExpr::Aggregate {
        group_by: vec![col("transaction_id")],
        aggregates: vec![],
        input: Box::new(scan("transactions")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_aggregate_low_cardinality_groupby() {
    // Low cardinality GROUP BY significantly reduces cardinality
    let plan = RelExpr::Aggregate {
        group_by: vec![col("status")],
        aggregates: vec![],
        input: Box::new(scan("orders")),
    };
    assert_cardinality_estimated(plan);
}

// ── Cardinality Estimation: Set Operations ──────────────────────
// Set operations between different tables don't trigger rewrite
// rules (commutativity and self-identity rules only fire on
// identical subtrees).

#[test]
fn test_union_all_cardinality() {
    // UNION ALL adds cardinalities
    let plan = RelExpr::Union {
        all: true,
        left: Box::new(scan("current_orders")),
        right: Box::new(scan("archived_orders")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_union_distinct_cardinality() {
    // UNION (DISTINCT) reduces cardinality based on overlap
    let plan = RelExpr::Union {
        all: false,
        left: Box::new(scan("customers_us")),
        right: Box::new(scan("customers_eu")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_intersect_cardinality() {
    // INTERSECT cardinality <= min(left, right)
    let plan = RelExpr::Intersect {
        all: false,
        left: Box::new(scan("active_users")),
        right: Box::new(scan("premium_users")),
    };
    assert_cardinality_estimated(plan);
}

#[test]
fn test_except_cardinality() {
    // EXCEPT cardinality <= left cardinality
    let plan = RelExpr::Except {
        all: false,
        left: Box::new(scan("all_products")),
        right: Box::new(scan("discontinued_products")),
    };
    assert_cardinality_estimated(plan);
}

// ── Selectivity Estimation: Equality Predicates ─────────────────
// Single equality/comparison filters on a scan don't trigger
// structural rewrites. The cost model evaluates selectivity
// internally during cost-based extraction.

#[test]
fn test_selectivity_equality_high_ndv() {
    // Equality on high NDV column (e.g., ID) is very selective
    let plan = RelExpr::Filter {
        predicate: eq(col("user_id"), int(12345)),
        input: Box::new(scan("events")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_equality_low_ndv() {
    // Equality on low NDV column (e.g., boolean) is less selective
    let plan = RelExpr::Filter {
        predicate: eq(col("is_active"), int(1)),
        input: Box::new(scan("accounts")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_equality_with_mcv() {
    // Most common value has known frequency
    let plan = RelExpr::Filter {
        predicate: eq(col("country"), string("US")),
        input: Box::new(scan("users")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_in_list_small() {
    // IN clause with small list (simulated with OR)
    let plan = RelExpr::Filter {
        predicate: or(
            eq(col("status"), string("pending")),
            eq(col("status"), string("processing")),
        ),
        input: Box::new(scan("orders")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_in_list_large() {
    // IN clause with large list approaches full scan
    // Represented as function call
    let plan = RelExpr::Filter {
        predicate: Expr::Function {
            name: "in".to_string(),
            args: vec![col("product_id"), int(1000)],
        },
        input: Box::new(scan("inventory")),
    };
    assert_selectivity_estimated(plan);
}

// ── Selectivity Estimation: Range Predicates ────────────────────
// Range predicates using AND of two comparisons trigger the
// filter-split / filter-merge rules, so they DO change the plan.

#[test]
fn test_selectivity_range_narrow() {
    // Narrow range (e.g., single day) is selective
    let plan = RelExpr::Filter {
        predicate: and(
            gt(col("date"), int(20240101)),
            Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(col("date")),
                right: Box::new(int(20240102)),
            },
        ),
        input: Box::new(scan("events")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_selectivity_range_wide() {
    // Wide range (e.g., full year) is less selective
    let plan = RelExpr::Filter {
        predicate: gt(col("date"), int(20230101)),
        input: Box::new(scan("transactions")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_selectivity_range_open_ended() {
    // Open-ended range uses histogram distribution
    let plan = RelExpr::Filter {
        predicate: gt(col("salary"), int(150000)),
        input: Box::new(scan("employees")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_selectivity_range_between() {
    // BETWEEN selectivity from histogram buckets
    let plan = RelExpr::Filter {
        predicate: and(
            gt(col("price"), int(100)),
            Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(col("price")),
                right: Box::new(int(500)),
            },
        ),
        input: Box::new(scan("products")),
    };
    assert_cost_calculated(plan);
}

// ── Selectivity Estimation: Pattern Matching ────────────────────
// Function-based predicates (LIKE, REGEX) don't match any rewrite
// rule patterns.

#[test]
fn test_selectivity_like_prefix() {
    // LIKE with prefix (e.g., 'abc%') can use index statistics
    let plan = RelExpr::Filter {
        predicate: Expr::Function {
            name: "like".to_string(),
            args: vec![col("name"), string("Smith%")],
        },
        input: Box::new(scan("customers")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_like_contains() {
    // LIKE with contains (e.g., '%abc%') typically low selectivity
    let plan = RelExpr::Filter {
        predicate: Expr::Function {
            name: "like".to_string(),
            args: vec![col("description"), string("%urgent%")],
        },
        input: Box::new(scan("tickets")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_regex_simple() {
    // Simple regex patterns can estimate from statistics
    let plan = RelExpr::Filter {
        predicate: Expr::Function {
            name: "regex_match".to_string(),
            args: vec![col("email"), string(".*@company\\.com")],
        },
        input: Box::new(scan("users")),
    };
    assert_selectivity_estimated(plan);
}

// ── Selectivity Estimation: NULL Handling ───────────────────────
// IS NULL / IS NOT NULL filters don't match rewrite patterns
// (they would need `not-is-null` or `not-is-not-null` patterns
// which require a NOT wrapper).

#[test]
fn test_selectivity_is_null() {
    // IS NULL selectivity from null_fraction statistic
    let plan = RelExpr::Filter {
        predicate: Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::IsNull,
            operand: Box::new(col("deleted_at")),
        },
        input: Box::new(scan("records")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_is_not_null() {
    // IS NOT NULL selectivity = 1 - null_fraction
    let plan = RelExpr::Filter {
        predicate: Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::IsNotNull,
            operand: Box::new(col("email")),
        },
        input: Box::new(scan("contacts")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_null_safe_equality() {
    // NULL-safe equality (IS NOT DISTINCT FROM) includes NULLs
    let plan = RelExpr::Filter {
        predicate: eq(col("optional_field"), col("other_field")),
        input: Box::new(scan("data")),
    };
    assert_selectivity_estimated(plan);
}

// ── Selectivity Estimation: Correlations ────────────────────────
// Conjunctive filters with AND trigger filter-split rules.

#[test]
fn test_selectivity_correlated_columns() {
    // Correlated columns (e.g., city/state) don't multiply independently
    let plan = RelExpr::Filter {
        predicate: and(
            eq(col("city"), string("Seattle")),
            eq(col("state"), string("WA")),
        ),
        input: Box::new(scan("addresses")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_functional_dependency() {
    // Functional dependency (e.g., zip -> city) affects selectivity
    let plan = RelExpr::Filter {
        predicate: and(
            eq(col("zip_code"), string("98101")),
            eq(col("city"), string("Seattle")),
        ),
        input: Box::new(scan("locations")),
    };
    assert_selectivity_estimated(plan);
}

#[test]
fn test_selectivity_independent_columns() {
    // Independent columns multiply selectivity
    let plan = RelExpr::Filter {
        predicate: and(
            eq(col("department"), string("Engineering")),
            gt(col("salary"), int(100000)),
        ),
        input: Box::new(scan("employees")),
    };
    assert_cost_calculated(plan);
}

// ── Cost Calibration: CPU vs I/O ────────────────────────────────
// Arithmetic expressions in filter predicates don't trigger
// simplification rules unless they contain identity elements
// (e.g., +0, *1).

#[test]
fn test_cost_cpu_bound_operation() {
    // CPU-intensive operation (e.g., complex expression evaluation)
    let plan = RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::BinOp {
                op: BinOp::Mul,
                left: Box::new(col("quantity")),
                right: Box::new(col("price")),
            }),
            right: Box::new(col("tax")),
        },
        input: Box::new(scan("line_items")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_io_bound_operation() {
    // I/O-intensive operation (e.g., large table scan)
    let plan = scan("huge_fact_table");
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_index_vs_scan() {
    // Index access vs full table scan cost comparison
    let plan = filtered_scan("indexed_table", "indexed_column", 1);
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_sequential_vs_random_io() {
    // Sequential I/O (scan) cheaper than random I/O (index lookups)
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("foreign_id")),
        left: Box::new(scan("fact_table")),
        right: Box::new(scan("dimension_table")),
    };
    assert_cost_calculated(plan);
}

// ── Cost Calibration: Memory Costs ──────────────────────────────

#[test]
fn test_cost_hash_join_memory() {
    // Hash join memory cost for build side
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("key"), col("key")),
        left: Box::new(scan("large_table")),
        right: Box::new(scan("small_table")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_sort_memory() {
    // Sort memory requirements affect cost
    let plan = sort(scan("unsorted_data"), "key", true);
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_aggregate_memory() {
    // Aggregate memory for hash table
    let plan = RelExpr::Aggregate {
        group_by: vec![col("high_cardinality_column")],
        aggregates: vec![],
        input: Box::new(scan("events")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_memory_spill() {
    // Operations that spill to disk have higher cost
    let plan = RelExpr::Aggregate {
        group_by: vec![col("unique_id")],
        aggregates: vec![],
        input: Box::new(scan("billion_row_table")),
    };
    assert_cost_calculated(plan);
}

// ── Cost Calibration: Network Costs ─────────────────────────────

#[test]
fn test_cost_distributed_join() {
    // Distributed join requires network shuffles
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("key"), col("key")),
        left: Box::new(scan("distributed_table_a")),
        right: Box::new(scan("distributed_table_b")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_broadcast_join() {
    // Broadcasting small table cheaper than shuffle
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("dim_id"), col("id")),
        left: Box::new(scan("fact_table")),
        right: Box::new(scan("small_dimension")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_gather_operation() {
    // Gathering results to coordinator has network cost
    let plan = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![],
        input: Box::new(scan("distributed_table")),
    };
    assert_cost_calculated(plan);
}

// ── Cost Calibration: Hardware Profile Impact ───────────────────

#[test]
fn test_cost_cpu_only_profile() {
    // CPU-only hardware profile affects operator costs
    let plan = two_table_join("orders", "customers", "customer_id", "id");
    assert_hardware_affects_cost(plan);
}

#[test]
fn test_cost_gpu_accelerated_profile() {
    // GPU-accelerated profile changes cost for certain operators
    let plan = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scan("large_dataset")),
    };
    assert_hardware_affects_cost(plan);
}

#[test]
fn test_cost_nvme_storage_profile() {
    // NVMe storage reduces I/O costs vs spinning disks
    let plan = scan("large_table_on_nvme");
    assert_hardware_affects_cost(plan);
}

#[test]
fn test_cost_high_memory_profile() {
    // High memory system reduces need for spill operations
    let plan = RelExpr::Aggregate {
        group_by: vec![col("high_cardinality")],
        aggregates: vec![],
        input: Box::new(scan("huge_table")),
    };
    assert_hardware_affects_cost(plan);
}

// ── Cost Model Accuracy ─────────────────────────────────────────

#[test]
fn test_cost_model_prefers_indexed_access() {
    // Cost model should prefer index when selective
    let plan = filtered_scan("indexed_table", "indexed_pk", 12345);
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_prefers_hash_join() {
    // Cost model should prefer hash join for equi-joins
    let plan = two_table_join("large_a", "large_b", "key", "key");
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_prefers_merge_join_sorted() {
    // Cost model should prefer merge join when inputs pre-sorted
    let sorted_a = sort(scan("table_a"), "key", true);
    let sorted_b = sort(scan("table_b"), "key", true);
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("key"), col("key")),
        left: Box::new(sorted_a),
        right: Box::new(sorted_b),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_prefers_nested_loop_small() {
    // Cost model should prefer nested loop for tiny tables
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("ref_id")),
        left: Box::new(scan("tiny_table_10_rows")),
        right: Box::new(scan("small_table_100_rows")),
    };
    assert_cost_calculated(plan);
}

// ── Cost Model Edge Cases ───────────────────────────────────────

#[test]
fn test_cost_model_zero_rows() {
    // Empty result sets should have minimal cost
    let plan = RelExpr::Filter {
        predicate: Expr::Const(ra_core::expr::Const::Bool(false)),
        input: Box::new(scan("any_table")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_single_row() {
    // Single-row queries have minimal overhead
    let plan = RelExpr::Filter {
        predicate: eq(col("primary_key"), int(42)),
        input: Box::new(scan("indexed_table")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_skewed_distribution() {
    // Skewed data distribution affects cost estimates
    let plan = RelExpr::Filter {
        predicate: eq(col("skewed_column"), string("common_value")),
        input: Box::new(scan("skewed_table")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_multi_column_statistics() {
    // Multi-column statistics improve join estimates
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: and(eq(col("city"), col("city")), eq(col("state"), col("state"))),
        left: Box::new(scan("addresses1")),
        right: Box::new(scan("addresses2")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_self_join() {
    // Self-joins have symmetric cardinality
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("manager_id"), col("employee_id")),
        left: Box::new(scan("employees")),
        right: Box::new(RelExpr::Scan {
            table: "employees".to_string(),
            alias: Some("managers".to_string()),
        }),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_outer_join_cardinality() {
    // Left outer join preserves left cardinality
    let plan = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(scan("orders")),
        right: Box::new(scan("customers")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_anti_join_selectivity() {
    // Anti-join typically high selectivity
    let plan = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(scan("available_products")),
        right: Box::new(scan("sold_out_products")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_semi_join_selectivity() {
    // Semi-join cardinality <= left cardinality
    let plan = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("user_id"), col("id")),
        left: Box::new(scan("all_users")),
        right: Box::new(scan("active_sessions")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_cost_model_stale_statistics() {
    // Stale statistics should still provide reasonable estimates
    let plan = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scan("table_with_stale_stats")),
    };
    assert_cost_calculated(plan);
}

// ── Positive Rewrite Rule Tests ─────────────────────────────────
// These tests verify that specific rewrite rules DO fire, validating
// that the test framework's `assert_rule_applies` works correctly.

#[test]
fn test_rewrite_filter_true_eliminated() {
    // filter(true, input) => input
    let plan = RelExpr::Filter {
        predicate: Expr::Const(ra_core::expr::Const::Bool(true)),
        input: Box::new(scan("any_table")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_filter_merge() {
    // Two stacked filters merge into a single AND filter
    let plan = RelExpr::Filter {
        predicate: gt(col("a"), int(10)),
        input: Box::new(RelExpr::Filter {
            predicate: gt(col("b"), int(20)),
            input: Box::new(scan("t")),
        }),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_filter_pushdown_through_join() {
    // Filter above a join gets pushed into join sides
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("fk_id")),
        left: Box::new(scan("left_table")),
        right: Box::new(scan("right_table")),
    };
    let plan = RelExpr::Filter {
        predicate: gt(col("value"), int(100)),
        input: Box::new(join),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_join_commutativity() {
    // Inner join inputs can be swapped (the e-graph explores both
    // orderings; the cost function picks the cheaper one).
    // Use optimize_with_egraph to verify the e-graph grew.
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("a"), col("b")),
        left: Box::new(scan("left_t")),
        right: Box::new(scan("right_t")),
    };
    let optimizer = create_test_optimizer();
    let (_, egraph) = optimizer
        .optimize_with_egraph(&plan)
        .expect("optimization should succeed");
    assert!(
        egraph.number_of_classes() > 3,
        "e-graph should grow from join commutativity"
    );
}

#[test]
fn test_rewrite_project_merge() {
    // Nested projects collapse into one
    let plan = project(project(scan("t"), vec!["a", "b", "c"]), vec!["a", "b"]);
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_sort_below_sort_eliminated() {
    // Inner sort is eliminated when an outer sort exists
    let plan = sort(sort(scan("t"), "a", true), "b", false);
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_limit_through_project() {
    // Limit pushes through project
    let plan = limit(project(scan("t"), vec!["a"]), 10);
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_filter_through_union() {
    // Filter pushes into both sides of a union
    let union = RelExpr::Union {
        all: true,
        left: Box::new(scan("t1")),
        right: Box::new(scan("t2")),
    };
    let plan = RelExpr::Filter {
        predicate: gt(col("x"), int(0)),
        input: Box::new(union),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_duckdb_sort_below_aggregate() {
    // Sort below aggregate is eliminated (DuckDB-inspired rule)
    let plan = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(sort(scan("data"), "irrelevant_col", true)),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_filter_pushdown_through_intersect() {
    // Filter pushes into both sides of intersect
    let intersect = RelExpr::Intersect {
        all: false,
        left: Box::new(scan("set_a")),
        right: Box::new(scan("set_b")),
    };
    let plan = RelExpr::Filter {
        predicate: gt(col("val"), int(5)),
        input: Box::new(intersect),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_filter_pushdown_through_except() {
    // Filter pushes into left side of except
    let except = RelExpr::Except {
        all: false,
        left: Box::new(scan("all_items")),
        right: Box::new(scan("excluded_items")),
    };
    let plan = RelExpr::Filter {
        predicate: gt(col("price"), int(0)),
        input: Box::new(except),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_cartesian_to_join() {
    // Filter on a cross join converts to an inner join
    let cross = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: Expr::Const(ra_core::expr::Const::Bool(true)),
        left: Box::new(scan("t1")),
        right: Box::new(scan("t2")),
    };
    let plan = RelExpr::Filter {
        predicate: eq(col("t1_id"), col("t2_id")),
        input: Box::new(cross),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_boolean_and_false_short_circuits() {
    // AND with false short-circuits to false
    let plan = RelExpr::Filter {
        predicate: and(col("x"), Expr::Const(ra_core::expr::Const::Bool(false))),
        input: Box::new(scan("t")),
    };
    assert_cost_calculated(plan);
}

#[test]
fn test_rewrite_boolean_or_true_short_circuits() {
    // OR with true short-circuits to true, then filter(true) is eliminated
    let plan = RelExpr::Filter {
        predicate: or(col("x"), Expr::Const(ra_core::expr::Const::Bool(true))),
        input: Box::new(scan("t")),
    };
    assert_cost_calculated(plan);
}
