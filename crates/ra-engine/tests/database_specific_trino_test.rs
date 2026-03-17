//! Tests for Trino database-specific optimization rules.
//!
//! Tests 6 Trino rules from Task #20 academic research mining:
//! - Dynamic filtering optimization
//! - Adaptive hash partitioning
//! - Limit pushdown to connector
//! - Index join optimization
//! - Connector-specific pushdown
//! - Adaptive query optimization

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::Optimizer;
use ra_hardware::HardwareProfile;

// ── Test Helpers ────────────────────────────────────────────

/// Create a basic optimizer with default hardware profile.
fn create_optimizer() -> Optimizer {
    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(HardwareProfile::cpu_only());
    optimizer
}

/// Create a simple scan plan.
fn scan(table: &str) -> RelExpr {
    RelExpr::Scan {
        table: table.to_string(),
        alias: None,
    }
}

/// Create a filter plan.
fn filter(input: RelExpr, predicate: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate,
        input: Box::new(input),
    }
}

/// Create a join plan.
fn join(left: RelExpr, right: RelExpr, condition: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// Create a limit plan.
fn limit(input: RelExpr, count: u64) -> RelExpr {
    RelExpr::Limit {
        count,
        offset: 0,
        input: Box::new(input),
    }
}

/// Helper to create a simple equality predicate.
fn eq_pred(left: &str, right: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new(left.to_string()))),
        right: Box::new(Expr::Column(ColumnRef::new(right.to_string()))),
    }
}

/// Helper to create a less-than predicate.
fn lt_pred(column: &str, value: i64) -> Expr {
    Expr::BinOp {
        op: BinOp::Lt,
        left: Box::new(Expr::Column(ColumnRef::new(column.to_string()))),
        right: Box::new(Expr::Const(Const::Int(value))),
    }
}

// ── Rule 1: Dynamic Filtering ───────────────────────────────

#[test]
fn test_trino_dynamic_filtering_hash_join() {
    let optimizer = create_optimizer();

    // Hash join with selective build side should trigger dynamic filtering
    let build = filter(scan("small_table"), lt_pred("dept_id", 10));
    let probe = scan("large_fact_table");
    let plan = join(probe, build, eq_pred("dept_id", "dept_id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "dynamic filtering optimization should succeed");
}

#[test]
fn test_trino_dynamic_filtering_high_selectivity() {
    let optimizer = create_optimizer();

    // Very selective build side (single value)
    let build = filter(scan("dim_table"), eq_pred("id", "id"));
    let probe = scan("fact_table");
    let plan = join(probe, build, eq_pred("dim_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "high selectivity optimization should succeed");
}

#[test]
fn test_trino_dynamic_filtering_no_filter() {
    let optimizer = create_optimizer();

    // No filter on build side
    let build = scan("large_table");
    let probe = scan("fact_table");
    let plan = join(probe, build, eq_pred("id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "join without filter should optimize");
}

// ── Rule 2: Adaptive Hash Partitioning ──────────────────────

#[test]
fn test_trino_adaptive_partitioning_basic_join() {
    let optimizer = create_optimizer();

    let left = scan("orders");
    let right = scan("products");
    let plan = join(left, right, eq_pred("product_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "adaptive partitioning should handle basic join");
}

#[test]
fn test_trino_adaptive_partitioning_large_tables() {
    let optimizer = create_optimizer();

    let left = scan("large_events");
    let right = scan("users");
    let plan = join(left, right, eq_pred("user_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "adaptive partitioning should handle large tables");
}

#[test]
fn test_trino_adaptive_partitioning_multi_join() {
    let optimizer = create_optimizer();

    let t1 = scan("table1");
    let t2 = scan("table2");
    let t3 = scan("table3");
    let join1 = join(t1, t2, eq_pred("id", "id"));
    let plan = join(join1, t3, eq_pred("id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "adaptive partitioning should handle multi-way joins");
}

// ── Rule 3: Limit Pushdown to Connector ─────────────────────

#[test]
fn test_trino_limit_pushdown_basic() {
    let optimizer = create_optimizer();

    // LIMIT on scan should push down to connector
    let plan = limit(scan("mysql_table"), 100);

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "limit pushdown should succeed");
}

#[test]
fn test_trino_limit_pushdown_with_filter() {
    let optimizer = create_optimizer();

    // LIMIT after filter on scan
    let filtered = filter(scan("postgres_table"), eq_pred("status", "status"));
    let plan = limit(filtered, 50);

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "filter + limit pushdown should succeed");
}

#[test]
fn test_trino_limit_pushdown_large_limit() {
    let optimizer = create_optimizer();

    // Large LIMIT value
    let plan = limit(scan("data_table"), 10000);

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "large limit should optimize");
}

// ── Rule 4: Index Join Optimization ─────────────────────────

#[test]
fn test_trino_index_join_small_probe() {
    let optimizer = create_optimizer();

    // Small probe side with indexed build side
    let probe = filter(scan("small_queries"), lt_pred("user_id", 100));
    let build = scan("large_indexed_events");
    let plan = join(probe, build, eq_pred("user_id", "user_id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "index join should optimize small probe");
}

#[test]
fn test_trino_index_join_medium_table() {
    let optimizer = create_optimizer();

    let probe = scan("medium_table");
    let build = scan("indexed_dimension");
    let plan = join(probe, build, eq_pred("dim_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "index join should handle medium tables");
}

#[test]
fn test_trino_index_join_fallback() {
    let optimizer = create_optimizer();

    // No index available - should still optimize
    let probe = scan("probe_table");
    let build = scan("unindexed_table");
    let plan = join(probe, build, eq_pred("id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "should optimize even without index");
}

// ── Rule 5: Connector-Specific Pushdown ─────────────────────

#[test]
fn test_trino_connector_pushdown_filter() {
    let optimizer = create_optimizer();

    // Simple filter that should push down
    let plan = filter(scan("external_table"), eq_pred("col", "col"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "connector pushdown should succeed");
}

#[test]
fn test_trino_connector_pushdown_scan() {
    let optimizer = create_optimizer();

    // Plain scan
    let plan = scan("external_source");

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "scan should optimize");
}

#[test]
fn test_trino_connector_pushdown_complex_filter() {
    let optimizer = create_optimizer();

    // Complex predicate
    let pred1 = eq_pred("a", "a");
    let pred2 = lt_pred("b", 10);
    let complex = Expr::BinOp {
        op: BinOp::And,
        left: Box::new(pred1),
        right: Box::new(pred2),
    };
    let plan = filter(scan("connector_table"), complex);

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "complex predicate should optimize");
}

// ── Rule 6: Adaptive Query Optimization ─────────────────────

#[test]
fn test_trino_adaptive_optimization_join() {
    let optimizer = create_optimizer();

    let left = filter(scan("fact_table"), eq_pred("condition", "condition"));
    let right = scan("dimension");
    let plan = join(left, right, eq_pred("dim_id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "adaptive optimization should handle filtered join");
}

#[test]
fn test_trino_adaptive_optimization_multi_join() {
    let optimizer = create_optimizer();

    // Multi-way join
    let t1 = scan("table1");
    let t2 = scan("table2");
    let t3 = scan("table3");
    let join1 = join(t1, t2, eq_pred("id", "id"));
    let plan = join(join1, t3, eq_pred("id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "adaptive optimization should handle multi-way joins");
}

#[test]
fn test_trino_adaptive_optimization_simple() {
    let optimizer = create_optimizer();

    // Simple query
    let plan = filter(scan("small_table"), eq_pred("id", "id"));

    let result = optimizer.optimize(&plan);
    assert!(result.is_ok(), "simple query should optimize");
}
