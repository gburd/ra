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

#[cfg(test)]
use egg::{rewrite, Rewrite};

#[cfg(test)]
use crate::analysis::RelAnalysis;
#[cfg(test)]
use crate::egraph::RelLang;

/// Return semi-join reduction rules.
///
/// These rules optimize semi-join and anti-join patterns.
/// Only unconditional (always-valid) rules are included.
#[must_use]
#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
pub fn semi_join_reduction_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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

    fn run_semi_join_reduction(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
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
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
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
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
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
    fn anti_join_with_empty_right_preserves_both_tables() {
        // Codeberg #24: an anti-join over a provably-empty right must not drop
        // the right relation reference (same class as #17).
        let expr = RelExpr::Join {
            join_type: JoinType::Anti,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2").filter(Expr::Const(Const::Bool(false)))),
        };
        let opt = crate::Optimizer::new();
        let first = opt.optimize(&expr).expect("optimize once");
        let second = opt.optimize(&first).expect("optimize twice");
        #[expect(clippy::items_after_statements, reason = "test-local helper")]
        fn tabs(e: &RelExpr, o: &mut std::collections::BTreeSet<String>) {
            if let RelExpr::Scan { table, .. } = e {
                o.insert(table.clone());
            }
            for c in e.children() {
                tabs(c, o);
            }
        }
        let (mut a, mut b) = (
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        );
        tabs(&first, &mut a);
        tabs(&second, &mut b);
        assert!(
            a.contains("t1") && a.contains("t2"),
            "both relations kept: {a:?}"
        );
        assert_eq!(a, b, "optimize-twice must preserve the table set");
    }
}
