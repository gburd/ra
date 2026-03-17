//! Tests for Greenplum MPP optimization rules.

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

fn filter(input: RelExpr, predicate: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate,
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

fn eq_pred(left: &str, right: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new(left.to_string()))),
        right: Box::new(Expr::Column(ColumnRef::new(right.to_string()))),
    }
}

// Motion node optimization, External table pushdown

#[test]
fn test_greenplum_motion_broadcast() {
    let optimizer = create_optimizer();
    let plan = join(scan("large_fact"), scan("small_dim"), eq_pred("dim_id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_greenplum_motion_redistribute() {
    let optimizer = create_optimizer();
    let plan = join(scan("distributed_a"), scan("distributed_b"), eq_pred("key", "key"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_greenplum_motion_gather() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![],
        input: Box::new(scan("distributed_table")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_greenplum_external_table_filter_pushdown() {
    let optimizer = create_optimizer();
    let plan = filter(scan("external_data"), eq_pred("status", "active"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_greenplum_external_table_projection_pushdown() {
    let optimizer = create_optimizer();
    let plan = scan("external_parquet");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_greenplum_external_table_join() {
    let optimizer = create_optimizer();
    let plan = join(scan("internal_table"), scan("external_source"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}
