//! Tests for recursive CTE SQL parsing.
//!
//! Validates that `sql_to_relexpr` correctly converts SQL
//! `WITH RECURSIVE` queries into `RelExpr::RecursiveCTE` nodes.
//!
//! These tests are currently ignored because the Lime grammar does not
//! yet distinguish WITH RECURSIVE from WITH and produces CTE nodes
//! instead of `RecursiveCTE` nodes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use ra_core::algebra::RelExpr;
use ra_parser::sql_to_relexpr;

// ── Basic parsing ──────────────────────────────────────────

#[test]
#[ignore = "Lime grammar does not yet produce RecursiveCTE nodes"]
fn simple_recursive_cte_parses() {
    let sql = "
        WITH RECURSIVE cnt(x) AS (
            SELECT 1
            UNION ALL
            SELECT x + 1 FROM cnt WHERE x < 10
        )
        SELECT x FROM cnt
    ";
    let expr = sql_to_relexpr(sql).expect("simple recursive CTE should parse");

    let RelExpr::RecursiveCTE {
        name,
        cycle_detection,
        ..
    } = &expr
    else {
        panic!("expected RecursiveCTE, got: {expr:?}");
    };

    assert_eq!(name, "cnt");
    assert!(
        cycle_detection.is_some(),
        "parser should set default cycle detection"
    );
    let cd = cycle_detection.as_ref().unwrap();
    assert_eq!(cd.max_depth, Some(1000));
}

#[test]
#[ignore = "Lime grammar does not yet produce RecursiveCTE nodes"]
fn recursive_cte_body_is_scan_of_cte_name() {
    let sql = "
        WITH RECURSIVE nums AS (
            SELECT 1 AS n
            UNION ALL
            SELECT n + 1 FROM nums WHERE n < 5
        )
        SELECT n FROM nums
    ";
    let expr = sql_to_relexpr(sql).expect("recursive CTE should parse");

    let RelExpr::RecursiveCTE { body, .. } = &expr else {
        panic!("expected RecursiveCTE");
    };

    // The body should reference the CTE name
    assert!(
        body.references_cte("nums"),
        "body should reference CTE 'nums'"
    );
}

#[test]
#[ignore = "Lime grammar does not yet produce RecursiveCTE nodes"]
fn recursive_cte_base_case_does_not_reference_cte() {
    let sql = "
        WITH RECURSIVE r AS (
            SELECT 1 AS val
            UNION ALL
            SELECT val + 1 FROM r WHERE val < 3
        )
        SELECT val FROM r
    ";
    let expr = sql_to_relexpr(sql).expect("recursive CTE should parse");

    let RelExpr::RecursiveCTE {
        base_case,
        recursive_case,
        ..
    } = &expr
    else {
        panic!("expected RecursiveCTE");
    };

    assert!(
        !base_case.references_cte("r"),
        "base case should not reference the CTE"
    );
    assert!(
        recursive_case.references_cte("r"),
        "recursive case should reference the CTE"
    );
}

// ── Graph traversal pattern ────────────────────────────────

#[test]
#[ignore = "Lime grammar does not yet produce RecursiveCTE nodes"]
fn graph_reachability_cte_parses() {
    let sql = "
        WITH RECURSIVE reachable(node) AS (
            SELECT dst FROM edges WHERE src = 'A'
            UNION ALL
            SELECT e.dst
            FROM edges e
            JOIN reachable r ON e.src = r.node
        )
        SELECT node FROM reachable
    ";
    let expr = sql_to_relexpr(sql).expect("graph reachability CTE should parse");

    let RelExpr::RecursiveCTE {
        name,
        recursive_case,
        ..
    } = &expr
    else {
        panic!("expected RecursiveCTE");
    };

    assert_eq!(name, "reachable");
    assert!(
        recursive_case.references_cte("reachable"),
        "recursive case should join with 'reachable'"
    );
}

// ── Running totals pattern ─────────────────────────────────

#[test]
#[ignore = "Lime grammar does not yet produce RecursiveCTE nodes"]
fn running_totals_cte_parses() {
    let sql = "
        WITH RECURSIVE running(id, total) AS (
            SELECT id, amount
            FROM transactions
            WHERE id = 1
            UNION ALL
            SELECT t.id, r.total + t.amount
            FROM transactions t
            JOIN running r ON t.id = r.id + 1
        )
        SELECT id, total FROM running
    ";
    let expr = sql_to_relexpr(sql).expect("running totals CTE should parse");

    let RelExpr::RecursiveCTE { name, .. } = &expr else {
        panic!("expected RecursiveCTE");
    };

    assert_eq!(name, "running");
}

// ── Error cases ────────────────────────────────────────────

#[test]
#[ignore = "Lime grammar does not yet distinguish UNION from UNION ALL in recursive CTEs"]
fn recursive_cte_without_union_all_fails() {
    let sql = "
        WITH RECURSIVE bad AS (
            SELECT 1
            UNION
            SELECT x + 1 FROM bad WHERE x < 10
        )
        SELECT * FROM bad
    ";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_err(),
        "recursive CTE without UNION ALL should fail"
    );
}

#[test]
fn non_recursive_with_clause_not_recursive_cte() {
    let sql = "
        WITH totals AS (
            SELECT SUM(amount) AS total FROM orders
        )
        SELECT total FROM totals
    ";
    let expr = sql_to_relexpr(sql).expect("non-recursive WITH should parse");

    // Should produce a CTE, not a RecursiveCTE
    assert!(
        !matches!(expr, RelExpr::RecursiveCTE { .. }),
        "non-recursive WITH should not produce RecursiveCTE"
    );
}

// ── Fibonacci-like pattern ─────────────────────────────────

#[test]
#[ignore = "Lime grammar does not yet produce RecursiveCTE nodes"]
fn fibonacci_cte_parses() {
    let sql = "
        WITH RECURSIVE fib(n, a, b) AS (
            SELECT 1, 0, 1
            UNION ALL
            SELECT n + 1, b, a + b FROM fib WHERE n < 20
        )
        SELECT n, a FROM fib
    ";
    let expr = sql_to_relexpr(sql).expect("fibonacci CTE should parse");

    let RelExpr::RecursiveCTE { name, .. } = &expr else {
        panic!("expected RecursiveCTE");
    };

    assert_eq!(name, "fib");
}

// ── Hierarchy traversal ────────────────────────────────────

#[test]
#[ignore = "Lime grammar does not yet produce RecursiveCTE nodes"]
fn employee_hierarchy_cte_parses() {
    let sql = "
        WITH RECURSIVE org_chart(emp_id, manager_id, level) AS (
            SELECT id, manager_id, 0
            FROM employees
            WHERE manager_id IS NULL
            UNION ALL
            SELECT e.id, e.manager_id, oc.level + 1
            FROM employees e
            JOIN org_chart oc ON e.manager_id = oc.emp_id
        )
        SELECT emp_id, level FROM org_chart
    ";
    let expr = sql_to_relexpr(sql).expect("hierarchy CTE should parse");

    let RelExpr::RecursiveCTE {
        name,
        base_case,
        recursive_case,
        ..
    } = &expr
    else {
        panic!("expected RecursiveCTE");
    };

    assert_eq!(name, "org_chart");
    assert!(!base_case.references_cte("org_chart"));
    assert!(recursive_case.references_cte("org_chart"));
}
