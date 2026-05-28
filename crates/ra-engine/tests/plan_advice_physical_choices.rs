#![expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
#![expect(
    clippy::panic,
    reason = "test code; panic is how we report a failed expectation"
)]
//! Integration test: `OptimizerConfig.plan_advice` populates
//! `OptimizationResult.physical_choices` so downstream consumers
//! (e.g. PG plan-builders) can read scan / join / parallelism
//! preferences without re-parsing the advice string.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_engine::plan_advice_physical::{
    JoinInnerStrategy, ParallelStrategy, ScanStrategy,
};
use ra_engine::{Optimizer, OptimizerConfig};

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan { table: name.into(), alias: None }
}

fn eq_join(left: RelExpr, right: RelExpr, l: &str, r: &str) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(l, "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified(r, "id"))),
        },
        left: Box::new(left),
        right: Box::new(right),
    }
}

#[test]
fn no_advice_yields_cost_driven_defaults() {
    // After RFC 0087 the optimizer always populates
    // physical_choices, falling back to cost-driven defaults
    // when supplied advice is absent. For a single-table scan
    // with no stats, the default is SeqScan.
    let q = scan("t");
    let result = Optimizer::new().optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "no advice + no stats → cost-driven default of SeqScan",
    );
    assert_eq!(
        result.physical_choices.join_for("t"),
        None,
        "single-table scan has no joins to constrain",
    );
}

#[test]
fn seq_scan_advice_populates_scan_strategy() {
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let config = OptimizerConfig {
        plan_advice: Some("SEQ_SCAN(a b)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config)
        .optimize_bounded(&q)
        .unwrap();
    assert_eq!(result.physical_choices.scan_for("a"), Some(&ScanStrategy::Seq));
    assert_eq!(result.physical_choices.scan_for("b"), Some(&ScanStrategy::Seq));
}

#[test]
fn index_scan_advice_carries_index_name() {
    let q = scan("orders");
    let config = OptimizerConfig {
        plan_advice: Some("INDEX_SCAN(orders orders_pkey)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config)
        .optimize_bounded(&q)
        .unwrap();
    match result.physical_choices.scan_for("orders") {
        Some(ScanStrategy::Index { schema, name }) => {
            assert_eq!(schema, &None);
            assert_eq!(name, "orders_pkey");
        }
        other => panic!("expected Index strategy, got {other:?}"),
    }
}

#[test]
fn hash_join_advice_populates_join_strategy() {
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let config = OptimizerConfig {
        plan_advice: Some("HASH_JOIN(b)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config)
        .optimize_bounded(&q)
        .unwrap();
    assert_eq!(
        result.physical_choices.join_for("b"),
        Some(&JoinInnerStrategy::Hash),
    );
}

#[test]
fn no_gather_advice_populates_parallel_strategy() {
    let q = scan("t");
    let config = OptimizerConfig {
        plan_advice: Some("NO_GATHER(t)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config)
        .optimize_bounded(&q)
        .unwrap();
    assert_eq!(
        result.physical_choices.parallel_for("t"),
        Some(ParallelStrategy::NoGather),
    );
}

#[test]
fn mixed_advice_populates_each_category() {
    let q = eq_join(
        eq_join(scan("a"), scan("b"), "a", "b"),
        scan("c"),
        "a",
        "c",
    );
    let advice = "SEQ_SCAN(a) INDEX_SCAN(b b_idx) HASH_JOIN(c) NO_GATHER(c)";
    let config = OptimizerConfig {
        plan_advice: Some(advice.into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config)
        .optimize_bounded(&q)
        .unwrap();
    assert_eq!(
        result.physical_choices.scan_for("a"),
        Some(&ScanStrategy::Seq),
    );
    assert!(matches!(
        result.physical_choices.scan_for("b"),
        Some(ScanStrategy::Index { .. }),
    ));
    assert_eq!(
        result.physical_choices.join_for("c"),
        Some(&JoinInnerStrategy::Hash),
    );
    assert_eq!(
        result.physical_choices.parallel_for("c"),
        Some(ParallelStrategy::NoGather),
    );
}

#[test]
fn join_order_does_not_supply_scan_or_join_strategies() {
    // JOIN_ORDER constrains the rule advisor, not the physical
    // choice map. With no further advice and no stats, the
    // map is populated cost-driven: SeqScan for both relations,
    // Hash for the inner-side equi-join.
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let config = OptimizerConfig {
        plan_advice: Some("JOIN_ORDER(a b)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config)
        .optimize_bounded(&q)
        .unwrap();
    assert_eq!(result.physical_choices.scan_for("a"), Some(&ScanStrategy::Seq));
    assert_eq!(result.physical_choices.scan_for("b"), Some(&ScanStrategy::Seq));
    assert_eq!(
        result.physical_choices.join_for("b"),
        Some(&ra_engine::plan_advice_physical::JoinInnerStrategy::Hash),
        "no JOIN_METHOD advice + equi-join → cost-driven Hash",
    );
}
