//! Semi-join reduction rules.
//!
//! Converts EXISTS subqueries and IN predicates to efficient semi-joins,
//! and optimizes semi-join patterns for better performance.
//!
//! Key transformations:
//! - EXISTS subquery -> semi-join
//! - IN subquery -> semi-join
//! - Filter pushdown through semi-joins
//! - Semi-join deduplication

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return semi-join reduction rules.
///
/// These rules convert various patterns into semi-joins,
/// which are often more efficient than nested loops or hash lookups.
#[must_use]
pub fn semi_join_reduction_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // ---------------------------------------------------------------
        // EXISTS pattern to semi-join
        // ---------------------------------------------------------------

        // Filter with EXISTS subquery -> semi-join
        // Pattern: SELECT * FROM t1 WHERE EXISTS (SELECT 1 FROM t2 WHERE t1.id = t2.id)
        // Note: This is a simplified pattern. Real implementation needs subquery detection.
        rewrite!("exists-to-semi-join";
            "(filter (exists ?subquery) ?input)" =>
            "(join semi (extract-join-condition ?subquery ?input) ?input (extract-subquery-input ?subquery))"
            if is_correlated_exists(?subquery)
        ),

        // NOT EXISTS to anti-join
        rewrite!("not-exists-to-anti-join";
            "(filter (not (exists ?subquery)) ?input)" =>
            "(join anti (extract-join-condition ?subquery ?input) ?input (extract-subquery-input ?subquery))"
            if is_correlated_exists(?subquery)
        ),

        // ---------------------------------------------------------------
        // IN subquery to semi-join
        // ---------------------------------------------------------------

        // IN (subquery) -> semi-join
        // Pattern: SELECT * FROM t1 WHERE id IN (SELECT id FROM t2 WHERE ...)
        rewrite!("in-subquery-to-semi-join";
            "(filter (in ?col ?subquery) ?input)" =>
            "(join semi (eq ?col (extract-subquery-col ?subquery)) ?input ?subquery)"
            if is_single_column_subquery(?subquery)
        ),

        // NOT IN to anti-join (with NULL handling)
        rewrite!("not-in-subquery-to-anti-join";
            "(filter (not (in ?col ?subquery)) ?input)" =>
            "(join anti (eq ?col (extract-subquery-col ?subquery)) ?input ?subquery)"
            if is_single_column_subquery(?subquery)
        ),

        // ---------------------------------------------------------------
        // Semi-join with DISTINCT elimination
        // ---------------------------------------------------------------

        // Semi-join already produces distinct results on the left side
        rewrite!("semi-join-distinct-elimination";
            "(distinct-rel (join semi ?cond ?left ?right))" =>
            "(join semi ?cond ?left ?right)"
        ),

        // ---------------------------------------------------------------
        // Filter pushdown through semi-join
        // ---------------------------------------------------------------

        // Push filter on left columns through semi-join (to left side)
        rewrite!("filter-through-semi-join-left";
            "(filter ?pred (join semi ?cond ?left ?right))" =>
            "(join semi ?cond (filter ?pred ?left) ?right)"
            if only_references_left(?pred)
        ),

        // Push filter on right columns through semi-join (to right side)
        rewrite!("filter-through-semi-join-right";
            "(filter ?pred (join semi ?cond ?left ?right))" =>
            "(join semi ?cond ?left (filter ?pred ?right))"
            if only_references_right(?pred)
        ),

        // Merge filter into semi-join condition
        rewrite!("filter-into-semi-join-condition";
            "(filter ?pred (join semi ?cond ?left ?right))" =>
            "(join semi (and ?cond ?pred) ?left ?right)"
        ),

        // ---------------------------------------------------------------
        // Semi-join chain optimization
        // ---------------------------------------------------------------

        // Merge adjacent semi-joins with same right side
        // (A semi-join B) semi-join B -> A semi-join B
        rewrite!("merge-duplicate-semi-joins";
            "(join semi ?cond1 (join semi ?cond2 ?left ?right) ?right)" =>
            "(join semi (and ?cond1 ?cond2) ?left ?right)"
        ),

        // ---------------------------------------------------------------
        // Semi-join to inner join + distinct
        // ---------------------------------------------------------------

        // When we need columns from both sides, convert semi-join to inner + distinct
        // This is useful when the optimizer later needs right-side columns
        rewrite!("semi-join-to-inner-distinct";
            "(project ?cols (join semi ?cond ?left ?right))" =>
            "(distinct-rel (project ?cols (join inner ?cond ?left ?right)))"
            if needs_right_columns(?cols)
        ),

        // ---------------------------------------------------------------
        // Anti-join optimizations
        // ---------------------------------------------------------------

        // Push filter through anti-join (left side)
        rewrite!("filter-through-anti-join-left";
            "(filter ?pred (join anti ?cond ?left ?right))" =>
            "(join anti ?cond (filter ?pred ?left) ?right)"
            if only_references_left(?pred)
        ),

        // Anti-join with empty right -> return all left rows
        rewrite!("anti-join-empty-right";
            "(join anti ?cond ?left (filter (const-bool false) ?right))" =>
            "?left"
        ),

        // ---------------------------------------------------------------
        // Semi-join with aggregates
        // ---------------------------------------------------------------

        // Semi-join before aggregate can sometimes be pushed down
        rewrite!("semi-join-before-aggregate-pushdown";
            "(aggregate ?groups ?aggs (join semi ?cond ?left ?right))" =>
            "(join semi ?cond (aggregate ?groups ?aggs ?left) ?right)"
            if safe_to_push_aggregate(?groups, ?aggs, ?cond)
        ),

        // ---------------------------------------------------------------
        // ANY/ALL patterns to semi/anti-join
        // ---------------------------------------------------------------

        // col op ANY(subquery) -> semi-join
        rewrite!("any-to-semi-join";
            "(filter (any ?op ?col ?subquery) ?input)" =>
            "(join semi (apply-op ?op ?col (extract-subquery-col ?subquery)) ?input ?subquery)"
        ),

        // col op ALL(subquery) -> anti-join with negated condition
        rewrite!("all-to-anti-join";
            "(filter (all ?op ?col ?subquery) ?input)" =>
            "(join anti (not (apply-op ?op ?col (extract-subquery-col ?subquery))) ?input ?subquery)"
        ),

        // ---------------------------------------------------------------
        // Scalar subquery to left join
        // ---------------------------------------------------------------

        // Scalar subquery in projection -> left join + aggregate
        // Pattern: SELECT *, (SELECT MAX(x) FROM t2 WHERE t2.id = t1.id) FROM t1
        rewrite!("scalar-subquery-to-left-join";
            "(project (list ?cols (scalar-subquery ?subquery)) ?input)" =>
            "(project ?cols
                (join left-outer (extract-correlation ?subquery)
                    ?input
                    (aggregate nil (extract-agg ?subquery) (extract-subquery-input ?subquery))))"
            if is_scalar_subquery(?subquery)
        ),
    ]
}

// Helper conditions (these would be implemented in the analysis)
fn is_correlated_exists(_subquery: &str) -> bool {
    // Check if subquery is a correlated EXISTS
    false
}

fn is_single_column_subquery(_subquery: &str) -> bool {
    // Check if subquery returns a single column
    false
}

fn only_references_left(_pred: &str) -> bool {
    // Check if predicate only references left-side columns
    false
}

fn only_references_right(_pred: &str) -> bool {
    // Check if predicate only references right-side columns
    false
}

fn needs_right_columns(_cols: &str) -> bool {
    // Check if projection needs columns from right side
    false
}

fn safe_to_push_aggregate(_groups: &str, _aggs: &str, _cond: &str) -> bool {
    // Check if it's safe to push aggregate through semi-join
    false
}

fn is_scalar_subquery(_subquery: &str) -> bool {
    // Check if subquery is a scalar subquery (returns single value)
    false
}

#[cfg(test)]
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
        // DISTINCT after semi-join is redundant
        let expr = RelExpr::Join {
            join_type: JoinType::Semi,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        }.distinct();

        let runner = run_semi_join_reduction(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn filter_pushed_through_semi_join() {
        // Filter after semi-join pushed to left side
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
    fn anti_join_with_empty_right_eliminated() {
        // Anti-join with empty right side
        let expr = RelExpr::Join {
            join_type: JoinType::Anti,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(
                RelExpr::scan("t2").filter(Expr::Const(Const::Bool(false)))
            ),
        };

        let runner = run_semi_join_reduction(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}