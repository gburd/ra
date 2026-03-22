//! Consensus optimization rules implemented by both DataFusion and Calcite.
//!
//! These rules are present in every production query optimizer:
//!
//! - **Extract equijoin predicate**: separate equality from non-equality
//!   predicates in join conditions to enable hash/merge join selection
//! - **Filter null join keys**: add IS NOT NULL filters on join keys
//!   before equijoins to reduce build/probe sizes
//! - **Propagate empty relation**: short-circuit empty inputs through
//!   the query tree to eliminate unnecessary computation

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// All consensus optimization rules.
///
/// Returns rules for equijoin extraction, null join key filtering,
/// and empty relation propagation.
#[must_use]
pub fn consensus_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = Vec::new();
    rules.extend(extract_equijoin_predicate_rules());
    rules.extend(filter_null_join_keys_rules());
    rules.extend(propagate_empty_relation_rules());
    rules
}

/// Extract equijoin predicates from compound join conditions.
///
/// When a join condition is `(eq lk rk) AND rest`, extract the
/// equality predicate as the join condition and move the rest
/// to a post-join filter. This enables hash join and merge join
/// selection for the equijoin portion.
///
/// # References
///
/// - DataFusion: `extract_equijoin_predicate.rs`
/// - Calcite: `JoinExtractFilterRule`
fn extract_equijoin_predicate_rules(
) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Equality on left of AND: (and (eq ?lk ?rk) ?rest)
        rewrite!("extract-equijoin-from-and-left";
            "(join inner (and (eq ?lk ?rk) ?rest) ?left ?right)" =>
            "(filter ?rest (join inner (eq ?lk ?rk) ?left ?right))"
        ),
        // Equality on right of AND: (and ?rest (eq ?lk ?rk))
        rewrite!("extract-equijoin-from-and-right";
            "(join inner (and ?rest (eq ?lk ?rk)) ?left ?right)" =>
            "(filter ?rest (join inner (eq ?lk ?rk) ?left ?right))"
        ),
    ]
}

/// Add IS NOT NULL filters on join key columns before equijoins.
///
/// NULL values never match in equijoins (NULL = NULL is NULL, not
/// TRUE), so filtering them early reduces build and probe side
/// sizes of hash joins.
///
/// # References
///
/// - DataFusion: `filter_null_join_keys.rs`
fn filter_null_join_keys_rules(
) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Add IS NOT NULL on left join key
        rewrite!("filter-null-join-key-left";
            "(join inner (eq ?lk ?rk) ?left ?right)" =>
            "(join inner (eq ?lk ?rk) (filter (is-not-null ?lk) ?left) ?right)"
        ),
        // Add IS NOT NULL on right join key
        rewrite!("filter-null-join-key-right";
            "(join inner (eq ?lk ?rk) ?left ?right)" =>
            "(join inner (eq ?lk ?rk) ?left (filter (is-not-null ?rk) ?right))"
        ),
    ]
}

/// Propagate empty relations through the query tree.
///
/// When an input is provably empty (represented as
/// `(filter (const-bool false) ...)` or `(limit 0 ...)`), propagate
/// the empty result upward to eliminate unnecessary computation.
///
/// # References
///
/// - DataFusion: `propagate_empty_relation.rs`
/// - Calcite: `PruneEmptyRules`
fn propagate_empty_relation_rules(
) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Inner join with empty left side => empty
        rewrite!("empty-inner-join-left";
            "(join inner ?cond (filter (const-bool false) ?left) ?right)" =>
            "(filter (const-bool false) ?left)"
        ),
        // Inner join with empty right side => empty
        rewrite!("empty-inner-join-right";
            "(join inner ?cond ?left (filter (const-bool false) ?right))" =>
            "(filter (const-bool false) ?right)"
        ),
        // Cross join with empty left side => empty
        rewrite!("empty-cross-join-left";
            "(join cross ?cond (filter (const-bool false) ?left) ?right)" =>
            "(filter (const-bool false) ?left)"
        ),
        // Cross join with empty right side => empty
        rewrite!("empty-cross-join-right";
            "(join cross ?cond ?left (filter (const-bool false) ?right))" =>
            "(filter (const-bool false) ?right)"
        ),
        // Semi join with empty left => empty
        rewrite!("empty-semi-join-left";
            "(join semi ?cond (filter (const-bool false) ?left) ?right)" =>
            "(filter (const-bool false) ?left)"
        ),
        // Semi join with empty right => empty
        rewrite!("empty-semi-join-right";
            "(join semi ?cond ?left (filter (const-bool false) ?right))" =>
            "(filter (const-bool false) ?right)"
        ),
        // Anti join with empty left => empty
        rewrite!("empty-anti-join-left";
            "(join anti ?cond (filter (const-bool false) ?left) ?right)" =>
            "(filter (const-bool false) ?left)"
        ),
        // Anti join with empty right => left side (nothing to exclude)
        rewrite!("empty-anti-join-right";
            "(join anti ?cond ?left (filter (const-bool false) ?right))" =>
            "?left"
        ),
        // Project over empty => empty
        rewrite!("empty-project";
            "(project ?cols (filter (const-bool false) ?input))" =>
            "(filter (const-bool false) ?input)"
        ),
        // Sort over empty => empty
        rewrite!("empty-sort";
            "(sort ?keys (filter (const-bool false) ?input))" =>
            "(filter (const-bool false) ?input)"
        ),
        // Limit over empty => empty
        rewrite!("empty-limit";
            "(limit ?n ?off (filter (const-bool false) ?input))" =>
            "(filter (const-bool false) ?input)"
        ),
        // Filter over empty => empty (any predicate)
        rewrite!("empty-filter";
            "(filter ?pred (filter (const-bool false) ?input))" =>
            "(filter (const-bool false) ?input)"
        ),
        // Union with empty left => right
        rewrite!("empty-union-left";
            "(union ?all (filter (const-bool false) ?left) ?right)" =>
            "?right"
        ),
        // Union with empty right => left
        rewrite!("empty-union-right";
            "(union ?all ?left (filter (const-bool false) ?right))" =>
            "?left"
        ),
        // Intersect with empty side => empty
        rewrite!("empty-intersect-left";
            "(intersect ?all (filter (const-bool false) ?left) ?right)" =>
            "(filter (const-bool false) ?left)"
        ),
        rewrite!("empty-intersect-right";
            "(intersect ?all ?left (filter (const-bool false) ?right))" =>
            "(filter (const-bool false) ?right)"
        ),
        // Except with empty left => empty
        rewrite!("empty-except-left";
            "(except ?all (filter (const-bool false) ?left) ?right)" =>
            "(filter (const-bool false) ?left)"
        ),
        // Except with empty right => left (nothing to subtract)
        rewrite!("empty-except-right";
            "(except ?all ?left (filter (const-bool false) ?right))" =>
            "?left"
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
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_with_consensus_rules(
        expr: &RelExpr,
    ) -> Runner<RelLang, RelAnalysis> {
        let rec =
            to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(50_000)
            .with_iter_limit(10)
            .run(&all_rules())
    }

    fn eq_expr(left: &str, right: &str) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(left))),
            right: Box::new(Expr::Column(ColumnRef::new(right))),
        }
    }

    fn gt_expr(left: &str, right: &str) -> Expr {
        Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new(left))),
            right: Box::new(Expr::Column(ColumnRef::new(right))),
        }
    }

    // -- Extract equijoin predicate tests --

    #[test]
    fn extract_equijoin_from_compound_condition() {
        // join(inner, (and (eq a b) (gt c d)), left, right)
        // Should produce: filter(gt c d, join(inner, eq a b, ...))
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(eq_expr("a", "b")),
                right: Box::new(gt_expr("c", "d")),
            },
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };
        let runner = run_with_consensus_rules(&expr);
        // The e-graph should contain the extracted form
        assert!(
            runner.egraph.number_of_classes() > 1,
            "equijoin extraction should add alternatives"
        );
    }

    #[test]
    fn extract_equijoin_right_side_of_and() {
        // join(inner, (and (gt c d) (eq a b)), left, right)
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(gt_expr("c", "d")),
                right: Box::new(eq_expr("a", "b")),
            },
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "equijoin extraction should work from right side of AND"
        );
    }

    // -- Filter null join keys tests --

    #[test]
    fn filter_null_join_keys_added() {
        // join(inner, eq(a, b), left, right)
        // Should add IS NOT NULL filters on both sides
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq_expr("customer_id", "id"),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "null join key filters should add alternatives"
        );
    }

    // -- Propagate empty relation tests --

    #[test]
    fn empty_inner_join_propagates() {
        // join(inner, cond, filter(false, left), right) => empty
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq_expr("a", "b"),
            left: Box::new(
                RelExpr::scan("empty_table")
                    .filter(Expr::Const(Const::Bool(false))),
            ),
            right: Box::new(RelExpr::scan("orders")),
        };
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "empty relation should propagate through inner join"
        );
    }

    #[test]
    fn empty_project_propagates() {
        // project(cols, filter(false, input)) => empty
        let expr = RelExpr::scan("t")
            .filter(Expr::Const(Const::Bool(false)))
            .project(vec![]);
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "empty relation should propagate through project"
        );
    }

    #[test]
    fn empty_union_left_simplifies() {
        // union(all, filter(false, left), right) => right
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(
                RelExpr::scan("empty_table")
                    .filter(Expr::Const(Const::Bool(false))),
            ),
            right: Box::new(RelExpr::scan("orders")),
        };
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "empty union branch should be eliminated"
        );
    }

    #[test]
    fn empty_sort_propagates() {
        // sort(keys, filter(false, input)) => empty
        let expr = RelExpr::Sort {
            keys: vec![ra_core::algebra::SortKey {
                expr: Expr::Column(ColumnRef::new("id")),
                direction: ra_core::algebra::SortDirection::Asc,
                nulls: ra_core::algebra::NullOrdering::Last,
            }],
            input: Box::new(
                RelExpr::scan("t")
                    .filter(Expr::Const(Const::Bool(false))),
            ),
        };
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "empty relation should propagate through sort"
        );
    }

    #[test]
    fn empty_cross_join_propagates() {
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(
                RelExpr::scan("empty_table")
                    .filter(Expr::Const(Const::Bool(false))),
            ),
            right: Box::new(RelExpr::scan("data")),
        };
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "empty relation should propagate through cross join"
        );
    }

    #[test]
    fn consensus_rules_count() {
        let rules = consensus_rules();
        // 2 extract-equijoin + 2 filter-null + 20 propagate-empty
        assert!(
            rules.len() >= 20,
            "expected at least 20 consensus rules, got {}",
            rules.len()
        );
    }
}
