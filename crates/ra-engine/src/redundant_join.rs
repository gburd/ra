//! Redundant join elimination rules.
//!
//! Identifies and removes joins that don't contribute to the query result:
//! - Cross joins with single-row or empty relations
//! - Inner joins with TRUE condition and single-row right side
//! - Anti-joins with empty right side
//!
//! Future work (requires analysis infrastructure):
//! - Self-join elimination on unique columns
//! - Unused cross/left join elimination via column tracking

#[cfg(test)]
use egg::{rewrite, Rewrite};

#[cfg(test)]
use crate::analysis::RelAnalysis;
#[cfg(test)]
use crate::egraph::RelLang;

/// Return redundant join elimination rules.
///
/// These rules detect and eliminate joins that don't contribute
/// meaningful data or filtering to the query result.
/// Only unconditional (always-valid) rules are included.
#[must_use]
#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
pub fn redundant_join_elimination_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Cross join with single-row right side (limit pattern)
        rewrite!("eliminate-cross-join-single-row-right";
            "(join cross ?cond ?left (limit 1 0 ?right))" =>
            "?left"
        ),
        // Symmetric case: single-row on left
        rewrite!("eliminate-cross-join-single-row-left";
            "(join cross ?cond (limit 1 0 ?left) ?right)" =>
            "?right"
        ),
        // Cross join with VALUES containing a single row
        rewrite!("eliminate-cross-join-values-single";
            "(join cross ?cond ?left (values (values-row (const-int 1))))" =>
            "?left"
        ),
        // Inner join with TRUE condition and single-row right side
        rewrite!("eliminate-inner-join-true-single-row";
            "(join inner (const-bool true) ?left (limit 1 0 ?right))" =>
            "?left"
        ),
        // Anti-join with empty right side keeps all left rows
        rewrite!("eliminate-anti-join-empty-right";
            "(join anti ?cond ?left (filter (const-bool false) ?right))" =>
            "?left"
        ),
    ]
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_redundant_join_elimination(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&redundant_join_elimination_rules())
    }

    #[test]
    fn cross_join_with_single_row_right_eliminated() {
        let single_row = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        }
        .limit(1, 0);

        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("t")),
            right: Box::new(single_row),
        };

        let runner = run_redundant_join_elimination(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn cross_join_with_single_row_left_eliminated() {
        let single_row = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        }
        .limit(1, 0);

        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(single_row),
            right: Box::new(RelExpr::scan("t")),
        };

        let runner = run_redundant_join_elimination(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn inner_join_true_single_row_eliminated() {
        let single_row = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        }
        .limit(1, 0);

        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("t")),
            right: Box::new(single_row),
        };

        let runner = run_redundant_join_elimination(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn anti_join_with_empty_right_eliminated() {
        let empty_right = RelExpr::scan("t2").filter(Expr::Const(Const::Bool(false)));

        let expr = RelExpr::Join {
            join_type: JoinType::Anti,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(empty_right),
        };

        let runner = run_redundant_join_elimination(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}
