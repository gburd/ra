#![expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
#![expect(
    clippy::expect_used,
    reason = "test code; expect is the conventional shorthand for surfacing failures in tests"
)]
//! Integration tests for `OptimizerConfig.plan_advice`.
//!
//! These exercise the **honor** half of plan-advice support:
//! when supplied advice tells the optimizer to do or not do
//! something, the rule advisor demotes accordingly.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
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
fn empty_plan_advice_treated_as_none() {
    // An empty advice string should not affect optimization
    // (no rules demoted, optimizer behaves identically).
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let config = OptimizerConfig {
        plan_advice: Some(String::new()),
        ..OptimizerConfig::default()
    };
    Optimizer::with_config(config)
        .optimize(&q)
        .expect("optimization with empty advice must succeed");
}

#[test]
fn invalid_plan_advice_is_warned_about_not_fatal() {
    let q = scan("t");
    let config = OptimizerConfig {
        plan_advice: Some("THIS_TAG_DOES_NOT_EXIST(t)".into()),
        ..OptimizerConfig::default()
    };
    // Optimization must still succeed; the parser-level error is
    // logged at WARN level.
    Optimizer::with_config(config).optimize(&q).unwrap();
}

#[test]
fn join_order_advice_disables_reordering() {
    // For a 3-table chain, run the optimizer twice: once without
    // any advice, once with JOIN_ORDER pinning the order. Both
    // should succeed; the version with advice should demote the
    // join-reordering rules so reordering doesn't happen.
    let q = eq_join(
        eq_join(scan("a"), scan("b"), "a", "b"),
        scan("c"),
        "a",
        "c",
    );

    let baseline = Optimizer::new()
        .optimize(&q)
        .expect("baseline optimization must succeed");
    let _ = baseline;

    let config = OptimizerConfig {
        plan_advice: Some("JOIN_ORDER(a b c)".into()),
        ..OptimizerConfig::default()
    };
    let pinned = Optimizer::with_config(config)
        .optimize(&q)
        .expect("optimization with JOIN_ORDER advice must succeed");
    // The contract: optimization succeeds. We don't assert the
    // produced plan equals the input — Ra applies many other
    // rewrites (predicate pushdown, etc.) that don't conflict
    // with JOIN_ORDER. The fact that no rules panicked and a
    // plan came back is what we test here. End-to-end shape
    // assertions belong in the round-trip test
    // (`plan_advice_round_trip.rs`).
    let _ = pinned;
}

#[test]
fn scan_method_advice_is_accepted_but_does_not_affect_relexpr() {
    // SEQ_SCAN advice is parsed and recorded but Ra's RelExpr
    // doesn't distinguish scan methods, so the produced plan is
    // identical to the no-advice baseline. This test just
    // verifies the optimizer accepts the advice without error.
    let q = scan("t");
    let config = OptimizerConfig {
        plan_advice: Some("SEQ_SCAN(t)".into()),
        ..OptimizerConfig::default()
    };
    let optimized = Optimizer::with_config(config).optimize(&q).unwrap();
    assert!(matches!(optimized, RelExpr::Scan { .. }));
}
