#![expect(
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::panic,
    reason = "diagnostic test: clarity over lint conformance"
)]
//! Diagnostic test for TPC-H Q20 nested correlated subquery.
//!
//! Q20 pattern: WHERE col IN (SELECT ... WHERE ... AND x > (SELECT agg FROM ... WHERE correlated))
//! This tests the decorrelation pass's ability to handle nested subqueries.

use ra_engine::subquery_decorrelation::{contains_subquery, decorrelate, tree_contains_subquery};
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;

const Q20: &str = "SELECT s_name, s_address \
FROM supplier, nation \
WHERE s_suppkey IN ( \
    SELECT ps_suppkey FROM partsupp \
    WHERE ps_partkey IN (SELECT p_partkey FROM part WHERE p_name LIKE 'forest%') \
    AND ps_availqty > ( \
        SELECT 0.5 * SUM(l_quantity) FROM lineitem \
        WHERE l_partkey = ps_partkey \
        AND l_suppkey = ps_suppkey \
        AND l_shipdate >= '1994-01-01' \
        AND l_shipdate < '1995-01-01' \
    ) \
) \
AND s_nationkey = n_nationkey \
AND n_name = 'CANADA' \
ORDER BY s_name";

#[test]
fn q20_parses_successfully() {
    let result = sql_to_relexpr(Q20);
    assert!(result.is_ok(), "Q20 should parse: {:?}", result.err());
}

#[test]
fn q20_contains_subqueries_before_decorrelation() {
    let relexpr = sql_to_relexpr(Q20).unwrap();
    assert!(
        tree_contains_subquery(&relexpr),
        "Q20 should contain subqueries"
    );
}

#[test]
fn q20_decorrelation_removes_all_subqueries() {
    let relexpr = sql_to_relexpr(Q20).unwrap();
    let decorrelated = decorrelate(&relexpr);
    let still_has = tree_contains_subquery(&decorrelated);
    if still_has {
        // Print the tree to diagnose where subqueries remain
        eprintln!("=== DECORRELATED TREE (still has subqueries) ===");
        print_tree(&decorrelated, 0);
        panic!(
            "After decorrelation, Q20 still has SubQuery nodes. \
             The decorrelation pass doesn't handle all nested patterns."
        );
    }
}

#[test]
fn q20_optimizes_successfully() {
    let relexpr = sql_to_relexpr(Q20).unwrap();
    let optimizer = Optimizer::new();
    match optimizer.optimize(&relexpr) {
        Ok(_) => {} // Success
        Err(e) => panic!("Q20 should optimize: {e}"),
    }
}

/// Print a simplified tree for debugging.
fn print_tree(expr: &ra_core::algebra::RelExpr, depth: usize) {
    let indent = "  ".repeat(depth);
    match expr {
        ra_core::algebra::RelExpr::Scan { table, .. } => {
            eprintln!("{indent}Scan({table})");
        }
        ra_core::algebra::RelExpr::Filter { predicate, input } => {
            let has_sq = contains_subquery(predicate);
            eprintln!("{indent}Filter(has_subquery={has_sq})");
            if has_sq {
                eprintln!("{indent}  predicate: {predicate:?}");
            }
            print_tree(input, depth + 1);
        }
        ra_core::algebra::RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let has_sq = contains_subquery(condition);
            eprintln!("{indent}{join_type:?} Join(cond_has_subquery={has_sq})");
            if has_sq {
                eprintln!("{indent}  condition: {condition:?}");
            }
            print_tree(left, depth + 1);
            print_tree(right, depth + 1);
        }
        ra_core::algebra::RelExpr::Project { input, .. } => {
            eprintln!("{indent}Project");
            print_tree(input, depth + 1);
        }
        ra_core::algebra::RelExpr::Aggregate { input, .. } => {
            eprintln!("{indent}Aggregate");
            print_tree(input, depth + 1);
        }
        ra_core::algebra::RelExpr::Sort { input, .. } => {
            eprintln!("{indent}Sort");
            print_tree(input, depth + 1);
        }
        ra_core::algebra::RelExpr::Limit { input, .. } => {
            eprintln!("{indent}Limit");
            print_tree(input, depth + 1);
        }
        other => {
            eprintln!("{indent}{:?}", std::mem::discriminant(other));
        }
    }
}
