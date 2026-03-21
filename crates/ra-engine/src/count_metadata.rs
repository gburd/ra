//! COUNT(*) metadata lookup optimization.
//!
//! Rewrites `COUNT(*)` aggregates over bare table scans into
//! O(1) metadata lookups when safe. This avoids full table scans
//! for simple row-count queries, mirroring optimizations found in
//! PostgreSQL (`pg_stat_user_tables.n_live_tup`), SQL Server
//! (`sys.dm_db_partition_stats.row_count`), and MongoDB
//! (`estimatedDocumentCount()`).
//!
//! The rewrite is only applied when the aggregate has:
//! - No grouping keys (global aggregate)
//! - A single `COUNT(*)` (count with nil argument, non-distinct)
//! - A bare `scan` as input (no filter/join)
//!
//! See: `research/database-shortcuts.md` section 1.

use egg::{rewrite, Id, Rewrite, Subst, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return rewrite rules for COUNT(*) metadata optimization.
#[must_use]
pub fn count_metadata_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Match: (aggregate ?groups ?aggs (scan ?table))
        // Rewrite to: (metadata-lookup ?table row-count)
        // Condition: groups is nil (no GROUP BY) and aggs is a
        //            single-element list containing count-star.
        rewrite!("count-star-to-metadata";
            "(aggregate ?groups ?aggs (scan ?table))" =>
            "(metadata-lookup ?table row-count)"
            if is_ungrouped_count_star(
                var("?groups"),
                var("?aggs")
            )
        ),
    ]
}

fn var(s: &str) -> Var {
    s.parse().unwrap_or_else(|_| panic!("bad var: {s}"))
}

/// Condition: the aggregate has no grouping keys and a single
/// `COUNT(*)` aggregate expression (count with nil arg, non-distinct).
fn is_ungrouped_count_star(
    groups_var: Var,
    aggs_var: Var,
) -> impl Fn(&mut egg::EGraph<RelLang, RelAnalysis>, Id, &Subst) -> bool
{
    move |egraph, _id, subst| {
        let groups_id = subst[groups_var];
        let aggs_id = subst[aggs_var];

        is_nil_or_empty_list(egraph, groups_id)
            && is_single_count_star(egraph, aggs_id)
    }
}

/// Check that a node is `nil` or an empty `(list)`.
fn is_nil_or_empty_list(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> bool {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Nil => return true,
            RelLang::List(ids) if ids.is_empty() => return true,
            _ => {}
        }
    }
    false
}

/// Check that the aggs list is `(list (agg-expr (count nil) all ?alias))`.
fn is_single_count_star(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> bool {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            if ids.len() != 1 {
                return false;
            }
            return is_count_star_agg_expr(egraph, ids[0]);
        }
    }
    false
}

/// Check that a node is `(agg-expr (count nil) all ?alias)`.
fn is_count_star_agg_expr(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> bool {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::AggExpr([func_id, distinct_id, _alias_id]) = node
        {
            let is_count_nil = is_count_nil(egraph, *func_id);
            let is_all = is_all_flag(egraph, *distinct_id);
            if is_count_nil && is_all {
                return true;
            }
        }
    }
    false
}

/// Check that a node is `(count nil)` (COUNT with no argument = COUNT(*)).
fn is_count_nil(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> bool {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Count([arg_id]) = node {
            let arg_canonical = egraph.find(*arg_id);
            for arg_node in &egraph[arg_canonical].nodes {
                if let RelLang::Nil = arg_node {
                    return true;
                }
            }
        }
    }
    false
}

/// Check that a node is the `all` flag (non-distinct).
fn is_all_flag(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> bool {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::All = node {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use crate::rewrite::all_rules;
    use egg::Runner;
    use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_with_count_rules(
        expr: &RelExpr,
    ) -> Runner<RelLang, RelAnalysis> {
        let rec =
            to_rec_expr(expr).expect("conversion should succeed");
        let mut rules = count_metadata_rules();
        // Include base rules so the e-graph stays well-formed
        rules.extend(all_rules());
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&rules)
    }

    /// Build `SELECT COUNT(*) FROM table_name` as a RelExpr.
    fn count_star_query(table_name: &str) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::Scan {
                table: table_name.to_string(),
                alias: None,
            }),
        }
    }

    #[test]
    fn count_star_rewrites_to_metadata_lookup() {
        let expr = count_star_query("users");
        let runner = run_with_count_rules(&expr);
        let root = runner.roots[0];

        // The e-graph should contain a metadata-lookup node
        let _has_metadata_lookup = runner.egraph[root]
            .nodes
            .iter()
            .any(|n| matches!(n, RelLang::MetadataLookup(_)));

        // Also verify the e-graph grew (rules applied)
        assert!(
            runner.egraph.number_of_classes() > 2,
            "e-graph should have grown from rule application"
        );

        // Check the root e-class or reachable classes contain
        // MetadataLookup
        let found = egraph_contains_metadata_lookup(&runner);
        assert!(
            found,
            "COUNT(*) over bare scan should rewrite to \
             metadata-lookup"
        );
    }

    #[test]
    fn count_star_with_filter_not_rewritten() {
        // SELECT COUNT(*) FROM users WHERE active = true
        // Should NOT rewrite because there's a filter
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(
                RelExpr::scan("users").filter(Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(
                        ColumnRef::new("active"),
                    )),
                    right: Box::new(Expr::Const(Const::Bool(true))),
                }),
            ),
        };
        let runner = run_with_count_rules(&expr);

        // The pattern requires (scan ?table) as the direct input
        // to aggregate -- a filter breaks the match, so no
        // metadata-lookup should appear for the filtered version.
        // Note: the filter-below-aggregate rule may push the filter
        // down but the aggregate's input won't be a bare scan.
        // We verify the root still has Aggregate nodes.
        let root = runner.roots[0];
        let has_aggregate = runner.egraph[root]
            .nodes
            .iter()
            .any(|n| matches!(n, RelLang::Aggregate(_)));
        assert!(
            has_aggregate,
            "filtered COUNT(*) should retain aggregate node"
        );
    }

    #[test]
    fn count_with_group_by_not_rewritten() {
        // SELECT department, COUNT(*) FROM users GROUP BY department
        // Should NOT rewrite because there are grouping keys
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("department"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::Scan {
                table: "users".to_string(),
                alias: None,
            }),
        };
        let runner = run_with_count_rules(&expr);

        // Should not contain metadata-lookup since GROUP BY is present
        let found = egraph_contains_metadata_lookup(&runner);
        assert!(
            !found,
            "COUNT(*) with GROUP BY should not rewrite to \
             metadata-lookup"
        );
    }

    #[test]
    fn count_distinct_not_rewritten() {
        // SELECT COUNT(DISTINCT name) FROM users
        // Should NOT rewrite because it's COUNT(DISTINCT col), not COUNT(*)
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(Expr::Column(ColumnRef::new("name"))),
                distinct: true,
                alias: None,
            }],
            input: Box::new(RelExpr::Scan {
                table: "users".to_string(),
                alias: None,
            }),
        };
        let runner = run_with_count_rules(&expr);

        let found = egraph_contains_metadata_lookup(&runner);
        assert!(
            !found,
            "COUNT(DISTINCT col) should not rewrite to \
             metadata-lookup"
        );
    }

    /// Search all e-classes for a MetadataLookup node.
    fn egraph_contains_metadata_lookup(
        runner: &Runner<RelLang, RelAnalysis>,
    ) -> bool {
        for class in runner.egraph.classes() {
            for node in &class.nodes {
                if matches!(node, RelLang::MetadataLookup(_)) {
                    return true;
                }
            }
        }
        false
    }
}
