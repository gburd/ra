#![expect(
    clippy::expect_used,
    reason = "test code"
)]
//! Integration tests for scope-based correlation analysis.
//!
//! Verifies that the decorrelation pass works correctly with non-TPC-H
//! naming conventions, proving that the scope-based approach replaces
//! the former prefix heuristic.

use ra_engine::correlation_analysis::{
    build_scope, classify_predicates, column_in_scope, extract_equi_pairs,
};
use ra_engine::subquery_decorrelation::{decorrelate, tree_contains_subquery};
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;

/// Non-TPC-H correlated aggregate: arbitrary column names.
///
/// ```sql
/// SELECT * FROM orders o
/// WHERE o.amount > (
///     SELECT AVG(amount) FROM orders o2
///     WHERE o2.customer_id = o.customer_id
/// )
/// ```
///
/// Should decorrelate to `LeftJoin` + `GroupBy` aggregate without needing
/// prefix-based heuristics.
#[test]
fn non_tpch_correlated_aggregate_decorrelates() {
    let sql = "SELECT * FROM orders o \
               WHERE o.amount > ( \
                   SELECT AVG(amount) FROM orders o2 \
                   WHERE o2.customer_id = o.customer_id \
               )";

    let relexpr = sql_to_relexpr(sql).expect("should parse");
    assert!(
        tree_contains_subquery(&relexpr),
        "should have subquery before decorrelation"
    );

    let decorrelated = decorrelate(&relexpr);
    assert!(
        !tree_contains_subquery(&decorrelated),
        "all subqueries should be decorrelated"
    );
}

/// Qualified correlated scalar with explicit table.column references.
///
/// ```sql
/// SELECT * FROM employees e
/// WHERE e.salary > (
///     SELECT AVG(e2.salary) FROM employees e2
///     WHERE e2.department_id = e.department_id
/// )
/// ```
#[test]
fn qualified_correlated_scalar_decorrelates() {
    let sql = "SELECT * FROM employees e \
               WHERE e.salary > ( \
                   SELECT AVG(e2.salary) FROM employees e2 \
                   WHERE e2.department_id = e.department_id \
               )";

    let relexpr = sql_to_relexpr(sql).expect("should parse");
    assert!(tree_contains_subquery(&relexpr));

    let decorrelated = decorrelate(&relexpr);
    assert!(
        !tree_contains_subquery(&decorrelated),
        "qualified correlated scalar should decorrelate without prefix heuristic"
    );
}

/// Correlated EXISTS with non-standard table names.
#[test]
fn correlated_exists_non_standard_names() {
    let sql = "SELECT * FROM my_table mt \
               WHERE EXISTS ( \
                   SELECT 1 FROM other_table ot \
                   WHERE ot.ref_id = mt.id \
               )";

    let relexpr = sql_to_relexpr(sql).expect("should parse");
    let decorrelated = decorrelate(&relexpr);
    assert!(!tree_contains_subquery(&decorrelated));
}

/// Correlated NOT IN with arbitrary naming.
#[test]
fn correlated_not_in_arbitrary_names() {
    let sql = "SELECT * FROM products p \
               WHERE p.category_id NOT IN ( \
                   SELECT dc.category_id FROM discontinued_categories dc \
               )";

    let relexpr = sql_to_relexpr(sql).expect("should parse");
    let decorrelated = decorrelate(&relexpr);
    assert!(!tree_contains_subquery(&decorrelated));
}

/// Regression: TPC-H Q2 correlated scalar (uses standard TPC-H names).
/// Ensures the new scope-based approach doesn't break existing queries.
#[test]
fn tpch_q2_style_correlated_scalar() {
    let sql = "SELECT s_acctbal, s_name, n_name, p_partkey, p_mfgr, \
                      s_address, s_phone, s_comment \
               FROM part, supplier, partsupp, nation, region \
               WHERE p_partkey = ps_partkey \
               AND s_suppkey = ps_suppkey \
               AND p_size = 15 \
               AND s_nationkey = n_nationkey \
               AND n_regionkey = r_regionkey \
               AND r_name = 'EUROPE' \
               AND ps_supplycost = ( \
                   SELECT MIN(ps_supplycost) FROM partsupp, supplier, nation, region \
                   WHERE p_partkey = ps_partkey \
                   AND s_suppkey = ps_suppkey \
                   AND s_nationkey = n_nationkey \
                   AND n_regionkey = r_regionkey \
                   AND r_name = 'EUROPE' \
               ) \
               ORDER BY s_acctbal DESC, n_name, s_name, p_partkey";

    let relexpr = sql_to_relexpr(sql).expect("should parse");
    let decorrelated = decorrelate(&relexpr);
    // The scalar subquery should be decorrelated (either via correlated path
    // or uncorrelated CrossJoin path)
    assert!(
        !tree_contains_subquery(&decorrelated),
        "TPC-H Q2-style should still decorrelate"
    );
}

/// Regression: TPC-H Q20 nested correlated subquery still works.
#[test]
fn tpch_q20_regression() {
    let sql = "SELECT s_name, s_address \
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

    let relexpr = sql_to_relexpr(sql).expect("should parse");
    let decorrelated = decorrelate(&relexpr);
    assert!(
        !tree_contains_subquery(&decorrelated),
        "TPC-H Q20 regression: should still decorrelate"
    );
}

/// Verify scope-based analysis correctly identifies inner vs outer columns.
#[test]
fn scope_analysis_unit_integration() {
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    // Build: Scan("employees" alias "e2")
    let inner_rel = RelExpr::Scan {
        table: "employees".to_owned(),
        alias: Some("e2".to_owned()),
    };

    let scope = build_scope(&inner_rel);

    // e2.department_id should be in scope
    assert!(column_in_scope(
        &ColumnRef::qualified("e2", "department_id"),
        &scope
    ));
    // employees.department_id should also be in scope
    assert!(column_in_scope(
        &ColumnRef::qualified("employees", "department_id"),
        &scope
    ));
    // e.department_id should NOT be in scope
    assert!(!column_in_scope(
        &ColumnRef::qualified("e", "department_id"),
        &scope
    ));

    // Classify: e2.department_id = e.department_id
    let pred = Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::qualified("e2", "department_id"))),
        right: Box::new(Expr::Column(ColumnRef::qualified("e", "department_id"))),
    };

    let (corr, local) = classify_predicates(std::slice::from_ref(&pred), &scope);
    assert_eq!(corr.len(), 1, "should identify as correlation predicate");
    assert_eq!(local.len(), 0);

    let pairs = extract_equi_pairs(&corr, &scope);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].0, ColumnRef::qualified("e2", "department_id"));
    assert_eq!(pairs[0].1, ColumnRef::qualified("e", "department_id"));
}

/// Full optimization pipeline passes with non-TPC-H correlated query.
#[test]
fn optimizer_handles_non_tpch_correlation() {
    let sql = "SELECT * FROM orders o \
               WHERE o.amount > ( \
                   SELECT AVG(amount) FROM orders o2 \
                   WHERE o2.customer_id = o.customer_id \
               )";

    let relexpr = sql_to_relexpr(sql).expect("should parse");
    let optimizer = Optimizer::new();
    let result = optimizer.optimize(&relexpr);
    assert!(
        result.is_ok(),
        "optimizer should handle non-TPC-H correlation: {:?}",
        result.err()
    );
}
