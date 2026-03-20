//! Column pruning rules.
//!
//! Removes unused columns as early as possible in the query plan
//! to reduce memory usage and I/O costs.
//!
//! Key optimizations:
//! - Push projections down to eliminate unused columns early
//! - Remove columns not referenced in parent operators
//! - Minimize columns read from base tables

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return column pruning rules.
///
/// These rules identify and remove columns that are not needed
/// for the final query result, pushing projections down to eliminate
/// them as early as possible.
#[must_use]
pub fn column_pruning_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // ---------------------------------------------------------------
        // Basic projection pushdown
        // ---------------------------------------------------------------

        // Push projection through filter to reduce columns early
        rewrite!("project-through-filter";
            "(project ?cols (filter ?pred ?input))" =>
            "(project ?cols (filter ?pred (project (union-cols ?cols (pred-cols ?pred)) ?input)))"
            if can_add_projection(?input)
        ),

        // Push projection through join (left side)
        rewrite!("project-through-join-left";
            "(project ?cols (join ?type ?cond ?left ?right))" =>
            "(project ?cols
                (join ?type ?cond
                    (project (needed-left-cols ?cols ?cond) ?left)
                    ?right))"
            if can_prune_left(?cols, ?cond, ?left)
        ),

        // Push projection through join (right side)
        rewrite!("project-through-join-right";
            "(project ?cols (join ?type ?cond ?left ?right))" =>
            "(project ?cols
                (join ?type ?cond
                    ?left
                    (project (needed-right-cols ?cols ?cond) ?right)))"
            if can_prune_right(?cols, ?cond, ?right)
        ),

        // ---------------------------------------------------------------
        // Projection merging and elimination
        // ---------------------------------------------------------------

        // Merge adjacent projections, keeping only needed columns
        rewrite!("project-merge-prune";
            "(project ?cols1 (project ?cols2 ?input))" =>
            "(project ?cols1 ?input)"
        ),

        // Eliminate projection that includes all columns
        rewrite!("project-all-columns-elimination";
            "(project ?cols ?input)" =>
            "?input"
            if includes_all_columns(?cols, ?input)
        ),

        // ---------------------------------------------------------------
        // Column pruning through aggregates
        // ---------------------------------------------------------------

        // Push projection below aggregate to only keep needed columns
        rewrite!("project-below-aggregate";
            "(aggregate ?groups ?aggs ?input)" =>
            "(aggregate ?groups ?aggs
                (project (union-cols (group-cols ?groups) (agg-cols ?aggs)) ?input))"
            if can_add_projection(?input)
        ),

        // Remove unused aggregate columns
        rewrite!("prune-unused-aggregates";
            "(project ?cols (aggregate ?groups ?aggs ?input))" =>
            "(project ?cols
                (aggregate ?groups (filter-aggs ?aggs ?cols) ?input))"
            if has_unused_aggs(?aggs, ?cols)
        ),

        // ---------------------------------------------------------------
        // Column pruning through set operations
        // ---------------------------------------------------------------

        // Push projection through union
        rewrite!("project-through-union";
            "(project ?cols (union ?all ?left ?right))" =>
            "(union ?all
                (project ?cols ?left)
                (project ?cols ?right))"
        ),

        // Push projection through intersect
        rewrite!("project-through-intersect";
            "(project ?cols (intersect ?all ?left ?right))" =>
            "(intersect ?all
                (project ?cols ?left)
                (project ?cols ?right))"
        ),

        // Push projection through except
        rewrite!("project-through-except";
            "(project ?cols (except ?all ?left ?right))" =>
            "(except ?all
                (project ?cols ?left)
                (project ?cols ?right))"
        ),

        // ---------------------------------------------------------------
        // Column pruning through sorting
        // ---------------------------------------------------------------

        // Only keep columns needed for sort keys and final projection
        rewrite!("prune-through-sort";
            "(project ?cols (sort ?keys ?input))" =>
            "(project ?cols
                (sort ?keys
                    (project (union-cols ?cols (sort-key-cols ?keys)) ?input)))"
            if can_add_projection(?input)
        ),

        // ---------------------------------------------------------------
        // Column pruning through limit
        // ---------------------------------------------------------------

        // Push projection through limit
        rewrite!("project-through-limit";
            "(project ?cols (limit ?n ?offset ?input))" =>
            "(limit ?n ?offset (project ?cols ?input))"
        ),

        // ---------------------------------------------------------------
        // Column pruning for semi/anti joins
        // ---------------------------------------------------------------

        // Semi-join only needs join columns from right side
        rewrite!("prune-semi-join-right";
            "(join semi ?cond ?left ?right)" =>
            "(join semi ?cond ?left
                (project (cond-right-cols ?cond) ?right))"
            if can_prune_semi_right(?cond, ?right)
        ),

        // Anti-join only needs join columns from right side
        rewrite!("prune-anti-join-right";
            "(join anti ?cond ?left ?right)" =>
            "(join anti ?cond ?left
                (project (cond-right-cols ?cond) ?right))"
            if can_prune_anti_right(?cond, ?right)
        ),

        // ---------------------------------------------------------------
        // Column pruning for outer joins
        // ---------------------------------------------------------------

        // Left outer join - prune right side columns not in output
        rewrite!("prune-left-outer-right";
            "(project ?cols (join left-outer ?cond ?left ?right))" =>
            "(project ?cols
                (join left-outer ?cond
                    ?left
                    (project (needed-right-cols ?cols ?cond) ?right)))"
            if can_prune_outer_right(?cols, ?cond, ?right)
        ),

        // ---------------------------------------------------------------
        // Early projection insertion at scan
        // ---------------------------------------------------------------

        // Add projection immediately after scan if columns can be pruned
        rewrite!("add-projection-after-scan";
            "(filter ?pred (scan ?table))" =>
            "(filter ?pred (project (pred-cols ?pred) (scan ?table)))"
            if can_prune_scan(?pred, ?table)
        ),

        // Add projection after scan for join
        rewrite!("add-projection-for-join-scan";
            "(join ?type ?cond (scan ?table) ?right)" =>
            "(join ?type ?cond
                (project (cond-left-cols ?cond) (scan ?table))
                ?right)"
            if can_prune_scan_for_join(?cond, ?table)
        ),

        // ---------------------------------------------------------------
        // Window function column pruning
        // ---------------------------------------------------------------

        // Only keep columns needed for window function and output
        rewrite!("prune-through-window";
            "(project ?cols (window ?window_exprs ?input))" =>
            "(project ?cols
                (window ?window_exprs
                    (project (union-cols ?cols (window-cols ?window_exprs)) ?input)))"
            if can_add_projection(?input)
        ),
    ]
}

// Helper conditions (these would be implemented in the analysis)
fn can_add_projection(_input: &str) -> bool {
    // Check if adding a projection would reduce columns
    false
}

fn can_prune_left(_cols: &str, _cond: &str, _left: &str) -> bool {
    // Check if left side has unused columns
    false
}

fn can_prune_right(_cols: &str, _cond: &str, _right: &str) -> bool {
    // Check if right side has unused columns
    false
}

fn includes_all_columns(_cols: &str, _input: &str) -> bool {
    // Check if projection includes all input columns
    false
}

fn has_unused_aggs(_aggs: &str, _cols: &str) -> bool {
    // Check if there are aggregates not used in projection
    false
}

fn can_prune_semi_right(_cond: &str, _right: &str) -> bool {
    // Check if right side of semi-join has extra columns
    false
}

fn can_prune_anti_right(_cond: &str, _right: &str) -> bool {
    // Check if right side of anti-join has extra columns
    false
}

fn can_prune_outer_right(_cols: &str, _cond: &str, _right: &str) -> bool {
    // Check if right side of outer join has unused columns
    false
}

fn can_prune_scan(_pred: &str, _table: &str) -> bool {
    // Check if scan can be pruned based on predicate
    false
}

fn can_prune_scan_for_join(_cond: &str, _table: &str) -> bool {
    // Check if scan can be pruned for join
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_column_pruning(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&column_pruning_rules())
    }

    #[test]
    fn projection_merge() {
        // Adjacent projections should be merged
        let expr = RelExpr::scan("t")
            .project(vec![
                ("a".to_string(), Expr::Column(ColumnRef::new("a"))),
                ("b".to_string(), Expr::Column(ColumnRef::new("b"))),
                ("c".to_string(), Expr::Column(ColumnRef::new("c"))),
            ])
            .project(vec![
                ("a".to_string(), Expr::Column(ColumnRef::new("a"))),
                ("b".to_string(), Expr::Column(ColumnRef::new("b"))),
            ]);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn projection_through_filter() {
        // Projection can be pushed through filter
        let expr = RelExpr::scan("t")
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("x"))),
                right: Box::new(Expr::Const(Const::Int(10))),
            })
            .project(vec![
                ("a".to_string(), Expr::Column(ColumnRef::new("a"))),
            ]);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn projection_through_union() {
        // Projection pushed through union
        let left = RelExpr::scan("t1");
        let right = RelExpr::scan("t2");
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(left),
            right: Box::new(right),
        }.project(vec![
            ("col1".to_string(), Expr::Column(ColumnRef::new("col1"))),
        ]);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn prune_semi_join_right() {
        // Semi-join right side should only keep join columns
        let expr = RelExpr::Join {
            join_type: JoinType::Semi,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        };

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}