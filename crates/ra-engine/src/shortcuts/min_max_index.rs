//! MIN/MAX index optimization.
//!
//! Rewrites `MIN(col)` and `MAX(col)` aggregates over a table scan
//! into `LIMIT 1` + `SORT` + `INDEX SCAN`, reducing cost from O(n)
//! to O(log n) when a B-tree index exists on the column.
//!
//! # Preconditions
//!
//! These rules should only be applied when:
//! - A B-tree index exists on the aggregated column (first column)
//! - The column has no NULLs, or an `IS NOT NULL` filter is added
//! - The aggregate is a single MIN or MAX (not multiple with
//!   different columns requiring different sort orders)
//!
//! # Safety
//!
//! Always safe when a B-tree index exists on the column. The rewrite
//! is semantically equivalent because:
//! - `MIN(col)` = first row when sorted ascending
//! - `MAX(col)` = first row when sorted descending

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Rewrite rules for MIN/MAX index optimization.
///
/// Transforms:
/// - `MIN(col)` over scan -> `LIMIT 1` of ascending index scan
/// - `MAX(col)` over scan -> `LIMIT 1` of descending index scan
///
/// The `index-scan` node signals to the cost model that a B-tree
/// traversal is used instead of a full table scan.
#[must_use]
pub fn min_max_index_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // MIN(col) with no GROUP BY on a table scan =>
        // LIMIT 1, SORT ASC on index-scan
        //
        // Pattern: (aggregate (list) (list (agg-expr (min (col ?col))
        //           ?dist ?alias)) (scan ?table))
        // Result:  (limit 1 0 (sort (list (sort-key (col ?col) asc
        //           nulls-last)) (index-scan ?table ?col)))
        rewrite!("min-to-index-scan";
            "(aggregate (list) (list (agg-expr (min (col ?col)) ?dist ?alias)) (scan ?table))" =>
            "(aggregate (list) (list (agg-expr (min (col ?col)) ?dist ?alias)) (limit (const-int 1) (const-int 0) (sort (list (sort-key (col ?col) asc nulls-last)) (index-scan ?table ?col))))"
        ),
        // MAX(col) with no GROUP BY on a table scan =>
        // LIMIT 1, SORT DESC on index-scan
        rewrite!("max-to-index-scan";
            "(aggregate (list) (list (agg-expr (max (col ?col)) ?dist ?alias)) (scan ?table))" =>
            "(aggregate (list) (list (agg-expr (max (col ?col)) ?dist ?alias)) (limit (const-int 1) (const-int 0) (sort (list (sort-key (col ?col) desc nulls-last)) (index-scan ?table ?col))))"
        ),
        // MIN(col) with filter: push index-scan under filter
        //
        // This handles WHERE-filtered MIN queries by replacing the
        // scan with an index-scan, keeping the filter in place.
        rewrite!("min-filtered-to-index-scan";
            "(aggregate (list) (list (agg-expr (min (col ?col)) ?dist ?alias)) (filter ?pred (scan ?table)))" =>
            "(aggregate (list) (list (agg-expr (min (col ?col)) ?dist ?alias)) (limit (const-int 1) (const-int 0) (sort (list (sort-key (col ?col) asc nulls-last)) (filter ?pred (index-scan ?table ?col)))))"
        ),
        // MAX(col) with filter
        rewrite!("max-filtered-to-index-scan";
            "(aggregate (list) (list (agg-expr (max (col ?col)) ?dist ?alias)) (filter ?pred (scan ?table)))" =>
            "(aggregate (list) (list (agg-expr (max (col ?col)) ?dist ?alias)) (limit (const-int 1) (const-int 0) (sort (list (sort-key (col ?col) desc nulls-last)) (filter ?pred (index-scan ?table ?col)))))"
        ),
    ]
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::analysis::RelAnalysis;
    use crate::egraph::{to_rec_expr, RelLang};
    use crate::rewrite::all_rules;
    use egg::Runner;
    use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_with_min_max_rules(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        let mut rules = min_max_index_rules();
        rules.extend(crate::rewrite::aggregate_optimization_rules());
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules)
    }

    fn min_aggregate(col: &str) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Min,
                arg: Some(Expr::Column(ColumnRef::new(col))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("orders")),
        }
    }

    fn max_aggregate(col: &str) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Max,
                arg: Some(Expr::Column(ColumnRef::new(col))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("orders")),
        }
    }

    #[test]
    fn min_indexed_col_rewrites_to_index_scan() {
        let expr = min_aggregate("order_id");
        let runner = run_with_min_max_rules(&expr);

        // The e-graph should contain the index-scan alternative
        let _root = runner.roots[0];
        let class_count = runner.egraph.number_of_classes();
        // Rewrite rules should have expanded the e-graph
        assert!(
            class_count > 5,
            "expected e-graph expansion from MIN index rewrite, \
             got {class_count} classes"
        );

        // Verify the index-scan node exists somewhere in the e-graph
        let has_index_scan = runner.egraph.classes().any(|class| {
            class
                .nodes
                .iter()
                .any(|node| matches!(node, RelLang::IndexScan(_)))
        });
        assert!(
            has_index_scan,
            "expected index-scan node in e-graph after MIN rewrite"
        );

        // Verify the sort node with ascending direction exists
        let has_asc_sort = runner
            .egraph
            .classes()
            .any(|class| class.nodes.iter().any(|node| matches!(node, RelLang::Asc)));
        assert!(has_asc_sort, "expected ascending sort in MIN index rewrite");
    }

    #[test]
    fn max_indexed_col_rewrites_to_index_scan() {
        let expr = max_aggregate("amount");
        let runner = run_with_min_max_rules(&expr);

        let has_index_scan = runner.egraph.classes().any(|class| {
            class
                .nodes
                .iter()
                .any(|node| matches!(node, RelLang::IndexScan(_)))
        });
        assert!(
            has_index_scan,
            "expected index-scan node in e-graph after MAX rewrite"
        );

        // Verify descending sort for MAX
        let has_desc_sort = runner
            .egraph
            .classes()
            .any(|class| class.nodes.iter().any(|node| matches!(node, RelLang::Desc)));
        assert!(
            has_desc_sort,
            "expected descending sort in MAX index rewrite"
        );
    }

    #[test]
    fn min_with_where_clause_rewrites() {
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Min,
                arg: Some(Expr::Column(ColumnRef::new("price"))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::Filter {
                predicate: Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("quantity"))),
                    right: Box::new(Expr::Const(Const::Int(0))),
                },
                input: Box::new(RelExpr::scan("orders")),
            }),
        };
        let runner = run_with_min_max_rules(&expr);

        let has_index_scan = runner.egraph.classes().any(|class| {
            class
                .nodes
                .iter()
                .any(|node| matches!(node, RelLang::IndexScan(_)))
        });
        assert!(has_index_scan, "expected index-scan for filtered MIN query");
    }

    #[test]
    fn non_indexed_no_spurious_rewrite() {
        // When there are multiple aggregates, the pattern should NOT match
        // because the pattern requires exactly one agg-expr in the list.
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![
                AggregateExpr {
                    function: AggregateFunction::Min,
                    arg: Some(Expr::Column(ColumnRef::new("a"))),
                    distinct: false,
                    alias: None,
                },
                AggregateExpr {
                    function: AggregateFunction::Max,
                    arg: Some(Expr::Column(ColumnRef::new("b"))),
                    distinct: false,
                    alias: None,
                },
            ],
            input: Box::new(RelExpr::scan("orders")),
        };
        let runner = run_with_min_max_rules(&expr);

        // With multiple aggregates the pattern should not match,
        // so no index-scan should appear.
        let has_index_scan = runner.egraph.classes().any(|class| {
            class
                .nodes
                .iter()
                .any(|node| matches!(node, RelLang::IndexScan(_)))
        });
        assert!(
            !has_index_scan,
            "should not rewrite multi-aggregate to index scan"
        );
    }

    #[test]
    fn min_max_rules_integrate_with_all_rules() {
        let expr = min_aggregate("order_id");
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let rules = all_rules();
        let runner = Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules);

        let has_index_scan = runner.egraph.classes().any(|class| {
            class
                .nodes
                .iter()
                .any(|node| matches!(node, RelLang::IndexScan(_)))
        });
        assert!(
            has_index_scan,
            "MIN index rewrite should fire in full rule set"
        );
    }

    #[test]
    fn index_scan_tracks_table_in_analysis() {
        let expr = min_aggregate("order_id");
        let runner = run_with_min_max_rules(&expr);

        // Find the e-class containing the index-scan and verify
        // it tracks the "orders" table.
        let tracks_table = runner.egraph.classes().any(|class| {
            let has_iscan = class
                .nodes
                .iter()
                .any(|node| matches!(node, RelLang::IndexScan(_)));
            has_iscan && class.data.tables.contains("orders")
        });
        assert!(
            tracks_table,
            "index-scan e-class should track the scanned table"
        );
    }
}
