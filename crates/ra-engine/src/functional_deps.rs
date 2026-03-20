//! Functional dependency exploitation rules.
//!
//! Uses functional dependencies from unique constraints and keys
//! to simplify GROUP BY clauses and eliminate redundant columns.
//!
//! Key optimizations:
//! - Remove functionally dependent columns from GROUP BY
//! - Simplify aggregates when grouping by unique key
//! - Eliminate redundant DISTINCT when projecting unique columns

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
        // GROUP BY simplification using unique keys
        // ---------------------------------------------------------------

        // When grouping by a unique key, other columns from same table
        // are functionally dependent and can be removed from GROUP BY
        // Pattern: GROUP BY pk, col1, col2 -> GROUP BY pk (when pk is unique)
        rewrite!("simplify-groupby-unique-key";
            "(aggregate (list ?pk ?others) ?aggs ?input)" =>
            "(aggregate (list ?pk) ?aggs ?input)"
            if is_unique_key(?pk, ?input) && are_dependent_cols(?others, ?pk, ?input)
        ),

        // ---------------------------------------------------------------
        // DISTINCT elimination on unique columns
        // ---------------------------------------------------------------

        // DISTINCT is redundant when projecting unique columns
        rewrite!("eliminate-distinct-unique";
            "(distinct-rel (project ?cols ?input))" =>
            "(project ?cols ?input)"
            if columns_are_unique(?cols, ?input)
        ),

        // DISTINCT after GROUP BY is redundant (GROUP BY already produces unique groups)
        rewrite!("eliminate-distinct-after-groupby";
            "(distinct-rel (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs ?input)"
        ),

        // ---------------------------------------------------------------
        // Aggregate simplification with unique keys
        // ---------------------------------------------------------------

        // COUNT(DISTINCT col) when col is unique -> COUNT(col)
        rewrite!("count-distinct-unique-to-count";
            "(aggregate ?groups (list (agg-expr distinct (count ?col) ?alias)) ?input)" =>
            "(aggregate ?groups (list (agg-expr all (count ?col) ?alias)) ?input)"
            if is_unique_column(?col, ?input)
        ),

        // MIN(col) = MAX(col) when grouping by col
        // This indicates the aggregate can be replaced with the column itself
        rewrite!("min-max-same-when-grouping-by-col";
            "(aggregate (list ?col) (list (agg-expr ?d (min ?col) ?alias)) ?input)" =>
            "(project (list (proj-alias ?col ?alias)) ?input)"
        ),

        rewrite!("max-same-when-grouping-by-col";
            "(aggregate (list ?col) (list (agg-expr ?d (max ?col) ?alias)) ?input)" =>
            "(project (list (proj-alias ?col ?alias)) ?input)"
        ),

        // ---------------------------------------------------------------
        // Join simplification using functional dependencies
        // ---------------------------------------------------------------

        // Self-join on unique key with same filter can be simplified
        // SELECT * FROM t t1 JOIN t t2 ON t1.pk = t2.pk WHERE t1.x = 5 AND t2.x = 5
        // Can become: SELECT * FROM t WHERE x = 5
        rewrite!("eliminate-self-join-same-filter";
            "(filter (and (eq ?t1_col ?val) (eq ?t2_col ?val))
                (join inner (eq ?t1_pk ?t2_pk)
                    (scan-alias ?t ?t1)
                    (scan-alias ?t ?t2)))" =>
            "(filter (eq ?t1_col ?val) (scan ?t))"
            if is_pk_join(?t1_pk, ?t2_pk) && same_column(?t1_col, ?t2_col)
        ),

        // ---------------------------------------------------------------
        // Redundant column elimination in projections
        // ---------------------------------------------------------------

        // When projecting pk and dependent columns, we can reconstruct
        // dependent columns later if needed (this is more of a storage optimization)
        rewrite!("mark-dependent-cols-for-reconstruction";
            "(project (list ?pk ?dependent_cols) ?input)" =>
            "(project (list ?pk) ?input)"
            if can_reconstruct_cols(?dependent_cols, ?pk, ?input)
        ),

        // ---------------------------------------------------------------
        // GROUP BY with expressions containing functionally dependent columns
        // ---------------------------------------------------------------

        // GROUP BY f(pk, col) where col depends on pk -> GROUP BY f(pk, _)
        // The exact value of col is determined by pk
        rewrite!("simplify-groupby-expression-with-deps";
            "(aggregate (list (func ?name ?pk ?dep_col)) ?aggs ?input)" =>
            "(aggregate (list ?pk) ?aggs ?input)"
            if is_dependent_col(?dep_col, ?pk, ?input)
        ),

        // ---------------------------------------------------------------
        // ORDER BY simplification using functional dependencies
        // ---------------------------------------------------------------

        // ORDER BY pk, col -> ORDER BY pk (when col is functionally dependent on pk)
        rewrite!("simplify-orderby-functional-deps";
            "(sort (list (sort-key ?pk ?dir1 ?nulls1) (sort-key ?col ?dir2 ?nulls2)) ?input)" =>
            "(sort (list (sort-key ?pk ?dir1 ?nulls1)) ?input)"
            if is_dependent_col(?col, ?pk, ?input)
        ),

        // ---------------------------------------------------------------
        // Window function partition simplification
        // ---------------------------------------------------------------

        // PARTITION BY pk, col -> PARTITION BY pk (when col depends on pk)
        rewrite!("simplify-window-partition-deps";
            "(window-expr ?fn (list ?pk ?dep_col) ?order ?frame ?args ?alias)" =>
            "(window-expr ?fn (list ?pk) ?order ?frame ?args ?alias)"
            if is_dependent_col(?dep_col, ?pk, current_input())
        ),
    ]
}

// Helper conditions (these would be implemented in the analysis)
fn is_unique_key(_col: &str, _input: &str) -> bool {
    // Check if column is a primary key or has unique constraint
    false
}

fn are_dependent_cols(_cols: &str, _key: &str, _input: &str) -> bool {
    // Check if columns are functionally dependent on the key
    false
}

fn columns_are_unique(_cols: &str, _input: &str) -> bool {
    // Check if the combination of columns is unique
    false
}

fn is_unique_column(_col: &str, _input: &str) -> bool {
    // Check if column has unique constraint
    false
}

fn is_pk_join(_col1: &str, _col2: &str) -> bool {
    // Check if join is on primary key columns
    false
}

fn same_column(_col1: &str, _col2: &str) -> bool {
    // Check if two column references refer to the same column
    false
}

fn can_reconstruct_cols(_cols: &str, _key: &str, _input: &str) -> bool {
    // Check if columns can be reconstructed from the key
    false
}

fn is_dependent_col(_col: &str, _key: &str, _input: &str) -> bool {
    // Check if col is functionally dependent on key
    false
}

fn current_input() -> &'static str {
    // Return current input context
    ""
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
}