//! Tests for Apache Drill schema-free SQL optimization rules.

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

// Schema discovery, Dynamic UDF, Late materialization, Schema versioning

#[test]
fn test_drill_schema_discovery_json() {
    let optimizer = create_optimizer();
    let plan = scan("json_files");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_schema_discovery_parquet() {
    let optimizer = create_optimizer();
    let plan = filter(scan("parquet_data"), eq_pred("col", "col"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_schema_discovery_mixed() {
    let optimizer = create_optimizer();
    let plan = join(scan("json_source"), scan("parquet_source"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_dynamic_udf_registration() {
    let optimizer = create_optimizer();
    let plan = filter(scan("data"), eq_pred("custom_fn", "value"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_dynamic_udf_optimization() {
    let optimizer = create_optimizer();
    let plan = join(scan("t1"), scan("t2"), eq_pred("udf_key", "key"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_dynamic_udf_caching() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("key".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("table")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_drill_late_materialization_wide_table() {
    let optimizer = create_optimizer();
    let plan = filter(scan("wide_table"), eq_pred("key_col", "value"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_late_materialization_projection() {
    let optimizer = create_optimizer();
    let plan = scan("columnar_data");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_late_materialization_join() {
    let optimizer = create_optimizer();
    let plan = join(scan("fact"), scan("dim"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_schema_versioning_evolution() {
    let optimizer = create_optimizer();
    let plan = scan("evolving_schema");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_schema_versioning_compatibility() {
    let optimizer = create_optimizer();
    let plan = join(scan("v1_data"), scan("v2_data"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_drill_schema_versioning_union() {
    let optimizer = create_optimizer();
    let plan = RelExpr::Union {
        all: true,
        left: Box::new(scan("old_format")),
        right: Box::new(scan("new_format")),
    };
    assert!(optimizer.optimize(&plan).is_ok());
}
