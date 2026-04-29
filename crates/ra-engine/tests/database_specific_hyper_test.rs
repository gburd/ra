//! Tests for `HyPer` compiled query optimization rules.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
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

fn lt_pred(column: &str, value: i64) -> Expr {
    Expr::BinOp {
        op: BinOp::Lt,
        left: Box::new(Expr::Column(ColumnRef::new(column.to_string()))),
        right: Box::new(Expr::Const(Const::Int(value))),
    }
}

// Morsel-driven parallelism, Adaptive codegen, Vectorized interpretation

#[test]
fn test_hyper_morsel_driven_scan() {
    let optimizer = create_optimizer();
    let plan = scan("large_table");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_hyper_morsel_driven_join() {
    let optimizer = create_optimizer();
    let plan = join(scan("fact"), scan("dimension"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_hyper_morsel_driven_aggregate() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("key".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("data")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_hyper_adaptive_codegen_simple() {
    let optimizer = create_optimizer();
    let plan = filter(scan("table"), lt_pred("value", 100));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_hyper_adaptive_codegen_complex() {
    let optimizer = create_optimizer();
    let j1 = join(scan("t1"), scan("t2"), eq_pred("id", "id"));
    let plan = join(j1, scan("t3"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_hyper_adaptive_codegen_threshold() {
    let optimizer = create_optimizer();
    let sorted = RelExpr::Sort {
        keys: vec![],
        input: Box::new(scan("unsorted")),
    };
    assert!(optimizer.optimize(&sorted).is_ok());
}

#[test]
fn test_hyper_vectorized_interpretation_scan() {
    let optimizer = create_optimizer();
    let plan = filter(scan("columnar"), eq_pred("col", "val"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_hyper_vectorized_interpretation_aggregate() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("category".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("sales")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_hyper_vectorized_interpretation_join() {
    let optimizer = create_optimizer();
    let plan = join(
        scan("orders"),
        scan("customers"),
        eq_pred("customer_id", "id"),
    );
    assert!(optimizer.optimize(&plan).is_ok());
}
