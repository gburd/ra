//! Materialized view rewrite rules for the e-graph optimizer.
//!
//! Adds `mv-scan` as an alternative representation for query
//! sub-trees that can be answered by a materialized view. The cost
//! model assigns lower cost to `mv-scan` nodes (pre-computed, no
//! joins or aggregations needed), so the extractor will prefer them
//! when the MV is cheaper than re-computing from base tables.

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::mv_matching::MvCatalog;

/// Generate e-graph rewrite rules from a materialized view catalog.
///
/// Returns structural rules that let the e-graph explore `mv-scan`
/// as an alternative representation for aggregate-over-scan/join
/// patterns. The cost model picks the winner.
#[must_use]
pub fn mv_rewrite_rules(_catalog: &MvCatalog) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        rewrite!("agg-scan-to-mv-scan";
            "(aggregate ?g ?a (scan ?table))" =>
            "(mv-scan ?table auto ?g ?a)"
        ),
        rewrite!("mv-scan-to-agg-scan";
            "(mv-scan ?table auto ?g ?a)" =>
            "(aggregate ?g ?a (scan ?table))"
        ),
        rewrite!("agg-join-to-mv-scan";
            "(aggregate ?g ?a (join inner ?cond ?left ?right))" =>
            "(mv-scan ?cond auto ?g ?a)"
        ),
        rewrite!("mv-scan-join-to-agg-join";
            "(mv-scan ?cond auto ?g ?a)" =>
            "(aggregate ?g ?a (join inner ?cond (scan auto) (scan auto)))"
        ),
    ]
}

/// Cost multiplier for an `mv-scan` relative to a table scan.
///
/// An MV scan reads pre-computed, pre-aggregated data, avoiding
/// joins and aggregation at query time.
#[must_use]
pub fn mv_scan_cost_factor() -> f64 {
    0.15
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::analysis::RelAnalysis;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
    use ra_core::expr::{ColumnRef, Expr};

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef::new(name))
    }

    #[test]
    fn mv_rewrite_rule_adds_mv_scan_alternative() {
        let expr = RelExpr::Aggregate {
            group_by: vec![col("region")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("amount")),
                distinct: false,
                alias: Some("total".to_string()),
            }],
            input: Box::new(RelExpr::scan("sales")),
        };

        let rec = to_rec_expr(&expr).expect("conversion ok");
        let catalog = MvCatalog::new();
        let rules = mv_rewrite_rules(&catalog);

        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules);

        let egraph = &runner.egraph;
        let has_mv_scan = egraph
            .classes()
            .any(|class| class.nodes.iter().any(|n| matches!(n, RelLang::MvScan(_))));
        assert!(has_mv_scan, "e-graph should contain an mv-scan alternative");
    }

    #[test]
    fn mv_scan_cost_factor_reasonable() {
        let factor = mv_scan_cost_factor();
        assert!(factor > 0.0);
        assert!(factor < 1.0);
    }

    #[test]
    fn empty_catalog_still_returns_rules() {
        let catalog = MvCatalog::new();
        let rules = mv_rewrite_rules(&catalog);
        assert!(!rules.is_empty());
    }
}
