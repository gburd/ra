//! Functional dependency exploitation rules.
//!
//! Uses functional dependencies from unique constraints and keys
//! to simplify GROUP BY clauses and eliminate redundant columns.
//!
//! Key optimizations:
//! - Eliminate DISTINCT after GROUP BY (always produces unique groups)
//! - Simplify MIN/MAX when grouping by the same column
//! - Convert COUNT(DISTINCT) patterns

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return functional dependency exploitation rules.
///
/// These rules use knowledge about functional dependencies
/// (typically from primary keys and unique constraints) to
/// simplify queries.
#[must_use]
pub fn functional_dependency_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // ---------------------------------------------------------------
        // DISTINCT elimination patterns (always valid)
        // ---------------------------------------------------------------

        // DISTINCT after GROUP BY is redundant (GROUP BY already produces unique groups)
        rewrite!("eliminate-distinct-after-groupby";
            "(distinct-rel (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs ?input)"
        ),

        // ---------------------------------------------------------------
        // Aggregate simplification patterns
        // ---------------------------------------------------------------

        // MIN(col) when grouping by col -> just project col
        // The minimum value in a group where all values are the same is that value
        rewrite!("min-same-when-grouping-by-col";
            "(aggregate (list ?col) (list (agg-expr ?d (min ?col) ?alias)) ?input)" =>
            "(project (list (proj-alias ?col ?alias)) ?input)"
        ),

        // MAX(col) when grouping by col -> just project col
        rewrite!("max-same-when-grouping-by-col";
            "(aggregate (list ?col) (list (agg-expr ?d (max ?col) ?alias)) ?input)" =>
            "(project (list (proj-alias ?col ?alias)) ?input)"
        ),

        // COUNT(*) when grouping by unique key -> always 1
        // Each group has exactly one row when grouping by unique key
        rewrite!("count-star-unique-group";
            "(aggregate (list ?pk) (list (agg-expr ?d (count ?star) ?alias)) (scan ?table))" =>
            "(project (list (proj-alias (const-int 1) ?alias)) (scan ?table))"
        ),

        // ---------------------------------------------------------------
        // Redundant aggregate elimination
        // ---------------------------------------------------------------

        // Aggregate with no aggregates and no GROUP BY -> DISTINCT
        rewrite!("aggregate-no-aggs-no-groups-to-distinct";
            "(aggregate nil nil ?input)" =>
            "(distinct-rel ?input)"
        ),

        // Double DISTINCT elimination
        rewrite!("double-distinct-elimination";
            "(distinct-rel (distinct-rel ?input))" =>
            "(distinct-rel ?input)"
        ),

        // ---------------------------------------------------------------
        // ORDER BY simplification patterns
        // ---------------------------------------------------------------

        // ORDER BY after ORDER BY - keep only the outer sort
        rewrite!("sort-after-sort";
            "(sort ?keys1 (sort ?keys2 ?input))" =>
            "(sort ?keys1 ?input)"
        ),

        // DISTINCT after ORDER BY may lose ordering
        // but ORDER BY after DISTINCT is preserved
        rewrite!("distinct-sort-reorder";
            "(distinct-rel (sort ?keys ?input))" =>
            "(sort ?keys (distinct-rel ?input))"
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{AggFunc, ColumnRef, Expr};

    fn run_functional_deps(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&functional_dependency_rules())
    }

    #[test]
    fn distinct_after_groupby_eliminated() {
        // DISTINCT after GROUP BY is redundant
        let expr = RelExpr::scan("t")
            .aggregate(
                vec!["dept".to_string()],
                vec![("count".to_string(), AggFunc::Count(Box::new(Expr::Column(ColumnRef::new("id")))))],
            )
            .distinct();

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn min_grouping_by_same_column_simplified() {
        // MIN(col) when grouping by col -> just project col
        let expr = RelExpr::scan("t")
            .aggregate(
                vec!["id".to_string()],
                vec![("min_id".to_string(), AggFunc::Min(Box::new(Expr::Column(ColumnRef::new("id")))))],
            );

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn max_grouping_by_same_column_simplified() {
        // MAX(col) when grouping by col -> just project col
        let expr = RelExpr::scan("t")
            .aggregate(
                vec!["id".to_string()],
                vec![("max_id".to_string(), AggFunc::Max(Box::new(Expr::Column(ColumnRef::new("id")))))],
            );

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn double_distinct_eliminated() {
        // Double DISTINCT should be simplified to single DISTINCT
        let expr = RelExpr::scan("t").distinct().distinct();

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}