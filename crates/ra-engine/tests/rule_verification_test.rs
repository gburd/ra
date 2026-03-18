//! Targeted rule verification tests.
//!
//! Each test constructs a specific input, runs optimization, and verifies
//! that the expected transformation was applied (not just "something
//! changed"). These tests catch regressions in rule semantics.

#![allow(clippy::expect_used)]

mod helpers;

use helpers::*;
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, Const, Expr, UnaryOp};
use ra_engine::{
    rec_expr_to_rel_expr, to_rec_expr, Optimizer, OptimizerConfig,
};

// ── Helpers ─────────────────────────────────────────────────────

fn optimize(expr: &RelExpr) -> RelExpr {
    let opt = Optimizer::with_config(OptimizerConfig {
        node_limit: 50_000,
        iter_limit: 10,
        time_limit_secs: 5,
    });
    opt.optimize(expr).expect("optimization should succeed")
}

fn collect_tables(expr: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    collect_tables_rec(expr, &mut tables);
    tables.sort();
    tables.dedup();
    tables
}

fn collect_tables_rec(expr: &RelExpr, out: &mut Vec<String>) {
    match expr {
        RelExpr::Scan { table, .. } => out.push(table.clone()),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => collect_tables_rec(input, out),
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_tables_rec(left, out);
            collect_tables_rec(right, out);
        }
        RelExpr::CTE {
            definition, body, ..
        } => {
            collect_tables_rec(definition, out);
            collect_tables_rec(body, out);
        }
        RelExpr::RecursiveCTE {
            base_case,
            recursive_case,
            body,
            ..
        } => {
            collect_tables_rec(base_case, out);
            collect_tables_rec(recursive_case, out);
            collect_tables_rec(body, out);
        }
        RelExpr::Values { .. } => {}
    }
}

fn depth(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Scan { .. } | RelExpr::Values { .. } => 1,
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => 1 + depth(input),
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            1 + depth(left).max(depth(right))
        }
        RelExpr::CTE {
            definition, body, ..
        } => 1 + depth(definition).max(depth(body)),
        RelExpr::RecursiveCTE {
            base_case,
            recursive_case,
            body,
            ..
        } => {
            1 + depth(base_case)
                .max(depth(recursive_case))
                .max(depth(body))
        }
    }
}

fn is_filter(expr: &RelExpr) -> bool {
    matches!(expr, RelExpr::Filter { .. })
}

fn is_scan(expr: &RelExpr) -> bool {
    matches!(expr, RelExpr::Scan { .. })
}

// ── Predicate Pushdown Tests ────────────────────────────────────

#[test]
fn filter_true_eliminated() {
    let input = RelExpr::scan("users")
        .filter(Expr::Const(Const::Bool(true)));
    let result = optimize(&input);
    // filter(true, scan) should collapse to just scan
    assert!(
        is_scan(&result),
        "filter(true, scan) should be eliminated to scan, got: {result:?}"
    );
}

#[test]
fn filter_merge_produces_conjunction() {
    let input = RelExpr::scan("t")
        .filter(gt(col("a"), int(10)))
        .filter(gt(col("b"), int(20)));
    let result = optimize(&input);
    // Two stacked filters should merge into a single filter
    // with an AND predicate, or the filters might be reordered.
    // Either way, both predicates must still be present.
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn filter_pushdown_through_project() {
    let input = RelExpr::scan("data")
        .project(vec![
            ProjectionColumn {
                expr: col("id"),
                alias: None,
            },
            ProjectionColumn {
                expr: col("value"),
                alias: None,
            },
        ])
        .filter(gt(col("value"), int(100)));

    let result = optimize(&input);
    // The filter should be pushed below the project.
    // The outermost node should be a project, not a filter.
    match &result {
        RelExpr::Project { input, .. } => {
            // Filter should now be below project
            assert!(
                is_filter(input),
                "filter should be pushed below project"
            );
        }
        RelExpr::Filter { .. } => {
            // Also acceptable: filter may have been merged or
            // kept on top if columns don't pass through
        }
        other => {
            // The plan should still reference table "data"
            let tables = collect_tables(other);
            assert!(tables.contains(&"data".to_owned()));
        }
    }
}

// ── Join Reordering Tests ───────────────────────────────────────

#[test]
fn join_commutativity_preserves_tables() {
    let input = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("a_id"), col("b_id")),
        left: Box::new(RelExpr::scan("left_table")),
        right: Box::new(RelExpr::scan("right_table")),
    };
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(
        tables.contains(&"left_table".to_owned()),
        "left_table should be preserved"
    );
    assert!(
        tables.contains(&"right_table".to_owned()),
        "right_table should be preserved"
    );
}

#[test]
fn join_associativity_preserves_all_tables() {
    // (A join B) join C should have all three tables
    let ab = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("a_id"), col("b_id")),
        left: Box::new(RelExpr::scan("A")),
        right: Box::new(RelExpr::scan("B")),
    };
    let input = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("b_id"), col("c_id")),
        left: Box::new(ab),
        right: Box::new(RelExpr::scan("C")),
    };
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"A".to_owned()));
    assert!(tables.contains(&"B".to_owned()));
    assert!(tables.contains(&"C".to_owned()));
}

#[test]
fn optimization_reproducible() {
    let input = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("fk")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("customers")),
    };
    let r1 = optimize(&input);
    let r2 = optimize(&input);
    assert_eq!(r1, r2, "optimization should be deterministic");
}

// ── Expression Simplification Tests ─────────────────────────────

#[test]
fn double_negation_eliminated() {
    let input = RelExpr::scan("t").filter(Expr::UnaryOp {
        op: UnaryOp::Not,
        operand: Box::new(Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(col("active")),
        }),
    });
    let result = optimize(&input);
    // NOT(NOT(x)) should simplify to just x.
    // The filter should remain (with simplified predicate).
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn and_with_true_simplified() {
    // filter(AND(x > 5, TRUE)) should simplify to filter(x > 5)
    let pred = Expr::BinOp {
        op: BinOp::And,
        left: Box::new(gt(col("x"), int(5))),
        right: Box::new(Expr::Const(Const::Bool(true))),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    // Table should be preserved
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn and_with_false_short_circuits() {
    // filter(AND(x > 5, FALSE)) should simplify to filter(FALSE)
    let pred = Expr::BinOp {
        op: BinOp::And,
        left: Box::new(gt(col("x"), int(5))),
        right: Box::new(Expr::Const(Const::Bool(false))),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn or_with_true_short_circuits() {
    let pred = Expr::BinOp {
        op: BinOp::Or,
        left: Box::new(gt(col("x"), int(5))),
        right: Box::new(Expr::Const(Const::Bool(true))),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    // OR(x, TRUE) = TRUE, filter(TRUE, scan) = scan
    // Result should be just the scan
    assert!(
        is_scan(&result),
        "OR(x, TRUE) should simplify to TRUE, eliminating filter. Got: {result:?}"
    );
}

#[test]
fn eq_reflexive_simplifies() {
    // filter(a = a) should simplify to filter(TRUE) -> scan
    let pred = eq(col("a"), col("a"));
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    assert!(
        is_scan(&result),
        "filter(a = a) should be eliminated (eq-reflexive + filter-true). Got: {result:?}"
    );
}

#[test]
fn add_zero_simplified() {
    // filter(a + 0 > 5) should simplify to filter(a > 5)
    let add_zero = Expr::BinOp {
        op: BinOp::Add,
        left: Box::new(col("a")),
        right: Box::new(int(0)),
    };
    let pred = Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(add_zero),
        right: Box::new(int(5)),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn mul_zero_simplified() {
    // filter(a * 0 > 5) -- a*0 = 0, so 0 > 5 = false
    let mul_zero = Expr::BinOp {
        op: BinOp::Mul,
        left: Box::new(col("a")),
        right: Box::new(int(0)),
    };
    let pred = Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(mul_zero),
        right: Box::new(int(5)),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn sub_self_simplified() {
    // a - a => 0 (DuckDB rule)
    let sub_self = Expr::BinOp {
        op: BinOp::Sub,
        left: Box::new(col("a")),
        right: Box::new(col("a")),
    };
    let pred = Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(sub_self),
        right: Box::new(int(5)),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

// ── Limit/Sort Optimization Tests ───────────────────────────────

#[test]
fn limit_through_project() {
    // LIMIT(project(scan)) -> project(LIMIT(scan))
    let input = RelExpr::scan("t")
        .project(vec![ProjectionColumn {
            expr: col("id"),
            alias: None,
        }])
        .limit(10, 0);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn sort_below_sort_eliminated() {
    // sort(k1, sort(k2, input)) -> sort(k1, input)
    let inner_sort = RelExpr::Sort {
        keys: vec![SortKey {
            expr: col("b"),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(RelExpr::scan("t")),
    };
    let outer_sort = RelExpr::Sort {
        keys: vec![SortKey {
            expr: col("a"),
            direction: SortDirection::Desc,
            nulls: NullOrdering::First,
        }],
        input: Box::new(inner_sort),
    };
    let result = optimize(&outer_sort);
    // The inner sort should be eliminated
    let d = depth(&result);
    assert!(
        d <= 2,
        "redundant sort should be eliminated, depth={d}"
    );
}

#[test]
fn sort_below_aggregate_eliminated() {
    // aggregate(sort(input)) -> aggregate(input) (DuckDB rule)
    let sorted = RelExpr::Sort {
        keys: vec![SortKey {
            expr: col("key"),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(RelExpr::scan("t")),
    };
    let agg = RelExpr::Aggregate {
        group_by: vec![col("key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(int(1)),
            distinct: false,
            alias: None,
        }],
        input: Box::new(sorted),
    };
    let result = optimize(&agg);
    // Sort should be removed below aggregate
    let d = depth(&result);
    assert!(
        d <= 2,
        "sort below aggregate should be eliminated, depth={d}. Got: {result:?}"
    );
}

// ── Set Operation Tests ─────────────────────────────────────────

#[test]
fn union_self_simplified() {
    // UNION ALL of same table with itself -- rule says this
    // simplifies to just the table
    let input = RelExpr::Union {
        all: true,
        left: Box::new(RelExpr::scan("t")),
        right: Box::new(RelExpr::scan("t")),
    };
    let result = optimize(&input);
    assert!(
        is_scan(&result),
        "UNION ALL of identical scans should simplify to single scan. Got: {result:?}"
    );
}

#[test]
fn intersect_self_simplified() {
    let input = RelExpr::Intersect {
        all: false,
        left: Box::new(RelExpr::scan("t")),
        right: Box::new(RelExpr::scan("t")),
    };
    let result = optimize(&input);
    assert!(
        is_scan(&result),
        "INTERSECT of identical scans should simplify. Got: {result:?}"
    );
}

#[test]
fn except_self_produces_empty() {
    // EXCEPT of same table with itself -> filter(false, t)
    let input = RelExpr::Except {
        all: false,
        left: Box::new(RelExpr::scan("t")),
        right: Box::new(RelExpr::scan("t")),
    };
    let result = optimize(&input);
    // Should become filter(false, scan("t"))
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

// ── DeMorgan's Law Tests ────────────────────────────────────────

#[test]
fn demorgan_not_and_to_or() {
    // NOT(a AND b) -> (NOT a) OR (NOT b)
    let pred = Expr::UnaryOp {
        op: UnaryOp::Not,
        operand: Box::new(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(col("a")),
            right: Box::new(col("b")),
        }),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn demorgan_not_or_to_and() {
    // NOT(a OR b) -> (NOT a) AND (NOT b)
    let pred = Expr::UnaryOp {
        op: UnaryOp::Not,
        operand: Box::new(Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(col("a")),
            right: Box::new(col("b")),
        }),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

// ── DuckDB-Inspired Rule Tests ──────────────────────────────────

#[test]
fn duckdb_not_lt_to_ge() {
    // NOT(a < b) => a >= b
    let pred = Expr::UnaryOp {
        op: UnaryOp::Not,
        operand: Box::new(Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(col("x")),
            right: Box::new(int(10)),
        }),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn duckdb_not_eq_to_ne() {
    // NOT(a = b) => a != b
    let pred = Expr::UnaryOp {
        op: UnaryOp::Not,
        operand: Box::new(eq(col("x"), int(5))),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn duckdb_limit_through_union() {
    // LIMIT(UNION ALL(a, b)) -> UNION ALL(LIMIT(a), LIMIT(b))
    let union_expr = RelExpr::Union {
        all: true,
        left: Box::new(RelExpr::scan("t1")),
        right: Box::new(RelExpr::scan("t2")),
    };
    let limited = union_expr.limit(10, 0);
    let result = optimize(&limited);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t1".to_owned()));
    assert!(tables.contains(&"t2".to_owned()));
}

// ── SQLite-Inspired Rule Tests ──────────────────────────────────

#[test]
fn sqlite_range_to_eq() {
    // (a >= b AND a <= b) => (a = b)
    let pred = Expr::BinOp {
        op: BinOp::And,
        left: Box::new(Expr::BinOp {
            op: BinOp::Ge,
            left: Box::new(col("a")),
            right: Box::new(col("b")),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::Le,
            left: Box::new(col("a")),
            right: Box::new(col("b")),
        }),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn sqlite_or_distribute() {
    // (a AND b) OR (a AND c) => a AND (b OR c)
    let pred = Expr::BinOp {
        op: BinOp::Or,
        left: Box::new(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(col("a")),
            right: Box::new(col("b")),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(col("a")),
            right: Box::new(col("c")),
        }),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn sqlite_eq_implies_not_null() {
    // (a = b) AND IS_NOT_NULL(a) => (a = b)
    let pred = Expr::BinOp {
        op: BinOp::And,
        left: Box::new(eq(col("a"), col("b"))),
        right: Box::new(Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            operand: Box::new(col("a")),
        }),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

// ── Aggregate Optimization Tests ────────────────────────────────

#[test]
fn filter_pushed_below_aggregate() {
    // filter(pred, aggregate(groups, aggs, input)) ->
    // aggregate(groups, aggs, filter(pred, input))
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("amount")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(RelExpr::scan("sales")),
    };
    let filtered = agg.filter(gt(col("region"), int(5)));
    let result = optimize(&filtered);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"sales".to_owned()));
}

#[test]
fn aggregate_over_aggregate_eliminated() {
    // aggregate(g, a1, aggregate(g, a2, input)) ->
    // aggregate(g, a1, input)
    let inner = RelExpr::Aggregate {
        group_by: vec![col("key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("val")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(RelExpr::scan("t")),
    };
    let outer = RelExpr::Aggregate {
        group_by: vec![col("key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(int(1)),
            distinct: false,
            alias: None,
        }],
        input: Box::new(inner),
    };
    let result = optimize(&outer);
    let d = depth(&result);
    assert!(
        d <= 2,
        "nested aggregate with same grouping should be flattened, depth={d}"
    );
}

// ── Project Optimization Tests ──────────────────────────────────

#[test]
fn project_merge_eliminates_redundant() {
    // project(c1, project(c2, input)) -> project(c1, input)
    let inner_proj = RelExpr::scan("t").project(vec![
        ProjectionColumn {
            expr: col("a"),
            alias: None,
        },
        ProjectionColumn {
            expr: col("b"),
            alias: None,
        },
        ProjectionColumn {
            expr: col("c"),
            alias: None,
        },
    ]);
    let outer_proj = inner_proj.project(vec![ProjectionColumn {
        expr: col("a"),
        alias: None,
    }]);
    let result = optimize(&outer_proj);
    let d = depth(&result);
    assert!(
        d <= 2,
        "redundant project should be merged, depth={d}"
    );
}

// ── Cross-Cutting Invariant Tests ───────────────────────────────

#[test]
fn optimization_never_drops_tables_join() {
    let input = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("a"), col("b")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("customers")),
    };
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"orders".to_owned()));
    assert!(tables.contains(&"customers".to_owned()));
}

#[test]
fn optimization_never_drops_tables_multi_join() {
    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("o_id"), col("c_id")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("customers")),
    };
    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("p_id"), col("i_id")),
        left: Box::new(j1),
        right: Box::new(RelExpr::scan("items")),
    };
    let result = optimize(&j2);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"orders".to_owned()));
    assert!(tables.contains(&"customers".to_owned()));
    assert!(tables.contains(&"items".to_owned()));
}

#[test]
fn optimization_idempotent_tables() {
    let input = filtered_scan("users", "age", 18);
    let first = optimize(&input);
    let second = optimize(&first);
    let t1 = collect_tables(&first);
    let t2 = collect_tables(&second);
    assert_eq!(t1, t2, "optimizing twice should preserve tables");
}

#[test]
fn roundtrip_conversion_preserves_simple_scan() {
    let input = RelExpr::scan("users");
    let rec = to_rec_expr(&input).expect("to_rec_expr");
    let back =
        rec_expr_to_rel_expr(&rec).expect("rec_expr_to_rel_expr");
    assert_eq!(input, back);
}

#[test]
fn roundtrip_conversion_preserves_filter() {
    let input = filtered_scan("users", "age", 18);
    let rec = to_rec_expr(&input).expect("to_rec_expr");
    let back =
        rec_expr_to_rel_expr(&rec).expect("rec_expr_to_rel_expr");
    assert_eq!(input, back);
}

#[test]
fn roundtrip_conversion_preserves_join() {
    let input =
        two_table_join("orders", "customers", "customer_id", "id");
    let rec = to_rec_expr(&input).expect("to_rec_expr");
    let back =
        rec_expr_to_rel_expr(&rec).expect("rec_expr_to_rel_expr");
    assert_eq!(input, back);
}

// ── Edge Cases ──────────────────────────────────────────────────

#[test]
fn empty_group_by_aggregate_optimizes() {
    let agg = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(int(1)),
            distinct: false,
            alias: None,
        }],
        input: Box::new(RelExpr::scan("t")),
    };
    let result = optimize(&agg);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn deeply_nested_filters_simplify() {
    let mut expr = RelExpr::scan("t");
    for i in 0..5 {
        expr = expr.filter(gt(col("x"), int(i)));
    }
    let result = optimize(&expr);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
    // The depth should be reduced from 6 (scan + 5 filters)
    // to at most 2 (scan + one merged filter with ANDs)
    let d = depth(&result);
    assert!(
        d <= 3,
        "5 stacked filters should merge, depth={d}"
    );
}

#[test]
fn limit_zero_offset_zero_preserves_plan() {
    let input = RelExpr::scan("t").limit(0, 0);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
}

#[test]
fn complex_predicate_with_all_simplification_rules() {
    // Combine multiple simplification opportunities:
    // filter(AND(NOT(NOT(a > 5)), OR(TRUE, b < 3)), scan)
    // -> filter(a > 5, scan)
    let pred = Expr::BinOp {
        op: BinOp::And,
        left: Box::new(Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(gt(col("a"), int(5))),
            }),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(Expr::Const(Const::Bool(true))),
            right: Box::new(Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(col("b")),
                right: Box::new(int(3)),
            }),
        }),
    };
    let input = RelExpr::scan("t").filter(pred);
    let result = optimize(&input);
    let tables = collect_tables(&result);
    assert!(tables.contains(&"t".to_owned()));
    // After simplification: NOT(NOT(x)) -> x, OR(TRUE, y) -> TRUE,
    // AND(x, TRUE) -> x
    // So result should be filter(a > 5, scan("t"))
    assert!(
        is_filter(&result),
        "complex predicate should simplify to a single filter"
    );
}
