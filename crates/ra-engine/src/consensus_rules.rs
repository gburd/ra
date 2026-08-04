//! Consensus optimization rules implemented by both `DataFusion` and Calcite.
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
/// - `DataFusion`: `extract_equijoin_predicate.rs`
/// - Calcite: `JoinExtractFilterRule`
fn extract_equijoin_predicate_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
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
/// - `DataFusion`: `filter_null_join_keys.rs`
fn filter_null_join_keys_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // Add IS NOT NULL on left join key — only when the left key actually
        // references only the left input. Without this guard, eq-commutative
        // can swap the key positions so ?lk references the *right* table, and
        // the derived IS NOT NULL would be placed on the wrong (left) child
        // (an unsound rewrite the cardinality-aware extractor would then pick).
        rewrite!("filter-null-join-key-left";
            "(join inner (eq ?lk ?rk) ?left ?right)" =>
            "(join inner (eq ?lk ?rk) (filter (is-not-null ?lk) ?left) ?right)"
            if crate::conditions::references_only("?lk", "?left")
        ),
        // Add IS NOT NULL on right join key — guarded symmetrically.
        rewrite!("filter-null-join-key-right";
            "(join inner (eq ?lk ?rk) ?left ?right)" =>
            "(join inner (eq ?lk ?rk) ?left (filter (is-not-null ?rk) ?right))"
            if crate::conditions::references_only("?rk", "?right")
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
/// - `DataFusion`: `propagate_empty_relation.rs`
/// - Calcite: `PruneEmptyRules`
fn propagate_empty_relation_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    // CORRECTNESS (RA-STEERING §2, Codeberg #17): only propagate empty
    // relations through operators that DO NOT drop a relation reference.
    // Rewrites that drop the other side of a join, or a set-op branch, are
    // UNSOUND as a general planner rewrite: PostgreSQL still locks / checks
    // permissions on / applies RLS to every relation referenced by the query,
    // even when a constant-false predicate makes the result provably empty.
    // They are also non-idempotent (the table set changes between passes,
    // caught by proptest optimization_twice_preserves_tables). The former
    // empty-{inner,cross,semi,anti}-join-{left,right} and
    // empty-{union,intersect,except}-{left,right} arms are intentionally
    // omitted; the retained arms keep every input relation as a descendant of
    // the resulting filter(false, …).
    vec![
        // Project over empty => empty (input subtree preserved)
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
    ]
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::analysis::RelAnalysis;
    use crate::egraph::{to_rec_expr, RelLang};
    use crate::rewrite::all_rules;
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_with_consensus_rules(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
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
    //
    // Correctness (Codeberg #17): empty propagation through a join/set-op must
    // NOT drop a relation. These tests assert the *relation-preserving*
    // behavior: after optimization the plan still references every base table,
    // and optimizing twice is idempotent w.r.t. the table set.

    fn collect_scans(e: &RelExpr, out: &mut std::collections::BTreeSet<String>) {
        if let RelExpr::Scan { table, .. } = e {
            out.insert(table.clone());
        }
        for c in e.children() {
            collect_scans(c, out);
        }
    }

    fn tables_of(e: &RelExpr) -> std::collections::BTreeSet<String> {
        let mut out = std::collections::BTreeSet::new();
        collect_scans(e, &mut out);
        out
    }

    #[test]
    fn empty_inner_join_preserves_both_tables() {
        // join(inner, cond, filter(false, empty_table), orders): result is
        // empty, but BOTH relations must remain referenced (locking/RLS).
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq_expr("a", "b"),
            left: Box::new(RelExpr::scan("empty_table").filter(Expr::Const(Const::Bool(false)))),
            right: Box::new(RelExpr::scan("orders")),
        };
        let opt = crate::Optimizer::new();
        let first = opt.optimize(&expr).expect("optimize once");
        let second = opt.optimize(&first).expect("optimize twice");
        assert!(
            tables_of(&first).contains("empty_table") && tables_of(&first).contains("orders"),
            "both relations must survive an always-empty inner join, got {:?}",
            tables_of(&first)
        );
        assert_eq!(
            tables_of(&first),
            tables_of(&second),
            "optimizing twice must preserve the table set (idempotence)"
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
    fn empty_union_left_preserves_both_tables() {
        // union(all, filter(false, empty_table), orders): the empty branch
        // still references empty_table, which must NOT be dropped.
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("empty_table").filter(Expr::Const(Const::Bool(false)))),
            right: Box::new(RelExpr::scan("orders")),
        };
        let opt = crate::Optimizer::new();
        let first = opt.optimize(&expr).expect("optimize once");
        let second = opt.optimize(&first).expect("optimize twice");
        assert!(
            tables_of(&first).contains("empty_table") && tables_of(&first).contains("orders"),
            "empty union branch must keep its relation reference, got {:?}",
            tables_of(&first)
        );
        assert_eq!(
            tables_of(&first),
            tables_of(&second),
            "idempotent table set"
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
            input: Box::new(RelExpr::scan("t").filter(Expr::Const(Const::Bool(false)))),
        };
        let runner = run_with_consensus_rules(&expr);
        assert!(
            runner.egraph.number_of_classes() > 1,
            "empty relation should propagate through sort"
        );
    }

    #[test]
    fn empty_cross_join_preserves_both_tables() {
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("empty_table").filter(Expr::Const(Const::Bool(false)))),
            right: Box::new(RelExpr::scan("data")),
        };
        let opt = crate::Optimizer::new();
        let first = opt.optimize(&expr).expect("optimize once");
        let second = opt.optimize(&first).expect("optimize twice");
        assert!(
            tables_of(&first).contains("empty_table") && tables_of(&first).contains("data"),
            "both relations must survive an always-empty cross join, got {:?}",
            tables_of(&first)
        );
        assert_eq!(
            tables_of(&first),
            tables_of(&second),
            "idempotent table set"
        );
    }

    #[test]
    fn consensus_rules_count() {
        let rules = consensus_rules();
        // 2 extract-equijoin + 2 filter-null + 4 propagate-empty
        // (the relation-dropping empty-join/set-op arms were removed for
        // correctness, Codeberg #17).
        assert_eq!(
            rules.len(),
            8,
            "expected 8 consensus rules, got {}",
            rules.len()
        );
    }
}
