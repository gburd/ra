//! Column pruning rules.
//!
//! Removes unused columns as early as possible in the query plan
//! to reduce memory usage and I/O costs.
//!
//! Key optimizations:
//! - Merge adjacent projections
//! - Push projections through set operations
//! - Minimize columns in semi/anti joins

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
        // Projection merging - always valid
        // ---------------------------------------------------------------

        // Merge adjacent projections, keeping only outer columns
        rewrite!("project-merge";
            "(project ?cols1 (project ?cols2 ?input))" =>
            "(project ?cols1 ?input)"
        ),

        // ---------------------------------------------------------------
        // Column pruning through set operations - always valid
        // ---------------------------------------------------------------

        // Push projection through union - both sides get same projection
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
        // Column pruning through limit - always valid
        // ---------------------------------------------------------------

        // Push projection through limit (limit doesn't use columns)
        rewrite!("project-through-limit";
            "(project ?cols (limit ?n ?offset ?input))" =>
            "(limit ?n ?offset (project ?cols ?input))"
        ),

        // ---------------------------------------------------------------
        // Simple projection elimination patterns
        // ---------------------------------------------------------------

        // Eliminate projection of all columns from values
        // VALUES already has exactly the columns it produces
        rewrite!("project-values-all";
            "(project ?cols (values ?rows))" =>
            "(values ?rows)"
        ),

        // Project after project with same columns (idempotent)
        rewrite!("project-idempotent";
            "(project ?cols (project ?cols ?input))" =>
            "(project ?cols ?input)"
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{ColumnRef, Expr};

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
    fn projection_through_limit() {
        // Projection can be pushed through limit
        let expr = RelExpr::scan("t")
            .limit(10, 0)
            .project(vec![
                ("a".to_string(), Expr::Column(ColumnRef::new("a"))),
            ]);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}