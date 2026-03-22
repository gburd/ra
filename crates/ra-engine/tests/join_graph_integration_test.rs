//! Integration tests for join graph filtering (Task #259).
//!
//! Validates that join graph construction works correctly with the optimizer.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::{JoinGraph, Optimizer, OptimizerConfig};

fn qual_col(table: &str, name: &str) -> Expr {
    Expr::Column(ColumnRef::qualified(table, name))
}

fn eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan {
        table: name.to_string(),
        alias: None,
    }
}

fn join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: cond,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn make_optimizer_with_stats(table_names: &[&str]) -> Optimizer {
    let mut opt = Optimizer::new();
    for name in table_names {
        let mut stats = Statistics::new(10000.0);
        stats.avg_row_size = 100;
        stats.total_size = 1_000_000;
        opt.add_table_stats(*name, stats);
    }
    opt
}

#[test]
fn test_join_graph_construction() {
    // Build a query with qualified column names
    let query = join(
        join(
            scan("users"),
            scan("orders"),
            eq(qual_col("users", "id"), qual_col("orders", "user_id")),
        ),
        scan("products"),
        eq(qual_col("orders", "product_id"), qual_col("products", "id")),
    );

    // Build join graph
    let join_graph = JoinGraph::from_expr(&query);
    let stats = join_graph.stats();

    assert_eq!(stats.table_count, 3);
    assert_eq!(stats.edge_count, 2);
    assert!(join_graph.can_join("users", "orders"));
    assert!(join_graph.can_join("orders", "products"));
    assert!(!join_graph.can_join("users", "products")); // No direct edge
}

#[test]
fn test_join_graph_with_optimizer() {
    // Query with qualified columns
    let query = join(
        join(
            scan("a"),
            scan("b"),
            eq(qual_col("a", "id"), qual_col("b", "a_id")),
        ),
        join(
            scan("c"),
            scan("d"),
            eq(qual_col("c", "id"), qual_col("d", "c_id")),
        ),
        eq(qual_col("b", "c_id"), qual_col("c", "id")),
    );

    let optimizer = make_optimizer_with_stats(&["a", "b", "c", "d"]);
    let result = optimizer.optimize(&query);
    assert!(result.is_ok());

    // Check join graph structure
    let join_graph = JoinGraph::from_expr(&query);
    let stats = join_graph.stats();

    assert_eq!(stats.table_count, 4);
    assert_eq!(stats.edge_count, 3);
}

#[test]
fn test_star_schema_join_graph() {
    // Star schema: fact table joins to multiple dimension tables
    let query = join(
        join(
            join(
                scan("fact"),
                scan("dim1"),
                eq(qual_col("fact", "dim1_id"), qual_col("dim1", "id")),
            ),
            scan("dim2"),
            eq(qual_col("fact", "dim2_id"), qual_col("dim2", "id")),
        ),
        scan("dim3"),
        eq(qual_col("fact", "dim3_id"), qual_col("dim3", "id")),
    );

    let join_graph = JoinGraph::from_expr(&query);
    let stats = join_graph.stats();

    // Star schema: low density (only hub-to-spoke edges)
    assert_eq!(stats.table_count, 4);
    assert_eq!(stats.edge_count, 3);

    // Density = 3 / (4*3/2) = 3/6 = 0.5
    assert!((stats.density() - 0.5).abs() < 0.01);

    // Star schema benefits from filtering (sparse graph)
    assert!(stats.estimated_reduction_factor() > 0.5); // >50% reduction
}

#[test]
fn test_clique_join_graph() {
    // Fully connected graph (all tables join to all others)
    let query = join(
        join(
            scan("a"),
            scan("b"),
            eq(qual_col("a", "id"), qual_col("b", "id")),
        ),
        scan("c"),
        Expr::BinOp {
            op: BinOp::And,
            left: Box::new(eq(qual_col("b", "id"), qual_col("c", "id"))),
            right: Box::new(eq(qual_col("a", "id"), qual_col("c", "id"))),
        },
    );

    let join_graph = JoinGraph::from_expr(&query);
    let stats = join_graph.stats();

    // All 3 edges exist (complete graph)
    assert_eq!(stats.table_count, 3);
    assert_eq!(stats.edge_count, 3);

    // Density = 3 / (3*2/2) = 3/3 = 1.0
    assert!((stats.density() - 1.0).abs() < 0.01);

    // Dense graph: minimal benefit from filtering
    assert!(stats.estimated_reduction_factor() < 0.1);
}

#[test]
fn test_disconnected_components() {
    // Two separate join pairs (cartesian product between them)
    let query = join(
        join(
            scan("a"),
            scan("b"),
            eq(qual_col("a", "id"), qual_col("b", "id")),
        ),
        join(
            scan("c"),
            scan("d"),
            eq(qual_col("c", "id"), qual_col("d", "id")),
        ),
        Expr::Const(ra_core::expr::Const::Bool(true)), // Cartesian product
    );

    let join_graph = JoinGraph::from_expr(&query);
    let stats = join_graph.stats();

    // Two components, no cross-component edges
    assert_eq!(stats.table_count, 4);
    assert_eq!(stats.edge_count, 2);

    // Check connectivity
    assert!(join_graph.is_connected(&["a".to_string(), "b".to_string()]));
    assert!(join_graph.is_connected(&["c".to_string(), "d".to_string()]));
    assert!(!join_graph.is_connected(&[
        "a".to_string(),
        "b".to_string(),
        "c".to_string(),
        "d".to_string()
    ]));
}

#[test]
fn test_join_graph_enabled_config() {
    let query = join(
        scan("users"),
        scan("orders"),
        eq(qual_col("users", "id"), qual_col("orders", "user_id")),
    );

    // Test with join graph enabled (default)
    let optimizer = make_optimizer_with_stats(&["users", "orders"]);
    let result = optimizer.optimize(&query);
    assert!(result.is_ok());

    // Test with join graph disabled
    let mut config = OptimizerConfig::default();
    config.use_join_graph_filtering = false;

    let mut optimizer_disabled = Optimizer::with_config(config);
    let mut stats = Statistics::new(10000.0);
    stats.avg_row_size = 100;
    stats.total_size = 1_000_000;
    optimizer_disabled.add_table_stats("users", stats.clone());
    optimizer_disabled.add_table_stats("orders", stats);

    let result = optimizer_disabled.optimize(&query);
    assert!(result.is_ok());
}
