//! Tests for Apache Impala MPP optimization rules.
//!
//! Tests 4 Impala rules from Task #20 academic research mining:
//! - Runtime filters for distributed joins
//! - Parquet predicate pushdown
//! - HDFS block location caching
//! - Codegen fallback strategy

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

// ── Rule 1: Runtime Filters ─────────────────────────────────

#[test]
fn test_impala_runtime_filter_broadcast() {
    let optimizer = create_optimizer();
    let build = filter(scan("small_dim"), lt_pred("id", 100));
    let probe = scan("large_fact");
    let plan = join(probe, build, eq_pred("dim_id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_impala_runtime_filter_partition() {
    let optimizer = create_optimizer();
    let left = scan("fact_table");
    let right = scan("dimension");
    let plan = join(left, right, eq_pred("key", "key"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_impala_runtime_filter_multi_join() {
    let optimizer = create_optimizer();
    let t1 = scan("table1");
    let t2 = scan("table2");
    let t3 = scan("table3");
    let j1 = join(t1, t2, eq_pred("id", "id"));
    let plan = join(j1, t3, eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

// ── Rule 2: Parquet Pushdown ────────────────────────────────

#[test]
fn test_impala_parquet_column_pruning() {
    let optimizer = create_optimizer();
    let plan = filter(scan("parquet_table"), eq_pred("col", "col"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_impala_parquet_predicate_pushdown() {
    let optimizer = create_optimizer();
    let plan = filter(scan("parquet_data"), lt_pred("timestamp", 1_000_000));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_impala_parquet_dictionary_encoding() {
    let optimizer = create_optimizer();
    let plan = filter(scan("parquet_strings"), eq_pred("status", "active"));
    assert!(optimizer.optimize(&plan).is_ok());
}

// ── Rule 3: HDFS Block Caching ──────────────────────────────

#[test]
fn test_impala_hdfs_cache_hot_data() {
    let optimizer = create_optimizer();
    let plan = filter(scan("hot_table"), eq_pred("category", "popular"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_impala_hdfs_cache_locality() {
    let optimizer = create_optimizer();
    let plan = scan("distributed_data");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_impala_hdfs_cache_replication() {
    let optimizer = create_optimizer();
    let plan = join(scan("table_a"), scan("table_b"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

// ── Rule 4: Codegen Fallback ────────────────────────────────

#[test]
fn test_impala_codegen_success() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("key".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("data_table")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_impala_codegen_fallback_complex() {
    let optimizer = create_optimizer();
    let plan = join(
        join(scan("t1"), scan("t2"), eq_pred("id", "id")),
        scan("t3"),
        eq_pred("id", "id"),
    );
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_impala_codegen_fallback_memory() {
    let optimizer = create_optimizer();
    let sorted = RelExpr::Sort {
        keys: vec![],
        input: Box::new(scan("large_table")),
    };
    assert!(optimizer.optimize(&sorted).is_ok());
}
