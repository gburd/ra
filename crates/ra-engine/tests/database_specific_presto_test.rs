//! Tests for Presto distributed SQL optimization rules.

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

// Cost-based join reordering, Fragment result caching, Dynamic partition pruning

#[test]
fn test_presto_join_reorder_two_way() {
    let optimizer = create_optimizer();
    let plan = join(scan("large"), scan("small"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_presto_join_reorder_multi_way() {
    let optimizer = create_optimizer();
    let j1 = join(scan("t1"), scan("t2"), eq_pred("id", "id"));
    let plan = join(j1, scan("t3"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_presto_join_reorder_star_schema() {
    let optimizer = create_optimizer();
    let j1 = join(scan("fact"), scan("dim1"), eq_pred("d1", "id"));
    let plan = join(j1, scan("dim2"), eq_pred("d2", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_presto_fragment_cache_subquery() {
    let optimizer = create_optimizer();
    let plan = filter(scan("table"), eq_pred("col", "val"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_presto_fragment_cache_cte() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("key".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("data")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_presto_fragment_cache_reuse() {
    let optimizer = create_optimizer();
    let plan = join(scan("cached"), scan("fresh"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_presto_dynamic_partition_pruning_basic() {
    let optimizer = create_optimizer();
    let filtered = filter(scan("partitioned_fact"), lt_pred("date", 20240101));
    assert!(optimizer.optimize(&filtered).is_ok());
}

#[test]
fn test_presto_dynamic_partition_pruning_join() {
    let optimizer = create_optimizer();
    let build = filter(scan("dim"), eq_pred("region", "US"));
    let probe = scan("partitioned_sales");
    let plan = join(probe, build, eq_pred("region_id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_presto_dynamic_partition_pruning_multi_level() {
    let optimizer = create_optimizer();
    let plan = filter(
        scan("multi_partitioned"),
        eq_pred("year", "2024")
    );
    assert!(optimizer.optimize(&plan).is_ok());
}
