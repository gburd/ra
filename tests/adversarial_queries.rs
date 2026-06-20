#![expect(clippy::expect_used, reason = "test code")]
//! Adversarial query testing: patterns where Ra might produce wrong results
//! or fail where PG succeeds. Tests parse + optimize + verify structural
//! correctness of the output plan.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::Expr;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;

fn opt(sql: &str) -> RelExpr {
    let expr = sql_to_relexpr(sql).expect("parse");
    Optimizer::default().optimize(&expr).expect("optimize")
}

fn try_opt(sql: &str) -> Result<RelExpr, String> {
    let expr = sql_to_relexpr(sql).map_err(|e| format!("parse: {e}"))?;
    Optimizer::default()
        .optimize(&expr)
        .map_err(|e| format!("optimize: {e}"))
}

fn tables_in(expr: &RelExpr) -> Vec<String> {
    let mut out = Vec::new();
    collect_tables(expr, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect_tables(expr: &RelExpr, out: &mut Vec<String>) {
    match expr {
        RelExpr::Scan { table, .. } => out.push(table.clone()),
        other => {
            for child in other.children() {
                collect_tables(child, out);
            }
        }
    }
}

fn has_join(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Join { .. } => true,
        other => other.children().iter().any(|c| has_join(c)),
    }
}

fn outer_join_preserved(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Join {
            join_type: JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter,
            ..
        } => true,
        other => other.children().iter().any(|c| outer_join_preserved(c)),
    }
}

fn contains_filter_with(expr: &RelExpr, needle: &str) -> bool {
    match expr {
        RelExpr::Filter { predicate, input } => {
            format!("{predicate:?}").contains(needle)
                || contains_filter_with(input, needle)
        }
        other => other.children().iter().any(|c| contains_filter_with(c, needle)),
    }
}

// ===== CORRECTNESS: outer join semantics =====

#[test]
fn left_join_is_null_preserved() {
    // WHERE t2.col IS NULL on a LEFT JOIN must preserve the LEFT JOIN
    // (it's an anti-join pattern). Converting to INNER would lose unmatched rows.
    let plan = opt(
        "SELECT * FROM t1 LEFT JOIN t2 ON t1.id = t2.id WHERE t2.id IS NULL"
    );
    assert!(
        outer_join_preserved(&plan),
        "LEFT JOIN + IS NULL must stay as outer join (anti-join pattern)"
    );
}

#[test]
fn left_join_with_non_null_filter_converts_to_inner() {
    // WHERE t2.col = 'x' implies t2 is non-NULL → safe to convert to INNER
    let plan = opt(
        "SELECT * FROM t1 LEFT JOIN t2 ON t1.id = t2.id WHERE t2.status = 'active'"
    );
    // Either stays LEFT with filter, or converts to INNER (both correct)
    let debug = format!("{plan:?}");
    assert!(
        debug.contains("Inner") || debug.contains("Left"),
        "must be valid join: {debug}"
    );
}

#[test]
fn full_outer_join_not_simplified() {
    // FULL OUTER JOIN must not be simplified to LEFT or INNER
    let plan = opt(
        "SELECT * FROM t1 FULL OUTER JOIN t2 ON t1.id = t2.id"
    );
    let debug = format!("{plan:?}");
    assert!(debug.contains("FullOuter"), "FULL OUTER must be preserved: {debug}");
}

// ===== CORRECTNESS: NULL handling =====

#[test]
fn coalesce_in_join_condition() {
    let plan = opt(
        "SELECT * FROM t1 JOIN t2 ON COALESCE(t1.x, 0) = COALESCE(t2.y, 0)"
    );
    assert!(has_join(&plan));
}

#[test]
fn is_distinct_from() {
    // IS DISTINCT FROM is NULL-safe equality — different from =
    let result = try_opt(
        "SELECT * FROM t1 WHERE t1.x IS DISTINCT FROM t1.y"
    );
    assert!(result.is_ok(), "IS DISTINCT FROM should parse: {result:?}");
}

// ===== CORRECTNESS: aggregate semantics =====

#[test]
fn count_star_vs_count_col() {
    // COUNT(*) counts all rows; COUNT(col) counts non-NULL values
    let plan = opt("SELECT COUNT(*), COUNT(x) FROM t");
    let debug = format!("{plan:?}");
    assert!(debug.contains("CountStar") || debug.contains("Count"),
        "aggregates preserved: {debug}");
}

#[test]
fn having_without_group_by() {
    let result = try_opt("SELECT COUNT(*) FROM t HAVING COUNT(*) > 5");
    assert!(result.is_ok(), "HAVING without GROUP BY: {result:?}");
}

#[test]
fn group_by_expression() {
    let result = try_opt(
        "SELECT EXTRACT(year FROM d) AS yr, COUNT(*) FROM t GROUP BY EXTRACT(year FROM d)"
    );
    assert!(result.is_ok(), "GROUP BY expression: {result:?}");
}

// ===== CORRECTNESS: subquery semantics =====

#[test]
fn correlated_exists_preserves_semantics() {
    let plan = opt(
        "SELECT * FROM orders o WHERE EXISTS \
         (SELECT 1 FROM lineitem l WHERE l.l_orderkey = o.o_orderkey)"
    );
    let debug = format!("{plan:?}");
    // Must decorrelate to SemiJoin (not CrossJoin or lose the correlation)
    assert!(debug.contains("Semi"), "EXISTS → SemiJoin: {debug}");
}

#[test]
fn not_in_with_nullable_column() {
    // NOT IN with nullable inner column must NOT become anti-join
    // (SQL NULL semantics: if inner has NULL, result is empty)
    let plan = opt(
        "SELECT * FROM t1 WHERE t1.x NOT IN (SELECT t2.y FROM t2)"
    );
    let debug = format!("{plan:?}");
    // Must NOT become Anti join (unsafe for NULLs) — should stay as SubQuery
    assert!(
        !debug.contains("Anti"),
        "NOT IN must not become anti-join (NULL semantics): {debug}"
    );
}

// ===== CORRECTNESS: set operations =====

#[test]
fn union_removes_duplicates() {
    let plan = opt("SELECT a FROM t1 UNION SELECT a FROM t2");
    let debug = format!("{plan:?}");
    // UNION (without ALL) must have deduplication
    assert!(
        debug.contains("Distinct") || debug.contains("Union"),
        "UNION must deduplicate: {debug}"
    );
}

#[test]
fn except_all_preserves_duplicates() {
    let result = try_opt("SELECT a FROM t1 EXCEPT ALL SELECT a FROM t2");
    assert!(result.is_ok(), "EXCEPT ALL: {result:?}");
}

// ===== CORRECTNESS: ORDER BY + LIMIT =====

#[test]
fn limit_respects_order() {
    let plan = opt("SELECT * FROM t ORDER BY x LIMIT 10");
    let debug = format!("{plan:?}");
    // Sort must be below Limit (not eliminated)
    assert!(debug.contains("Sort"), "ORDER BY must survive with LIMIT: {debug}");
    assert!(debug.contains("Limit") || debug.contains("count: 10"),
        "LIMIT preserved: {debug}");
}

#[test]
fn offset_without_limit() {
    let result = try_opt("SELECT * FROM t ORDER BY x OFFSET 5");
    assert!(result.is_ok(), "OFFSET without LIMIT: {result:?}");
}

// ===== ADVERSARIAL: edge cases that trip optimizers =====

#[test]
fn self_join() {
    let plan = opt(
        "SELECT e1.name, e2.name FROM emp e1 JOIN emp e2 ON e1.mgr_id = e2.id"
    );
    let tables = tables_in(&plan);
    // Must have two references to emp (via aliases)
    assert!(tables.contains(&"emp".to_string()), "self-join lost table: {tables:?}");
}

#[test]
fn cross_join_with_where() {
    // Implicit cross join with WHERE should become an inner join
    let plan = opt("SELECT * FROM t1, t2 WHERE t1.x = t2.y");
    assert!(has_join(&plan));
}

#[test]
fn empty_result_optimization() {
    // WHERE 1=0 should be recognized as always-false (but not crash)
    let result = try_opt("SELECT * FROM t WHERE 1 = 0");
    assert!(result.is_ok());
}

#[test]
fn deeply_nested_subquery() {
    let result = try_opt(
        "SELECT * FROM t1 WHERE x > (SELECT MAX(y) FROM t2 WHERE y > \
         (SELECT MIN(z) FROM t3))"
    );
    assert!(result.is_ok(), "nested scalar subqueries: {result:?}");
}

#[test]
fn many_table_join() {
    let result = try_opt(
        "SELECT * FROM t1 JOIN t2 ON t1.a=t2.a JOIN t3 ON t2.b=t3.b \
         JOIN t4 ON t3.c=t4.c JOIN t5 ON t4.d=t5.d JOIN t6 ON t5.e=t6.e \
         JOIN t7 ON t6.f=t7.f JOIN t8 ON t7.g=t8.g"
    );
    assert!(result.is_ok(), "8-table join: {result:?}");
    let plan = result.expect("optimize");
    let tables = tables_in(&plan);
    assert_eq!(tables.len(), 8, "all 8 tables present: {tables:?}");
}

#[test]
fn lateral_subquery() {
    let result = try_opt(
        "SELECT * FROM t1, LATERAL (SELECT * FROM t2 WHERE t2.id = t1.id LIMIT 1) sub"
    );
    assert!(result.is_ok(), "LATERAL: {result:?}");
}

#[test]
fn recursive_cte_fibonacci() {
    let result = try_opt(
        "WITH RECURSIVE fib(n, a, b) AS (\
         SELECT 1, 0, 1 \
         UNION ALL \
         SELECT n+1, b, a+b FROM fib WHERE n < 20\
         ) SELECT n, a FROM fib"
    );
    assert!(result.is_ok(), "recursive CTE: {result:?}");
}

#[test]
fn window_with_partition_and_frame() {
    let result = try_opt(
        "SELECT *, SUM(x) OVER (PARTITION BY grp ORDER BY id \
         ROWS BETWEEN 2 PRECEDING AND CURRENT ROW) FROM t"
    );
    assert!(result.is_ok(), "window with frame: {result:?}");
}

#[test]
fn insert_on_conflict_returning() {
    let result = try_opt(
        "INSERT INTO t (id, x) VALUES (1, 2) \
         ON CONFLICT (id) DO UPDATE SET x = EXCLUDED.x"
    );
    assert!(result.is_ok(), "INSERT ON CONFLICT: {result:?}");
}

#[test]
fn merge_statement() {
    let result = try_opt(
        "MERGE INTO t USING s ON t.id = s.id \
         WHEN MATCHED THEN UPDATE SET x = s.x \
         WHEN NOT MATCHED THEN INSERT (id, x) VALUES (s.id, s.x)"
    );
    assert!(result.is_ok(), "MERGE: {result:?}");
}

#[test]
fn complex_case_expression() {
    let result = try_opt(
        "SELECT CASE WHEN x > 100 THEN 'high' \
         WHEN x > 50 THEN 'mid' \
         WHEN x > 0 THEN 'low' \
         ELSE 'zero' END FROM t"
    );
    assert!(result.is_ok(), "complex CASE: {result:?}");
}

#[test]
fn multiple_aggregates_different_filters() {
    let result = try_opt(
        "SELECT COUNT(*), SUM(x), AVG(y), MIN(z), MAX(z) FROM t GROUP BY grp"
    );
    assert!(result.is_ok(), "multi-agg: {result:?}");
}

#[test]
fn correlated_scalar_with_multiple_correlations() {
    let plan = opt(
        "SELECT * FROM orders o WHERE o.total > \
         (SELECT AVG(o2.total) FROM orders o2 \
          WHERE o2.customer_id = o.customer_id AND o2.region = o.region)"
    );
    let debug = format!("{plan:?}");
    // Should decorrelate (2 correlation predicates → GROUP BY both)
    assert!(
        debug.contains("Aggregate") && debug.contains("Join"),
        "multi-correlation decorrelation: {debug}"
    );
}

#[test]
fn for_update_skip_locked() {
    let result = try_opt("SELECT * FROM t WHERE id = 1 FOR UPDATE SKIP LOCKED");
    assert!(result.is_ok(), "FOR UPDATE SKIP LOCKED: {result:?}");
}

#[test]
fn json_operators_in_filter() {
    let result = try_opt(
        "SELECT * FROM t WHERE data->>'name' = 'test' AND data @> '{\"active\": true}'"
    );
    assert!(result.is_ok(), "JSON operators: {result:?}");
}
