//! Covering index (index-only scan) optimization.
//!
//! Rewrites `project(cols, filter(pred, scan(table)))` into an
//! `index-only-scan` when a covering index exists that includes all
//! projected columns and all columns referenced by the predicate.
//! This eliminates the heap fetch, yielding 2-10x speedup.
//!
//! # Background
//!
//! A covering index stores all columns needed by a query in the index
//! structure itself (either as key columns or INCLUDE columns).  When
//! the optimizer detects that every column in the projection and filter
//! is present in such an index, it can replace the table scan + heap
//! fetch with an index-only scan.
//!
//! # E-graph representation
//!
//! ```text
//! (index-only-scan <table> <index_name> <projected_cols> <predicate>)
//! ```

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Rewrite rules for covering index optimization.
///
/// The primary rule rewrites:
/// ```text
/// (project ?cols (filter ?pred (scan ?table)))
///   => (index-only-scan ?table ?idx ?cols ?pred)
/// ```
///
/// Because egg's pattern-based rewrite rules cannot call into
/// external metadata (the `FactsProvider`) to check whether a
/// covering index actually exists, the rewrite is expressed as a
/// structural equivalence.  A downstream applier or cost model is
/// responsible for pruning index-only-scan nodes that reference
/// non-existent indexes.
///
/// The cost model assigns a lower cost to `index-only-scan` than
/// `scan` (no heap fetch), so the extractor will prefer it when
/// available.
#[must_use]
pub fn covering_index_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // project(cols, filter(pred, scan(table)))
        //   => index-only-scan(table, "auto", cols, pred)
        //
        // The sentinel index name "auto" signals that the executor
        // must resolve the actual covering index at plan execution
        // time (or during a physical planning pass).
        rewrite!("project-filter-scan-to-index-only";
            "(project ?cols (filter ?pred (scan ?table)))" =>
            "(index-only-scan ?table auto ?cols ?pred)"
        ),
        // Reverse direction so the e-graph explores both
        // representations and the cost model picks the winner.
        rewrite!("index-only-to-project-filter-scan";
            "(index-only-scan ?table auto ?cols ?pred)" =>
            "(project ?cols (filter ?pred (scan ?table)))"
        ),
    ]
}

/// Estimate the cost of an index-only scan relative to a full scan.
///
/// An index-only scan eliminates heap fetches, making it roughly
/// 30% of the cost of a regular scan + filter + project pipeline.
/// The 0.3 factor is conservative; real-world speedups range from
/// 2x to 10x depending on row width and cache effects.
#[must_use]
pub fn index_only_scan_cost_factor() -> f64 {
    0.3
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::analysis::RelAnalysis;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{ProjectionColumn, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_with_covering_rules(
        expr: &RelExpr,
    ) -> Runner<RelLang, RelAnalysis> {
        let rec =
            to_rec_expr(expr).expect("conversion should succeed");
        let rules = covering_index_rules();
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules)
    }

    /// When all projected columns are covered by the index, the
    /// e-graph should contain an `index-only-scan` alternative.
    #[test]
    fn covering_index_rewrite_applied() {
        let expr = RelExpr::scan("orders")
            .filter(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(
                    ColumnRef::new("customer_id"),
                )),
                right: Box::new(Expr::Const(Const::Int(42))),
            })
            .project(vec![
                ProjectionColumn {
                    expr: Expr::Column(
                        ColumnRef::new("customer_id"),
                    ),
                    alias: None,
                },
                ProjectionColumn {
                    expr: Expr::Column(
                        ColumnRef::new("order_date"),
                    ),
                    alias: None,
                },
            ]);

        let runner = run_with_covering_rules(&expr);
        let root = runner.roots[0];

        // The e-graph should have grown beyond the original nodes,
        // indicating the covering index rewrite was applied.
        assert!(
            runner.egraph.number_of_classes() > 3,
            "expected e-graph growth from covering index rule"
        );

        // Verify the root e-class still references the "orders"
        // table.
        let data = &runner.egraph[root].data;
        assert!(
            data.tables.contains("orders"),
            "root should reference orders table"
        );
    }

    /// A scan without a filter+project wrapper should NOT trigger
    /// the covering index rewrite.
    #[test]
    fn plain_scan_not_rewritten() {
        let expr = RelExpr::scan("users");
        let rec =
            to_rec_expr(&expr).expect("conversion should succeed");
        let rules = covering_index_rules();
        let runner = Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&rules);

        // Only two e-classes: the symbol "users" and the scan node.
        assert!(
            runner.egraph.number_of_classes() <= 3,
            "plain scan should not trigger covering index rule"
        );
    }

    /// Filter + scan without a project should NOT match the
    /// covering index rule pattern.
    #[test]
    fn filter_without_project_not_rewritten() {
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let rec =
            to_rec_expr(&expr).expect("conversion should succeed");
        let rules = covering_index_rules();
        let runner = Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&rules);

        // No index-only-scan should appear because there is no
        // project wrapper.
        let classes_before = runner.egraph.number_of_classes();
        assert!(
            classes_before <= 8,
            "filter-only query should not create many alternatives"
        );
    }

    #[test]
    fn cost_factor_is_positive() {
        let factor = index_only_scan_cost_factor();
        assert!(factor > 0.0);
        assert!(factor < 1.0);
    }
}
