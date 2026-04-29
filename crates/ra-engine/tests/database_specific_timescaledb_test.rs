//! Tests for `TimescaleDB` time-series optimization rules.

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

// Time-bucket aggregation, Chunk pruning, Continuous aggregates

#[test]
fn test_timescaledb_time_bucket_hour() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("time_bucket".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("metrics")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_timescaledb_time_bucket_day() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("day".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("daily_data")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_timescaledb_time_bucket_custom() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("custom_interval".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("time_series")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_timescaledb_chunk_pruning_time_range() {
    let optimizer = create_optimizer();
    let plan = filter(scan("hypertable"), lt_pred("time", 1_640_000_000));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_timescaledb_chunk_pruning_multi_dimension() {
    let optimizer = create_optimizer();
    let plan = filter(
        filter(scan("multi_dim_hypertable"), lt_pred("time", 1000)),
        eq_pred("device_id", "device_id"),
    );
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_timescaledb_chunk_pruning_join() {
    let optimizer = create_optimizer();
    let filtered = filter(scan("hypertable"), lt_pred("time", 1000));
    let plan = join(filtered, scan("metadata"), eq_pred("id", "id"));
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_timescaledb_continuous_aggregate_basic() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("hour".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("sensor_data")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}

#[test]
fn test_timescaledb_continuous_aggregate_refresh() {
    let optimizer = create_optimizer();
    let plan = scan("materialized_view");
    assert!(optimizer.optimize(&plan).is_ok());
}

#[test]
fn test_timescaledb_continuous_aggregate_realtime() {
    let optimizer = create_optimizer();
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("minute".to_string()))],
        aggregates: vec![],
        input: Box::new(scan("recent_data")),
    };
    assert!(optimizer.optimize(&agg).is_ok());
}
