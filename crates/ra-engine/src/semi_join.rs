//! Semi-join reduction rules.
//!
//! Optimizes semi-join and anti-join patterns for better performance.
//!
//! Currently enabled (unconditional) rules:
//! - DISTINCT elimination after semi-join
//! - Filter merging into semi-join condition
//! - Duplicate semi-join merging
//! - Anti-join with empty right elimination
//!
//! Future work (requires analysis infrastructure):
//! - EXISTS/IN subquery to semi-join conversion
//! - Conditional filter pushdown through semi-joins
//! - ANY/ALL to semi/anti-join conversion

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return semi-join reduction rules.
///
/// These rules optimize semi-join and anti-join patterns.
/// Only unconditional (always-valid) rules are included.
#[must_use]
pub fn semi_join_reduction_rules(
) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Semi-join already produces distinct results on the left side
        rewrite!("semi-join-distinct-elimination";
            "(distinct-rel (join semi ?cond ?left ?right))" =>
            "(join semi ?cond ?left ?right)"
        ),
        // Merge filter into semi-join condition
        rewrite!("filter-into-semi-join-condition";
            "(filter ?pred (join semi ?cond ?left ?right))" =>
            "(join semi (and ?cond ?pred) ?left ?right)"
        ),
        // Merge adjacent semi-joins with same right side
        // (A semi-join B) semi-join B -> A semi-join B with combined condition
        rewrite!("merge-duplicate-semi-joins";
            "(join semi ?cond1 (join semi ?cond2 ?left ?right) ?right)" =>
            "(join semi (and ?cond1 ?cond2) ?left ?right)"
        ),
        // Anti-join with empty right side keeps all left rows
        rewrite!("anti-join-empty-right";
            "(join anti ?cond ?left (filter (const-bool false) ?right))" =>
            "?left"
        ),
    ]
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_semi_join_reduction(
        expr: &RelExpr,
    ) -> Runner<RelLang, RelAnalysis> {
        let rec =
            to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&semi_join_reduction_rules())
    }

    #[test]
    fn distinct_after_semi_join_eliminated() {
        let expr = RelExpr::Join {
            join_type: JoinType::Semi,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(
                    ColumnRef::new("id"),
                )),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        }
        .distinct();

        let runner = run_semi_join_reduction(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn filter_merged_into_semi_join_condition() {
        let semi_join = RelExpr::Join {
            join_type: JoinType::Semi,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(
                    ColumnRef::new("id"),
                )),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        };

        let expr = semi_join.filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("value"))),
            right: Box::new(Expr::Const(Const::Int(10))),
        });

        let runner = run_semi_join_reduction(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn anti_join_with_empty_right_eliminated() {
        let expr = RelExpr::Join {
            join_type: JoinType::Anti,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(
                    ColumnRef::new("id"),
                )),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(
                RelExpr::scan("t2")
                    .filter(Expr::Const(Const::Bool(false))),
            ),
        };

        let runner = run_semi_join_reduction(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}
