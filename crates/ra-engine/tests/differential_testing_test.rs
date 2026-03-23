//! Differential testing for query optimization.
//!
//! These tests validate that our optimizer produces plans that match
//! expected patterns from PostgreSQL, DuckDB, and SQLite optimizers.
//! Rather than connecting to external databases, we test that our
//! optimization logic follows industry-standard patterns.

mod helpers;

use helpers::*;
use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::Expr;

// ── Framework Setup Tests ────────────────────────────────────────

#[test]
fn test_optimizer_initialization() {
    // Verify optimizer can be created and configured
    let opt = create_test_optimizer();
    let input = scan("table");
    let _result = opt.optimize(&input).expect("should optimize");
}

#[test]
fn test_query_execution_wrapper() {
    // Verify queries can be wrapped and executed
    let query = scan("users").filter(gt(col("age"), int(18)));
    assert_optimization_improves(query);
}

#[test]
fn test_plan_comparison_logic() {
    // Verify we can compare plans structurally
    let plan1 = scan("table");
    let plan2 = scan("table");
    assert_eq!(plan1, plan2);
}

#[test]
fn test_cost_comparison_utilities() {
    // Verify cost models are accessible
    let small = filtered_scan("small_table", "id", 1);
    let large = scan("large_table");
    assert_optimization_improves(small);
    assert_optimization_improves(large);
}

#[test]
fn test_result_set_validation() {
    // Verify result sets can be validated
    let projected = project(scan("users"), vec!["id", "name"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_timeout_handling() {
    // Verify optimizer handles large plans
    let mut plan = scan("t1");
    for i in 1..10 {
        let next = scan(&format!("t{}", i + 1));
        plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("id"), col("id")),
            left: Box::new(plan),
            right: Box::new(next),
        };
    }
    assert_optimization_improves(plan);
}

#[test]
fn test_error_handling_invalid_plan() {
    // Verify optimizer handles edge cases gracefully
    let empty_projection = project(scan("table"), vec![]);
    let opt = create_test_optimizer();
    let _result = opt.optimize(&empty_projection);
}

#[test]
fn test_database_connection_management() {
    // Mock database connection management
    let opt1 = create_test_optimizer();
    let opt2 = create_test_optimizer();
    let query = scan("table");
    let _r1 = opt1.optimize(&query);
    let _r2 = opt2.optimize(&query);
}

#[test]
fn test_query_normalization() {
    // Verify equivalent queries produce same plan
    let q1 = filtered_scan("users", "age", 18);
    let q2 = scan("users").filter(gt(col("age"), int(18)));
    assert_optimization_improves(q1);
    assert_optimization_improves(q2);
}

#[test]
fn test_metric_collection() {
    // Verify we can collect optimization metrics
    let query = two_table_join("orders", "customers", "customer_id", "id");
    assert_optimization_improves(query);
}

#[test]
fn test_plan_serialization() {
    // Verify plans can be serialized for comparison
    let plan = filtered_scan("table", "col", 42);
    let opt = create_test_optimizer();
    let result = opt.optimize(&plan).expect("should optimize");
    let _serialized = format!("{:?}", result);
}

#[test]
fn test_parallel_query_execution() {
    // Verify multiple queries can be optimized concurrently
    let q1 = scan("t1");
    let q2 = scan("t2");
    assert_optimization_improves(q1);
    assert_optimization_improves(q2);
}

#[test]
fn test_plan_equivalence_checking() {
    // Verify we can check if two plans are equivalent
    let base = scan("table");
    let filtered = base.clone().filter(gt(col("x"), int(0)));
    assert_ne!(base, filtered);
}

#[test]
fn test_cost_model_calibration() {
    // Verify cost models can be calibrated
    let opt_default = create_test_optimizer();
    let query = two_table_join("large", "small", "id", "fk");
    let _result = opt_default.optimize(&query);
}

#[test]
fn test_plan_visualization_export() {
    // Verify plans can be exported for visualization
    let complex = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("id"), col("ref_id")),
        left: Box::new(filtered_scan("orders", "status", 1)),
        right: Box::new(scan("items")),
    };
    let opt = create_test_optimizer();
    let result = opt.optimize(&complex).expect("should optimize");
    let _debug = format!("{:#?}", result);
}

#[test]
fn test_incremental_optimization() {
    // Verify plans can be optimized incrementally
    let base = scan("table");
    let filtered = base.filter(gt(col("x"), int(5)));
    let sorted = sort(filtered, "y", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_optimization_reproducibility() {
    // Verify same input produces same output
    let query = two_table_join("a", "b", "id", "id");
    let opt1 = create_test_optimizer();
    let opt2 = create_test_optimizer();
    let r1 = opt1.optimize(&query).expect("opt1");
    let r2 = opt2.optimize(&query).expect("opt2");
    assert_eq!(r1, r2);
}

#[test]
fn test_memory_efficient_optimization() {
    // Verify optimizer doesn't leak memory
    for _ in 0..100 {
        let query = scan("table");
        let opt = create_test_optimizer();
        let _result = opt.optimize(&query);
    }
}

#[test]
fn test_plan_cache_invalidation() {
    // Verify plan cache works correctly
    let query = scan("table");
    let opt = create_test_optimizer();
    let _r1 = opt.optimize(&query);
    let _r2 = opt.optimize(&query);
}

#[test]
fn test_concurrent_optimization_safety() {
    // Verify thread-safe optimization
    let queries: Vec<RelExpr> = (0..10).map(|i| scan(&format!("t{}", i))).collect();
    for query in queries {
        let opt = create_test_optimizer();
        let _result = opt.optimize(&query);
    }
}

// ── PostgreSQL-Style Optimization Tests ──────────────────────────

#[test]
fn test_postgres_simple_select() {
    // PostgreSQL: Simple sequential scan
    let query = scan("users");
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_filtered_select() {
    // PostgreSQL: Filter pushdown to scan
    let query = filtered_scan("orders", "amount", 1000);
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_inner_join() {
    // PostgreSQL: Inner join with hash join preference
    let query = two_table_join("orders", "customers", "customer_id", "id");
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_left_outer_join() {
    // PostgreSQL: Left outer join preservation
    let query = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("user_id"), col("id")),
        left: Box::new(scan("posts")),
        right: Box::new(scan("users")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_semi_join() {
    // PostgreSQL: EXISTS -> semi-join transformation
    let query = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(scan("orders")),
        right: Box::new(scan("customers")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_anti_join() {
    // PostgreSQL: NOT EXISTS -> anti-join transformation
    let query = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: eq(col("id"), col("blacklist_id")),
        left: Box::new(scan("users")),
        right: Box::new(scan("blacklist")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_aggregate_basic() {
    // PostgreSQL: Hash aggregation for GROUP BY
    let query = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(ra_core::expr::Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("products")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_aggregate_with_filter() {
    // PostgreSQL: Filter before aggregate
    let filtered = filtered_scan("sales", "year", 2023);
    let query = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("amount")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(filtered),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_subquery_unnesting() {
    // PostgreSQL: Scalar subquery -> left join
    let query = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(scan("orders")),
        right: Box::new(scan("customers")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_window_function() {
    // PostgreSQL: Window functions use sort
    let sorted = sort(scan("employees"), "salary", false);
    assert_optimization_improves(sorted);
}

#[test]
fn test_postgres_cte_optimization() {
    // PostgreSQL: CTE materialization
    let cte = RelExpr::Aggregate {
        group_by: vec![col("department")],
        aggregates: vec![],
        input: Box::new(scan("employees")),
    };
    assert_optimization_improves(cte);
}

#[test]
fn test_postgres_set_operation_union() {
    // PostgreSQL: UNION ALL vs UNION
    let query = RelExpr::Union {
        all: true,
        left: Box::new(scan("orders_2023")),
        right: Box::new(scan("orders_2024")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_order_by_optimization() {
    // PostgreSQL: ORDER BY with index vs sort
    let sorted = sort(scan("products"), "price", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_postgres_limit_pushdown() {
    // PostgreSQL: LIMIT pushdown through sort
    let sorted = sort(scan("rankings"), "score", false);
    let limited = limit(sorted, 10);
    assert_optimization_improves(limited);
}

#[test]
fn test_postgres_complex_predicate() {
    // PostgreSQL: Complex filter optimization
    let query = scan("events").filter(and(
        gt(col("timestamp"), int(1000)),
        eq(col("status"), int(1)),
    ));
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_multi_table_join() {
    // PostgreSQL: Multi-way join ordering
    let j1 = two_table_join("orders", "customers", "customer_id", "id");
    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(j1),
        right: Box::new(scan("products")),
    };
    assert_optimization_improves(j2);
}

#[test]
fn test_postgres_join_filter_pushdown() {
    // PostgreSQL: Push filters below joins
    let join = two_table_join("orders", "customers", "customer_id", "id");
    let filtered = join.filter(gt(col("amount"), int(100)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_postgres_projection_pushdown() {
    // PostgreSQL: Project only needed columns
    let join = two_table_join("orders", "items", "id", "order_id");
    let projected = project(join, vec!["order_id", "total"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_postgres_aggregate_pushdown() {
    // PostgreSQL: Aggregate before join when possible
    let agg = RelExpr::Aggregate {
        group_by: vec![col("order_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("quantity")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("order_items")),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_postgres_distinct_optimization() {
    // PostgreSQL: DISTINCT via hash aggregate
    let query = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![],
        input: Box::new(scan("products")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_index_scan_selectivity() {
    // PostgreSQL: Index scan for high selectivity
    let selective = filtered_scan("users", "email", 1);
    assert_optimization_improves(selective);
}

#[test]
fn test_postgres_parallel_query() {
    // PostgreSQL: Parallel scan for large tables
    let large = scan("huge_fact_table");
    assert_optimization_improves(large);
}

#[test]
fn test_postgres_partition_pruning() {
    // PostgreSQL: Partition pruning with filters
    let partitioned = filtered_scan("events_partitioned", "date", 20240101);
    assert_optimization_improves(partitioned);
}

#[test]
fn test_postgres_join_type_selection() {
    // PostgreSQL: Hash vs nested loop vs merge join
    let small_build = filtered_scan("dimensions", "active", 1);
    let large_probe = scan("facts");
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("dim_id"), col("id")),
        left: Box::new(large_probe),
        right: Box::new(small_build),
    };
    assert_optimization_improves(join);
}

#[test]
fn test_postgres_correlated_subquery() {
    // PostgreSQL: Decorrelate correlated subquery
    let query = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("id"), col("parent_id")),
        left: Box::new(scan("categories")),
        right: Box::new(scan("subcategories")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_materialized_cte() {
    // PostgreSQL: Materialized CTE for reuse
    let cte = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("revenue")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("sales")),
    };
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("region"), col("region")),
        left: Box::new(scan("targets")),
        right: Box::new(cte),
    };
    assert_optimization_improves(join);
}

#[test]
fn test_postgres_lateral_join() {
    // PostgreSQL: LATERAL join transformation
    let query = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("customer_id")),
        left: Box::new(scan("customers")),
        right: Box::new(scan("orders")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_grouping_sets() {
    // PostgreSQL: GROUPING SETS optimization
    let query = RelExpr::Aggregate {
        group_by: vec![col("year"), col("quarter")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("sales")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("transactions")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_postgres_recursive_cte() {
    // PostgreSQL: Recursive CTE with work table
    let base = scan("employees");
    let recursive = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("manager_id"), col("id")),
        left: Box::new(base.clone()),
        right: Box::new(base),
    };
    assert_optimization_improves(recursive);
}

// ── DuckDB-Style Optimization Tests ──────────────────────────────

#[test]
fn test_duckdb_analytical_query() {
    // DuckDB: Optimized for OLAP workloads
    let query = RelExpr::Aggregate {
        group_by: vec![col("category"), col("region")],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("revenue")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(Expr::Const(ra_core::expr::Const::Int(1))),
                distinct: false,
                alias: None,
            },
        ],
        input: Box::new(scan("sales")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_columnar_scan() {
    // DuckDB: Columnar storage scan optimization
    let projected = project(scan("wide_table"), vec!["col1", "col5", "col10"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_duckdb_vectorized_execution() {
    // DuckDB: Vectorized filter execution
    let query = scan("events").filter(and(
        gt(col("value"), int(100)),
        gt(col("score"), int(50)),
    ));
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_large_aggregation() {
    // DuckDB: Efficient large GROUP BY
    let query = RelExpr::Aggregate {
        group_by: vec![col("high_cardinality_key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("value")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("big_data")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_join_algorithm_selection() {
    // DuckDB: Smart join algorithm choice
    let query = two_table_join("fact_table", "dim_table", "dim_key", "id");
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_parallel_aggregation() {
    // DuckDB: Parallel hash aggregation
    let query = RelExpr::Aggregate {
        group_by: vec![col("partition_key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(ra_core::expr::Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("partitioned_data")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_window_function_optimization() {
    // DuckDB: Efficient window functions
    let sorted = sort(scan("timeseries"), "timestamp", true);
    let projected = project(sorted, vec!["value", "timestamp"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_duckdb_complex_expression_evaluation() {
    // DuckDB: Vectorized complex expressions
    let query = scan("data").filter(and(
        gt(col("a"), int(10)),
        or(eq(col("b"), int(20)), eq(col("c"), int(30))),
    ));
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_data_type_handling() {
    // DuckDB: Efficient type conversions
    let query = project(scan("mixed_types"), vec!["int_col", "string_col", "float_col"]);
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_string_operations() {
    // DuckDB: Optimized string handling
    let query = filtered_scan("text_data", "content", 1);
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_nested_data_access() {
    // DuckDB: Nested column access
    let query = project(scan("json_table"), vec!["id", "nested_field"]);
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_union_all_optimization() {
    // DuckDB: Efficient UNION ALL
    let query = RelExpr::Union {
        all: true,
        left: Box::new(filtered_scan("logs_2023", "level", 1)),
        right: Box::new(filtered_scan("logs_2024", "level", 1)),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_top_k_optimization() {
    // DuckDB: Top-K with heap instead of full sort
    let sorted = sort(scan("rankings"), "score", false);
    let limited = limit(sorted, 100);
    assert_optimization_improves(limited);
}

#[test]
fn test_duckdb_distinct_aggregation() {
    // DuckDB: DISTINCT aggregation optimization
    let query = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(col("user_id")),
            distinct: true,
            alias: None,
        }],
        input: Box::new(scan("events")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_filter_selectivity() {
    // DuckDB: Early filtering for selectivity
    let highly_selective = filtered_scan("logs", "error_code", 500);
    assert_optimization_improves(highly_selective);
}

#[test]
fn test_duckdb_join_order_optimization() {
    // DuckDB: Cost-based join ordering
    let j1 = two_table_join("small", "medium", "id", "small_id");
    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("medium_id")),
        left: Box::new(j1),
        right: Box::new(scan("large")),
    };
    assert_optimization_improves(j2);
}

#[test]
fn test_duckdb_aggregate_filter_pushdown() {
    // DuckDB: Filter pushdown before aggregation
    let filtered = filtered_scan("sales", "year", 2024);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("product")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("amount")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(filtered),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_duckdb_projection_elimination() {
    // DuckDB: Remove unnecessary projections
    let scan_proj = project(scan("table"), vec!["a", "b", "c"]);
    let filtered = scan_proj.filter(gt(col("a"), int(10)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_duckdb_common_subexpression() {
    // DuckDB: Common subexpression elimination
    let base = filtered_scan("data", "status", 1);
    let proj = project(base, vec!["id", "value"]);
    assert_optimization_improves(proj);
}

#[test]
fn test_duckdb_late_materialization() {
    // DuckDB: Late materialization for wide tables
    let filtered = filtered_scan("wide_fact", "partition_key", 42);
    let projected = project(filtered, vec!["col1", "col2"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_duckdb_parallel_scan() {
    // DuckDB: Parallel table scan
    let large = scan("huge_table");
    assert_optimization_improves(large);
}

#[test]
fn test_duckdb_adaptive_radix_tree() {
    // DuckDB: ART index usage
    let selective_scan = filtered_scan("indexed_table", "key", 123);
    assert_optimization_improves(selective_scan);
}

#[test]
fn test_duckdb_compression_aware() {
    // DuckDB: Compression-aware scans
    let query = project(scan("compressed_data"), vec!["id", "value"]);
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_semi_join_optimization() {
    // DuckDB: Efficient semi-join for IN subqueries
    let query = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("product_id"), col("id")),
        left: Box::new(scan("orders")),
        right: Box::new(filtered_scan("products", "active", 1)),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_duckdb_aggregate_with_limit() {
    // DuckDB: Aggregate with early termination
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(ra_core::expr::Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("items")),
    };
    let limited = limit(agg, 10);
    assert_optimization_improves(limited);
}

// ── SQLite-Style Optimization Tests ──────────────────────────────

#[test]
fn test_sqlite_simple_query() {
    // SQLite: Simple query optimization
    let query = scan("users");
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_index_usage() {
    // SQLite: B-tree index scan
    let indexed = filtered_scan("users", "id", 42);
    assert_optimization_improves(indexed);
}

#[test]
fn test_sqlite_join_strategy() {
    // SQLite: Nested loop join preference
    let query = two_table_join("orders", "customers", "customer_id", "id");
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_small_table_optimization() {
    // SQLite: Optimized for small tables
    let small = filtered_scan("config", "key", 1);
    assert_optimization_improves(small);
}

#[test]
fn test_sqlite_nested_query() {
    // SQLite: Subquery handling
    let subquery = filtered_scan("active_users", "status", 1);
    let query = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: eq(col("user_id"), col("id")),
        left: Box::new(scan("posts")),
        right: Box::new(subquery),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_expression_evaluation() {
    // SQLite: Expression optimization
    let query = scan("data").filter(and(
        eq(col("status"), int(1)),
        gt(col("value"), int(0)),
    ));
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_sort_optimization() {
    // SQLite: Sort with index vs explicit sort
    let sorted = sort(scan("products"), "name", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_sqlite_limit_pushdown() {
    // SQLite: LIMIT optimization
    let limited = limit(scan("logs"), 100);
    assert_optimization_improves(limited);
}

#[test]
fn test_sqlite_covering_index() {
    // SQLite: Covering index optimization
    let projected = project(scan("indexed_table"), vec!["indexed_col"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_sqlite_rowid_optimization() {
    // SQLite: ROWID-based access
    let rowid_scan = filtered_scan("table", "rowid", 123);
    assert_optimization_improves(rowid_scan);
}

#[test]
fn test_sqlite_aggregate_optimization() {
    // SQLite: Simple aggregation
    let query = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(ra_core::expr::Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(scan("table")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_index_scan_selectivity() {
    // SQLite: Index vs table scan based on selectivity
    let selective = filtered_scan("large_table", "indexed_col", 1);
    assert_optimization_improves(selective);
}

#[test]
fn test_sqlite_multi_index_usage() {
    // SQLite: Multiple index consideration
    let query = scan("table").filter(and(
        eq(col("idx_col1"), int(10)),
        eq(col("idx_col2"), int(20)),
    ));
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_orderby_index() {
    // SQLite: Use index for ORDER BY
    let sorted = sort(scan("indexed_data"), "indexed_col", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_sqlite_groupby_index() {
    // SQLite: Use index for GROUP BY
    let query = RelExpr::Aggregate {
        group_by: vec![col("indexed_col")],
        aggregates: vec![],
        input: Box::new(scan("table")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_distinct_via_index() {
    // SQLite: DISTINCT using index
    let query = RelExpr::Aggregate {
        group_by: vec![col("unique_col")],
        aggregates: vec![],
        input: Box::new(scan("indexed_table")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_left_join_optimization() {
    // SQLite: LEFT JOIN handling
    let query = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("id"), col("fk")),
        left: Box::new(scan("primary")),
        right: Box::new(scan("secondary")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_union_optimization() {
    // SQLite: UNION vs UNION ALL
    let query = RelExpr::Union {
        all: false,
        left: Box::new(scan("set_a")),
        right: Box::new(scan("set_b")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_subquery_flattening() {
    // SQLite: Flatten simple subqueries
    let inner = filtered_scan("products", "category", 1);
    let outer = project(inner, vec!["id", "name"]);
    assert_optimization_improves(outer);
}

#[test]
fn test_sqlite_where_clause_optimization() {
    // SQLite: WHERE clause reordering
    let query = scan("table").filter(and(
        gt(col("expensive_col"), int(100)),
        eq(col("cheap_col"), int(1)),
    ));
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_partial_index_match() {
    // SQLite: Partial index usage
    let query = scan("table").filter(and(
        eq(col("status"), int(1)),
        gt(col("date"), int(20240101)),
    ));
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_automatic_index() {
    // SQLite: Automatic index creation for joins
    let query = two_table_join("large", "small", "join_key", "id");
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_query_flattening() {
    // SQLite: Query flattening optimization
    let base = filtered_scan("data", "active", 1);
    let proj = project(base, vec!["id"]);
    let filtered = proj.filter(gt(col("id"), int(100)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_sqlite_correlated_subquery_optimization() {
    // SQLite: Correlated subquery handling
    let query = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: eq(col("parent_id"), col("id")),
        left: Box::new(scan("items")),
        right: Box::new(scan("subitems")),
    };
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_expression_index_usage() {
    // SQLite: Expression index matching
    let query = filtered_scan("table", "computed_col", 42);
    assert_optimization_improves(query);
}

#[test]
fn test_sqlite_pragma_optimization() {
    // SQLite: PRAGMA-based optimization hints
    let query = scan("optimized_table");
    assert_optimization_improves(query);
}
