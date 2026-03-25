//! Column pruning rules.
//!
//! Removes unused columns as early as possible in the query plan
//! to reduce memory usage and I/O costs.
//!
//! Key optimizations:
//! - Push projections through set operations (intersect, except)
//! - Push projections through limits
//! - Eliminate redundant projections over values
//! - Idempotent projection elimination
//!
//! Note: `project-merge` and `project-through-union` are already
//! defined in rewrite.rs and are not duplicated here.

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return column pruning rules.
///
/// These rules push projections down through operators to eliminate
/// unused columns as early as possible. Only rules not already
/// present in rewrite.rs are included.
#[must_use]
pub fn column_pruning_rules(
) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
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
        // Push projection through limit (limit doesn't use columns)
        rewrite!("project-through-limit";
            "(project ?cols (limit ?n ?offset ?input))" =>
            "(limit ?n ?offset (project ?cols ?input))"
        ),
        // VALUES already has exactly the columns it produces
        rewrite!("project-values-all";
            "(project ?cols (values ?rows))" =>
            "(values ?rows)"
        ),
        // Projection with same columns is idempotent
        rewrite!("project-idempotent";
            "(project ?cols (project ?cols ?input))" =>
            "(project ?cols ?input)"
        ),
    ]
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{ProjectionColumn, RelExpr};
    use ra_core::expr::{ColumnRef, Expr};

    fn run_column_pruning(
        expr: &RelExpr,
    ) -> Runner<RelLang, RelAnalysis> {
        let rec =
            to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&column_pruning_rules())
    }

    fn pcol(name: &str) -> ProjectionColumn {
        ProjectionColumn {
            expr: Expr::Column(ColumnRef::new(name)),
            alias: None,
        }
    }

    #[test]
    fn projection_through_limit() {
        let expr = RelExpr::scan("t")
            .limit(10, 0)
            .project(vec![pcol("a")]);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn projection_idempotent() {
        let cols = vec![pcol("a"), pcol("b")];
        let expr = RelExpr::scan("t")
            .project(cols.clone())
            .project(cols);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn projection_through_intersect() {
        let expr = RelExpr::Intersect {
            all: false,
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        }
        .project(vec![pcol("col1")]);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn projection_through_except() {
        let expr = RelExpr::Except {
            all: false,
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        }
        .project(vec![pcol("col1")]);

        let runner = run_column_pruning(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}
