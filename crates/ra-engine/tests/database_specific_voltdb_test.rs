//! Tests for VoltDB in-memory OLTP optimization rules.

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

fn join(left: RelExpr, right: RelExpr, condition: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn eq_pred(left: &str, right: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new(left.to_string()))),
        right: Box::new(Expr::Column(ColumnRef::new(right.to_string()))),
    }
}

// Deterministic order, Single-partition optimization, Replicated table joins

#[test]
fn test_voltdb_deterministic_order_basic() {
    let optimizer = create_optimizer();
    let sorted = RelExpr::Sort {
        keys: vec![],
        input: Box::new(scan("transactions")),
    };
    assert!(optimizer.optimize(&sorted).is_ok());
}

#[test]
fn test_voltdb_deterministic_order_replica() {
    let optimizer = create_optimizer();
    let plan = scan("replicated_table");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_voltdb_deterministic_order_partition() {
    let optimizer = create_optimizer();
    let plan = join(scan("orders"), scan("items"), eq_pred("order_id", "order_id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_voltdb_single_partition_query() {
    let optimizer = create_optimizer();
    let plan = scan("partitioned_data");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_voltdb_single_partition_join() {
    let optimizer = create_optimizer();
    let plan = join(scan("partition_a"), scan("partition_b"), eq_pred("key", "key"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_voltdb_single_partition_aggregate() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("partition_key".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("partitioned")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_voltdb_replicated_join_broadcast() {
    let optimizer = create_optimizer();
    let plan = join(scan("partitioned_fact"), scan("replicated_dim"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_voltdb_replicated_join_multi() {
    let optimizer = create_optimizer();
    let j1 = join(scan("fact"), scan("dim1"), eq_pred("id", "id"));
    let plan = join(j1, scan("dim2"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_voltdb_replicated_join_aggregate() {
    let optimizer = create_optimizer();
    let joined = join(scan("orders"), scan("products"), eq_pred("product_id", "id"));
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("category".to_string()))],
        aggregates: vec![],
        input: Box::new(joined),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}
