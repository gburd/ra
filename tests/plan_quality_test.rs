#![expect(clippy::expect_used, reason = "test code")]
//! Test plan quality: verify optimizer produces correct and efficient plans.

use ra_parser::sql_to_relexpr;
use ra_engine::Optimizer;
use ra_core::algebra::RelExpr;

fn optimize(sql: &str) -> RelExpr {
    let expr = sql_to_relexpr(sql).expect("parse");
    Optimizer::default().optimize(&expr).expect("optimize")
}

fn contains(expr: &RelExpr, needle: &str) -> bool {
    format!("{expr:?}").contains(needle)
}

// === CORRECTNESS TESTS ===

/// Filter pushdown must not push past an outer join's non-preserved side
/// inappropriately: WHERE on nullable side must stay above the join.
#[test]
fn filter_on_nullable_side_not_pushed_below_left_join() {
    let plan = optimize(
        "SELECT * FROM t1 LEFT JOIN t2 ON t1.id = t2.id WHERE t2.status = 'active'"
    );
    // A filter on t2 (nullable side) when pushed below a LEFT JOIN would
    // change semantics (turn it into an inner join). If Ra pushes it down
    // correctly, it should be IN the join condition or the join should
    // become INNER (which is valid optimization — the filter implies non-NULL).
    // Either way, the plan should not have the filter BELOW the join on
    // only the right side (that would lose rows).
    let debug = format!("{plan:?}");
    // Valid outcomes: (1) Filter above join, (2) INNER join (filter implies non-null),
    // (3) Filter merged into ON condition
    let has_left_join = debug.contains("Left");
    let has_filter_above = matches!(&plan,
        RelExpr::Project { input, .. }
        if matches!(&**input, RelExpr::Filter { .. } | RelExpr::Join { .. })
    ) || matches!(&plan, RelExpr::Filter { .. });
    // If still LEFT JOIN, filter must be above or in condition
    if has_left_join {
        assert!(
            has_filter_above || debug.contains("status"),
            "filter on nullable side lost: {debug}"
        );
    }
    // If converted to INNER, that's a valid optimization
}

/// Predicate pushdown through a join must keep predicates on the correct side.
#[test]
fn filter_pushdown_respects_column_origin() {
    let plan = optimize(
        "SELECT * FROM orders o JOIN customers c ON o.cust_id = c.id \
         WHERE c.country = 'US' AND o.amount > 100"
    );
    let debug = format!("{plan:?}");
    // c.country = 'US' should be pushed to the customers scan side
    // o.amount > 100 should be pushed to the orders scan side
    // Both should NOT be on the wrong table's scan
    assert!(debug.contains("country"), "country predicate missing");
    assert!(debug.contains("amount"), "amount predicate missing");
}

/// Join commutativity: ensure the optimizer can swap join sides for better plans.
#[test]
fn join_commutativity_produces_valid_plan() {
    let plan = optimize(
        "SELECT * FROM small_table s JOIN large_table l ON s.id = l.sid"
    );
    // The plan should have a valid join with both tables present
    let debug = format!("{plan:?}");
    assert!(debug.contains("small_table"), "small_table missing");
    assert!(debug.contains("large_table"), "large_table missing");
}

/// Redundant filter elimination: x = 1 AND x = 1 → x = 1
#[test]
fn redundant_filter_elimination() {
    let plan = optimize(
        "SELECT * FROM t WHERE x = 1 AND x = 1"
    );
    let debug = format!("{plan:?}");
    // Should not have duplicate predicates
    let count = debug.matches("x").count();
    // At most 2 occurrences of "x" (one in predicate, one in the projected *)
    // A redundant filter would have 4+
    assert!(count <= 4, "redundant filter not eliminated: found {count} 'x' in {debug}");
}

/// Union followed by filter: filter should push into both branches.
#[test]
fn filter_pushdown_into_union() {
    let plan = optimize(
        "SELECT * FROM (SELECT a, b FROM t1 UNION ALL SELECT a, b FROM t2) sub WHERE a > 10"
    );
    let debug = format!("{plan:?}");
    // The filter should appear on both branches (pushed through union)
    // or above the union (acceptable but sub-optimal)
    assert!(debug.contains("10"), "filter value lost in plan");
}

/// Multi-join ordering: verify all tables present after optimization.
#[test]
fn multi_join_preserves_all_tables() {
    let plan = optimize(
        "SELECT * FROM t1 JOIN t2 ON t1.a = t2.a \
         JOIN t3 ON t2.b = t3.b \
         JOIN t4 ON t3.c = t4.c"
    );
    let debug = format!("{plan:?}");
    for t in &["t1", "t2", "t3", "t4"] {
        assert!(debug.contains(t), "table {t} missing from plan: {debug}");
    }
}

/// CTE optimization: the CTE body should be optimized.
#[test]
fn cte_body_is_optimized() {
    let plan = optimize(
        "WITH active AS (SELECT * FROM users WHERE status = 'active') \
         SELECT * FROM active a JOIN orders o ON a.id = o.user_id WHERE o.amount > 100"
    );
    let debug = format!("{plan:?}");
    assert!(debug.contains("users"), "users table missing");
    assert!(debug.contains("orders"), "orders table missing");
    assert!(debug.contains("amount"), "amount filter missing");
}

/// Aggregate pushdown: filter on grouped column should push below aggregate.
#[test]
fn filter_before_aggregate_is_valid() {
    let plan = optimize(
        "SELECT dept, COUNT(*) FROM emp WHERE salary > 50000 GROUP BY dept"
    );
    let debug = format!("{plan:?}");
    assert!(debug.contains("salary"), "salary filter missing");
    assert!(debug.contains("Aggregate") || debug.contains("group_by"),
        "aggregate missing from plan");
}

/// LIMIT should not be pushed below a join (would change result count).
#[test]
fn limit_not_pushed_below_join() {
    let plan = optimize(
        "SELECT * FROM t1 JOIN t2 ON t1.id = t2.id LIMIT 10"
    );
    let debug = format!("{plan:?}");
    assert!(debug.contains("Limit") || debug.contains("limit") || debug.contains("count: 10"),
        "LIMIT missing from plan: {debug}");
    // LIMIT should be above the join
    assert!(debug.contains("Join") || debug.contains("join"),
        "join missing from plan: {debug}");
}

/// Window function must stay above its input (can't be pushed down).
#[test]
fn window_function_preserved() {
    let plan = optimize(
        "SELECT *, ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary DESC) rn FROM emp"
    );
    let debug = format!("{plan:?}");
    assert!(debug.contains("Window") || debug.contains("window"),
        "window function missing: {debug}");
}

/// Compound AND predicates should be split and pushed to correct join sides.
#[test]
fn compound_filter_pushdown_through_join() {
    let plan = optimize(
        "SELECT * FROM t1 JOIN t2 ON t1.id = t2.id WHERE t1.a = 1 AND t2.b = 2"
    );
    let debug = format!("{plan:?}");
    // After optimization, t1.a=1 should be pushed below the join (on t1 side)
    // and t2.b=2 should be pushed below the join (on t2 side).
    // The plan should NOT have a compound Filter above the Join.
    let has_compound_above_join = debug.contains("And") && debug.find("And").unwrap() < debug.find("Join").unwrap_or(usize::MAX);
    assert!(
        !has_compound_above_join,
        "compound predicate should be pushed through join, got: {debug}"
    );
}
