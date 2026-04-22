//! Integration tests for materialized view matching and rewriting.

mod helpers;

use helpers::*;
use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::mv_matching::{
    match_query_with_mv, view_benefit, MatchType, MaterializedViewInfo, MvCatalog,
};
use ra_engine::mv_rewrite::{mv_rewrite_rules, mv_scan_cost_factor};

fn agg_col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn agg_int(v: i64) -> Expr {
    Expr::Const(Const::Int(v))
}

fn agg_eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn agg_gt(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn customer_order_summary(extra_filter: Option<Expr>) -> RelExpr {
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: agg_eq(agg_col("c_custkey"), agg_col("o_custkey")),
        left: Box::new(RelExpr::scan("customer")),
        right: Box::new(RelExpr::scan("orders")),
    };
    let input = if let Some(pred) = extra_filter {
        RelExpr::Filter {
            predicate: pred,
            input: Box::new(join),
        }
    } else {
        join
    };
    RelExpr::Aggregate {
        group_by: vec![agg_col("c_custkey")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(agg_col("o_totalprice")),
            distinct: false,
            alias: Some("total".to_string()),
        }],
        input: Box::new(input),
    }
}

fn make_mv(extra_filter: Option<Expr>, row_count: u64) -> MaterializedViewInfo {
    MaterializedViewInfo {
        name: "customer_order_summary".to_string(),
        definition: customer_order_summary(extra_filter),
        base_tables: vec!["customer".to_string(), "orders".to_string()],
        row_count,
        is_incremental: false,
    }
}

#[test]
fn exact_match_same_query_and_mv() {
    let query = customer_order_summary(None);
    let mv = make_mv(None, 1000);
    let result = match_query_with_mv(&query, &mv);
    assert!(result.is_some());
    let m = result.unwrap();
    assert_eq!(m.match_type, MatchType::Exact);
    assert!(m.compensation.is_none());
    assert_eq!(m.mv_row_count, 1000);
}

#[test]
fn exact_match_via_catalog() {
    let mut catalog = MvCatalog::new();
    catalog.add_view(make_mv(None, 500));
    let query = customer_order_summary(None);
    let best = catalog.best_match(&query);
    assert!(best.is_some());
    assert_eq!(best.unwrap().match_type, MatchType::Exact);
}

#[test]
fn view_subsumes_query_extra_filter() {
    let query = customer_order_summary(Some(agg_gt(agg_col("o_totalprice"), agg_int(100))));
    let mv = make_mv(None, 2000);
    let result = match_query_with_mv(&query, &mv);
    assert!(result.is_some());
    let m = result.unwrap();
    assert_eq!(m.match_type, MatchType::ViewSubsumes);
    assert!(m.compensation.is_some());
}

#[test]
fn query_subsumes_view_mv_has_extra_filter() {
    let query = customer_order_summary(None);
    let mv = make_mv(Some(agg_gt(agg_col("o_totalprice"), agg_int(100))), 300);
    let result = match_query_with_mv(&query, &mv);
    assert!(result.is_some());
    assert_eq!(result.unwrap().match_type, MatchType::QuerySubsumes);
}

#[test]
fn no_match_different_tables() {
    let query = RelExpr::Aggregate {
        group_by: vec![agg_col("id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: None,
            distinct: false,
            alias: None,
        }],
        input: Box::new(RelExpr::scan("products")),
    };
    let mv = make_mv(None, 1000);
    assert!(match_query_with_mv(&query, &mv).is_none());
}

#[test]
fn no_match_different_aggregation() {
    let query = RelExpr::Aggregate {
        group_by: vec![agg_col("c_custkey")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: None,
            distinct: false,
            alias: None,
        }],
        input: Box::new(RelExpr::Join {
            join_type: JoinType::Inner,
            condition: agg_eq(agg_col("c_custkey"), agg_col("o_custkey")),
            left: Box::new(RelExpr::scan("customer")),
            right: Box::new(RelExpr::scan("orders")),
        }),
    };
    let mv = make_mv(None, 1000);
    assert!(match_query_with_mv(&query, &mv).is_none());
}

#[test]
fn view_benefit_positive_for_cheaper_mv() {
    let benefit = view_benefit(1000.0, 100.0, 20.0);
    assert!(benefit > 0.0);
    assert!((benefit - 880.0).abs() < f64::EPSILON);
}

#[test]
fn view_benefit_negative_for_expensive_mv() {
    let benefit = view_benefit(100.0, 200.0, 0.0);
    assert!(benefit < 0.0);
}

#[test]
fn mv_scan_cost_factor_is_subunit() {
    let f = mv_scan_cost_factor();
    assert!(f > 0.0 && f < 1.0);
}

#[test]
fn catalog_prefers_exact_match_over_partial() {
    let mut catalog = MvCatalog::new();
    catalog.add_view(make_mv(None, 5000));
    catalog.add_view(MaterializedViewInfo {
        name: "filtered_summary".to_string(),
        definition: customer_order_summary(Some(agg_gt(agg_col("o_totalprice"), agg_int(0)))),
        base_tables: vec!["customer".to_string(), "orders".to_string()],
        row_count: 100,
        is_incremental: false,
    });
    let query = customer_order_summary(None);
    let best = catalog.best_match(&query);
    assert!(best.is_some());
    let m = best.unwrap();
    assert_eq!(m.match_type, MatchType::Exact);
    assert_eq!(m.mv_name, "customer_order_summary");
}

#[test]
fn catalog_prefers_smaller_among_same_match_type() {
    let mut catalog = MvCatalog::new();
    catalog.add_view(MaterializedViewInfo {
        name: "big_summary".to_string(),
        definition: customer_order_summary(None),
        base_tables: vec!["customer".to_string(), "orders".to_string()],
        row_count: 10000,
        is_incremental: false,
    });
    catalog.add_view(MaterializedViewInfo {
        name: "small_summary".to_string(),
        definition: customer_order_summary(None),
        base_tables: vec!["customer".to_string(), "orders".to_string()],
        row_count: 500,
        is_incremental: false,
    });
    let query = customer_order_summary(None);
    let best = catalog.best_match(&query);
    assert!(best.is_some());
    assert_eq!(best.unwrap().mv_name, "small_summary");
}

#[test]
fn empty_catalog_returns_none() {
    let catalog = MvCatalog::new();
    let query = customer_order_summary(None);
    assert!(catalog.best_match(&query).is_none());
}

#[test]
fn mv_rewrite_rules_not_empty() {
    let catalog = MvCatalog::new();
    let rules = mv_rewrite_rules(&catalog);
    assert!(!rules.is_empty());
}

#[test]
fn optimizer_handles_mv_scan_roundtrip() {
    let mv_expr = RelExpr::MvScan {
        view_name: "test_view".to_string(),
        alias: None,
    };
    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&mv_expr);
    assert!(result.is_ok());
}

#[test]
fn optimizer_handles_filter_over_mv_scan() {
    let mv = RelExpr::MvScan {
        view_name: "revenue_summary".to_string(),
        alias: None,
    };
    let filtered = RelExpr::Filter {
        predicate: agg_gt(agg_col("amount"), agg_int(100)),
        input: Box::new(mv),
    };
    let optimizer = create_test_optimizer();
    let result = optimizer.optimize(&filtered);
    assert!(result.is_ok());
}
