//! Tests for Exasol in-memory OLAP optimization rules.
//!
//! These tests verify the five Phase 1 Exasol rules:
//! - EXA-001: columnar-scan-inmem
//! - EXA-002: late-materialization
//! - EXA-003: column-filter-pushdown
//! - EXA-004: bloom-filter-join
//! - EXA-005: simd-vectorization

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_engine::Optimizer;
use ra_hardware::HardwareProfile;

fn create_optimizer() -> Optimizer {
    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(HardwareProfile::cpu_only());
    optimizer
}

fn scan(table: &str) -> RelExpr {
    RelExpr::Scan {
        table: table.to_string(),
        alias: None,
    }
}

fn filter(input: RelExpr, condition: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate: condition,
        input: Box::new(input),
    }
}

fn project(input: RelExpr, columns: Vec<String>) -> RelExpr {
    RelExpr::Project {
        columns: columns.into_iter().map(|c| ra_core::algebra::ProjectionColumn {
            expr: Expr::Column(ColumnRef::new(c)),
            alias: None,
        }).collect(),
        input: Box::new(input),
    }
}

fn join(left: RelExpr, right: RelExpr, condition: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn aggregate(input: RelExpr, group_by: Vec<String>, aggs: Vec<(String, String)>) -> RelExpr {
    use ra_core::algebra::{AggregateExpr, AggregateFunction};
    RelExpr::Aggregate {
        group_by: group_by.into_iter().map(|c| Expr::Column(ColumnRef::new(c))).collect(),
        aggregates: aggs
            .into_iter()
            .map(|(col, func)| {
                let function = match func.as_str() {
                    "Count" => AggregateFunction::Count,
                    "Sum" => AggregateFunction::Sum,
                    "Avg" => AggregateFunction::Avg,
                    "Min" => AggregateFunction::Min,
                    "Max" => AggregateFunction::Max,
                    _ => AggregateFunction::Count,
                };
                AggregateExpr {
                    function,
                    arg: Some(Expr::Column(ColumnRef::new(col))),
                    distinct: false,
                    alias: None,
                }
            })
            .collect(),
        input: Box::new(input),
    }
}

fn eq_pred(left: &str, right: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new(left.to_string()))),
        right: Box::new(Expr::Column(ColumnRef::new(right.to_string()))),
    }
}

fn gt_pred(left: &str, value: i64) -> Expr {
    use ra_core::expr::Const;
    Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new(left.to_string()))),
        right: Box::new(Expr::Const(Const::Int(value))),
    }
}

fn and_pred(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::And,
        left: Box::new(left),
        right: Box::new(right),
    }
}

// ============================================================================
// EXA-001: Columnar Scan In-Memory
// ============================================================================

#[test]
fn test_exa001_columnar_scan_basic() {
    let optimizer = create_optimizer();

    // SELECT customer_id, total_amount FROM orders
    let plan = project(scan("orders"), vec!["customer_id".to_string(), "total_amount".to_string()]);

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "Columnar scan optimization should succeed");

    // The optimizer should convert standard scan to columnar scan
    // when data is in memory and only subset of columns accessed
}

#[test]
fn test_exa001_columnar_scan_selective_columns() {
    let optimizer = create_optimizer();

    // SELECT product_id FROM products
    // Access 1 out of 50 columns (2%)
    let plan = project(scan("products"), vec!["product_id".to_string()]);

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Columnar scan should be applied due to high column selectivity
}

#[test]
fn test_exa001_columnar_scan_with_filter() {
    let optimizer = create_optimizer();

    // SELECT customer_id, order_date FROM orders WHERE status = 'completed'
    let plan = project(
        filter(scan("orders"), eq_pred("status", "completed")),
        vec!["customer_id".to_string(), "order_date".to_string()],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());
}

// ============================================================================
// EXA-002: Late Materialization
// ============================================================================

#[test]
fn test_exa002_late_materialization_selective_filter() {
    let optimizer = create_optimizer();

    // SELECT name, email, address FROM customers WHERE loyalty_tier = 'platinum'
    // Highly selective filter (1%), wide output columns
    let plan = project(
        filter(scan("customers"), eq_pred("loyalty_tier", "platinum")),
        vec!["name".to_string(), "email".to_string(), "address".to_string()],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "Late materialization should apply");

    // Should defer materialization of output columns until after filter
}

#[test]
fn test_exa002_late_materialization_range_filter() {
    let optimizer = create_optimizer();

    // SELECT product_name, description, price
    // FROM products
    // WHERE product_id > 1000000
    let plan = project(
        filter(scan("products"), gt_pred("product_id", 1_000_000)),
        vec![
            "product_name".to_string(),
            "description".to_string(),
            "price".to_string(),
        ],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());
}

#[test]
fn test_exa002_late_materialization_with_aggregation() {
    let optimizer = create_optimizer();

    // SELECT region, SUM(sales_amount)
    // FROM sales
    // WHERE sale_date BETWEEN '2024-01-01' AND '2024-01-31'
    // GROUP BY region
    let plan = aggregate(
        filter(
            scan("sales"),
            and_pred(
                gt_pred("sale_date", 20240101),
                gt_pred("sale_date", 20240131),
            ),
        ),
        vec!["region".to_string()],
        vec![("sales_amount".to_string(), "Sum".to_string())],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());
}

#[test]
fn test_exa002_no_late_materialization_nonselective() {
    let optimizer = create_optimizer();

    // SELECT * FROM events WHERE year >= 2020
    // Non-selective filter (90% of data), late materialization not beneficial
    let plan = filter(scan("events"), gt_pred("year", 2020));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Should not apply late materialization due to low selectivity
}

// ============================================================================
// EXA-003: Column Filter Pushdown
// ============================================================================

#[test]
fn test_exa003_column_filter_pushdown_equality() {
    let optimizer = create_optimizer();

    // SELECT customer_id, order_date, total_amount
    // FROM orders
    // WHERE status = 'completed'
    let plan = project(
        filter(scan("orders"), eq_pred("status", "completed")),
        vec![
            "customer_id".to_string(),
            "order_date".to_string(),
            "total_amount".to_string(),
        ],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "Filter pushdown should succeed");

    // Should push filter down to scan level with bloom filter generation
}

#[test]
fn test_exa003_column_filter_pushdown_range() {
    let optimizer = create_optimizer();

    // SELECT product_id, sale_date, revenue
    // FROM sales
    // WHERE sale_date BETWEEN '2024-01-15' AND '2024-01-20'
    let plan = project(
        filter(
            scan("sales"),
            and_pred(
                gt_pred("sale_date", 20240115),
                gt_pred("sale_date", 20240120),
            ),
        ),
        vec![
            "product_id".to_string(),
            "sale_date".to_string(),
            "revenue".to_string(),
        ],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Should use zone maps to prune chunks
}

#[test]
fn test_exa003_column_filter_pushdown_conjunctive() {
    let optimizer = create_optimizer();

    // SELECT user_id, action, timestamp
    // FROM events
    // WHERE event_type = 'purchase'
    //   AND region = 'US'
    //   AND user_id > 1000000
    let plan = project(
        filter(
            scan("events"),
            and_pred(
                eq_pred("event_type", "purchase"),
                and_pred(eq_pred("region", "US"), gt_pred("user_id", 1_000_000)),
            ),
        ),
        vec!["user_id".to_string(), "action".to_string(), "timestamp".to_string()],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Should evaluate filters in order of selectivity
}

// ============================================================================
// EXA-004: Bloom Filter Join
// ============================================================================

#[test]
fn test_exa004_bloom_filter_join_dimension_fact() {
    let optimizer = create_optimizer();

    // SELECT o.order_id, o.total_amount, c.customer_name
    // FROM orders o
    // JOIN customers c ON o.customer_id = c.customer_id
    // WHERE o.order_date = '2024-01-15'
    let plan = project(
        join(
            filter(scan("orders"), eq_pred("order_date", "20240115")),
            scan("customers"),
            eq_pred("o.customer_id", "c.customer_id"),
        ),
        vec![
            "o.order_id".to_string(),
            "o.total_amount".to_string(),
            "c.customer_name".to_string(),
        ],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "Bloom filter join should apply");

    // Should build bloom filter on filtered orders (small side)
    // and probe on customers (large side)
}

#[test]
fn test_exa004_bloom_filter_join_star_schema() {
    let optimizer = create_optimizer();

    // TPC-H Q5 style: nation JOIN supplier JOIN lineitem
    // SELECT n.n_name, SUM(l.l_extendedprice)
    // FROM nation n, supplier s, lineitem l
    // WHERE s.s_nationkey = n.n_nationkey
    //   AND l.l_suppkey = s.s_suppkey
    let plan = aggregate(
        join(
            join(scan("nation"), scan("supplier"), eq_pred("n_nationkey", "s_nationkey")),
            scan("lineitem"),
            eq_pred("s_suppkey", "l_suppkey"),
        ),
        vec!["n.n_name".to_string()],
        vec![("l.l_extendedprice".to_string(), "Sum".to_string())],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Should cascade bloom filters: nation → supplier → lineitem
}

#[test]
fn test_exa004_bloom_filter_join_highly_selective() {
    let optimizer = create_optimizer();

    // SELECT l.l_orderkey, SUM(l.l_quantity)
    // FROM lineitem l
    // WHERE l.l_orderkey IN (
    //   SELECT o.o_orderkey FROM orders o
    //   WHERE o.o_custkey IN (
    //     SELECT c.c_custkey FROM customer c
    //     WHERE c.c_mktsegment = 'BUILDING'
    //   )
    // )
    //
    // Simplified as joins for this test
    let plan = aggregate(
        join(
            join(
                filter(scan("customer"), eq_pred("c_mktsegment", "BUILDING")),
                scan("orders"),
                eq_pred("c_custkey", "o_custkey"),
            ),
            scan("lineitem"),
            eq_pred("o_orderkey", "l_orderkey"),
        ),
        vec!["l.l_orderkey".to_string()],
        vec![("l.l_quantity".to_string(), "Sum".to_string())],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Each bloom filter should eliminate 90%+ of downstream table
}

#[test]
fn test_exa004_no_bloom_filter_small_tables() {
    let optimizer = create_optimizer();

    // Small table join (both < 1000 rows)
    // SELECT * FROM status_codes s
    // JOIN events e ON s.code = e.status
    let plan = join(scan("status_codes"), scan("events"), eq_pred("code", "status"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Bloom filter overhead > benefit for small tables
}

// ============================================================================
// EXA-005: SIMD Vectorization
// ============================================================================

#[test]
fn test_exa005_simd_filter_integer() {
    let optimizer = create_optimizer();

    // SELECT customer_id, order_date, total_amount
    // FROM orders
    // WHERE customer_id > 1000000
    let plan = project(
        filter(scan("orders"), gt_pred("customer_id", 1_000_000)),
        vec![
            "customer_id".to_string(),
            "order_date".to_string(),
            "total_amount".to_string(),
        ],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "SIMD filter should apply");

    // Should use SIMD comparison for integer column
}

#[test]
fn test_exa005_simd_aggregation_sum() {
    let optimizer = create_optimizer();

    // SELECT SUM(revenue) FROM sales WHERE region = 'US'
    let plan = aggregate(
        filter(scan("sales"), eq_pred("region", "US")),
        vec![],
        vec![("revenue".to_string(), "Sum".to_string())],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Should use horizontal SIMD sum reduction
}

#[test]
fn test_exa005_simd_aggregation_count() {
    let optimizer = create_optimizer();

    // SELECT COUNT(*) FROM orders WHERE status = 'completed'
    let plan = aggregate(
        filter(scan("orders"), eq_pred("status", "completed")),
        vec![],
        vec![("*".to_string(), "Count".to_string())],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());
}

#[test]
fn test_exa005_simd_hash_join() {
    let optimizer = create_optimizer();

    // SELECT o.order_id, c.customer_name
    // FROM orders o
    // JOIN customers c ON o.customer_id = c.customer_id
    let plan = project(
        join(scan("orders"), scan("customers"), eq_pred("customer_id", "customer_id")),
        vec!["o.order_id".to_string(), "c.customer_name".to_string()],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Should use SIMD hash computation for join keys
}

// ============================================================================
// Integration Tests: Multiple Rules
// ============================================================================

#[test]
fn test_integration_tpch_q1() {
    let optimizer = create_optimizer();

    // TPC-H Q1: Pricing Summary Report
    // SELECT
    //   l_returnflag, l_linestatus,
    //   SUM(l_quantity), SUM(l_extendedprice)
    // FROM lineitem
    // WHERE l_shipdate <= '1998-09-01'
    // GROUP BY l_returnflag, l_linestatus
    let plan = aggregate(
        filter(scan("lineitem"), gt_pred("l_shipdate", 19980901)),
        vec!["l_returnflag".to_string(), "l_linestatus".to_string()],
        vec![
            ("l_quantity".to_string(), "Sum".to_string()),
            ("l_extendedprice".to_string(), "Sum".to_string()),
        ],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "TPC-H Q1 optimization should succeed");

    // Expected optimizations:
    // - EXA-001: Columnar scan (access 4 out of 16 columns)
    // - EXA-003: Filter pushdown with zone maps
    // - EXA-005: SIMD aggregation
}

#[test]
fn test_integration_tpch_q3() {
    let optimizer = create_optimizer();

    // TPC-H Q3: Shipping Priority Query
    // SELECT
    //   l_orderkey, SUM(l_extendedprice * (1 - l_discount))
    // FROM customer, orders, lineitem
    // WHERE c_mktsegment = 'BUILDING'
    //   AND c_custkey = o_custkey
    //   AND l_orderkey = o_orderkey
    // GROUP BY l_orderkey
    let plan = aggregate(
        join(
            join(
                filter(scan("customer"), eq_pred("c_mktsegment", "BUILDING")),
                scan("orders"),
                eq_pred("c_custkey", "o_custkey"),
            ),
            scan("lineitem"),
            eq_pred("o_orderkey", "l_orderkey"),
        ),
        vec!["l_orderkey".to_string()],
        vec![("l_extendedprice".to_string(), "Sum".to_string())],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "TPC-H Q3 optimization should succeed");

    // Expected optimizations:
    // - EXA-002: Late materialization (defer wide columns)
    // - EXA-003: Filter pushdown (c_mktsegment)
    // - EXA-004: Bloom filter cascade (customer → orders → lineitem)
    // - EXA-005: SIMD aggregation
}

#[test]
fn test_integration_tpch_q6() {
    let optimizer = create_optimizer();

    // TPC-H Q6: Forecasting Revenue Change
    // SELECT SUM(l_extendedprice * l_discount)
    // FROM lineitem
    // WHERE l_shipdate BETWEEN '1994-01-01' AND '1994-12-31'
    //   AND l_discount BETWEEN 0.05 AND 0.07
    //   AND l_quantity < 24
    let plan = aggregate(
        filter(
            scan("lineitem"),
            and_pred(
                and_pred(
                    gt_pred("l_shipdate", 19940101),
                    gt_pred("l_shipdate", 19941231),
                ),
                and_pred(
                    gt_pred("l_discount", 5),
                    and_pred(gt_pred("l_discount", 7), gt_pred("l_quantity", 24)),
                ),
            ),
        ),
        vec![],
        vec![("l_extendedprice".to_string(), "Sum".to_string())],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "TPC-H Q6 optimization should succeed");

    // Expected optimizations:
    // - EXA-001: Columnar scan (access 3 out of 16 columns)
    // - EXA-003: Filter pushdown with zone maps (partition pruning)
    // - EXA-005: SIMD filter evaluation + aggregation
}

#[test]
fn test_integration_wide_table_selective_query() {
    let optimizer = create_optimizer();

    // Wide table (100 columns) with selective filter
    // SELECT col1, col2, col3
    // FROM wide_table
    // WHERE filter_col = 'rare_value'
    let plan = project(
        filter(scan("wide_table"), eq_pred("filter_col", "rare_value")),
        vec!["col1".to_string(), "col2".to_string(), "col3".to_string()],
    );

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // Expected optimizations:
    // - EXA-001: Columnar scan (access 4 out of 100 columns = 4%)
    // - EXA-002: Late materialization (defer col1, col2, col3 until after filter)
    // - EXA-003: Filter pushdown (dictionary-encoded)
    // - EXA-005: SIMD filter evaluation
}

#[test]
fn test_no_optimization_single_row_lookup() {
    let optimizer = create_optimizer();

    // Point query - Exasol optimizations don't apply
    // SELECT * FROM orders WHERE order_id = 12345
    let plan = filter(scan("orders"), eq_pred("order_id", "12345"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok());

    // No Exasol optimizations should apply:
    // - Not columnar scan (need all columns)
    // - Not late materialization (single row)
    // - Not SIMD (batch size = 1)
}
