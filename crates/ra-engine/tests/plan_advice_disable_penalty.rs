#![expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
//! Test that `OptimizerConfig.plan_advice` triggers the
//! `Cost::DISABLE_PENALTY` when supplied advice can't be honored
//! by the produced plan.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::cost::Cost;
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
fn complying_join_order_advice_does_not_trigger_penalty() {
    // Plan: ((a JOIN b) JOIN c) — outer-deep order [a, b, c].
    // Advice agrees with that order, so no FAILED bit is set.
    let q = eq_join(
        eq_join(scan("a"), scan("b"), "a", "b"),
        scan("c"),
        "a",
        "c",
    );
    let config = OptimizerConfig {
        plan_advice: Some("JOIN_ORDER(a b c)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config).optimize_bounded(&q).unwrap();
    assert!(
        result.cost < Cost::DISABLE_PENALTY,
        "complying advice should not trigger penalty; got cost={}",
        result.cost,
    );
}

#[test]
fn violating_join_order_advice_triggers_penalty() {
    // Same plan, but advice asks for the reverse order. The
    // honor pass demotes join-reordering rules; the only legal
    // ordering for this query (given filter pushdown etc.) is
    // [a, b, c]. validate_advice flags FAILED, the penalty
    // applies.
    let q = eq_join(
        eq_join(scan("a"), scan("b"), "a", "b"),
        scan("c"),
        "a",
        "c",
    );
    let config = OptimizerConfig {
        plan_advice: Some("JOIN_ORDER(c b a)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config).optimize_bounded(&q).unwrap();
    assert!(
        result.cost >= Cost::DISABLE_PENALTY,
        "violating advice should add at least one DISABLE_PENALTY; \
         got cost={}",
        result.cost,
    );
}

#[test]
fn empty_advice_does_not_trigger_penalty() {
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let config = OptimizerConfig {
        plan_advice: Some(String::new()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config).optimize_bounded(&q).unwrap();
    assert!(result.cost < Cost::DISABLE_PENALTY);
}

#[test]
fn no_advice_does_not_trigger_penalty() {
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let result = Optimizer::new().optimize_bounded(&q).unwrap();
    assert!(result.cost < Cost::DISABLE_PENALTY);
}
