use super::*;

use ra_core::algebra::{JoinType, NullOrdering, RelExpr, SortDirection};
use ra_core::expr::{BinOp, Expr};

/// Recursively search for a node matching the predicate.
fn find_node(r: &RelExpr, pred: fn(&RelExpr) -> bool) -> Option<&RelExpr> {
    if pred(r) {
        return Some(r);
    }
    r.children().into_iter().find_map(|c| find_node(c, pred))
}

/// Check that a node matching the predicate exists anywhere in the tree.
fn has_node(r: &RelExpr, pred: fn(&RelExpr) -> bool) -> bool {
    find_node(r, pred).is_some()
}

// ---- Existing tests (preserved) ----

// ---- RA-STEERING #21: quantified-comparison operator threading ----

/// Find the first `Expr::SubQuery` anywhere in a Filter predicate of the tree.
fn find_subquery(r: &RelExpr) -> Option<&ra_core::expr::Expr> {
    fn in_expr(e: &ra_core::expr::Expr) -> Option<&ra_core::expr::Expr> {
        use ra_core::expr::Expr;
        match e {
            Expr::SubQuery { .. } => Some(e),
            Expr::BinOp { left, right, .. } => in_expr(left).or_else(|| in_expr(right)),
            Expr::UnaryOp { operand, .. } => in_expr(operand),
            _ => None,
        }
    }
    if let RelExpr::Filter { predicate, .. } = r {
        if let Some(sq) = in_expr(predicate) {
            return Some(sq);
        }
    }
    r.children().into_iter().find_map(find_subquery)
}

/// `x > ANY (SELECT ...)` must carry the comparison operator into the
/// `SubQuery`'s `test_expr` as a template
/// `BinOp { op: Gt, left: x, right: Column("__subquery_operand") }`.
/// Before the fix the operator was dropped and `test_expr` was just `x`,
/// so all six ordered ops decorrelated as `=` (a wrong-answer defect).
#[test]
fn gt_any_builds_subquery_operand_template() {
    use ra_core::expr::{Expr, SubQueryType};
    let sql = "SELECT s_suppkey FROM supplier s                WHERE s.s_acctbal > ANY(SELECT c.c_acctbal FROM customer c)";
    let plan = sql_to_relexpr(sql).expect("should parse");
    let sq = find_subquery(&plan).expect("expected a SubQuery in the filter");
    let Expr::SubQuery {
        subquery_type,
        test_expr,
        ..
    } = sq
    else {
        panic!("not a SubQuery: {sq:?}");
    };
    assert_eq!(
        *subquery_type,
        SubQueryType::Any,
        "ANY -> SubQueryType::Any"
    );
    let te = test_expr.as_ref().expect("test_expr must be present");
    let Expr::BinOp { op, right, .. } = te.as_ref() else {
        panic!("test_expr must be a comparison template, got {te:?}");
    };
    assert_eq!(
        *op,
        BinOp::Gt,
        "the `>` operator must be carried in the template"
    );
    match right.as_ref() {
        Expr::Column(c) => assert_eq!(
            c.column, "__subquery_operand",
            "template RHS must be the __subquery_operand sentinel"
        ),
        other => panic!("template RHS must be the sentinel column, got {other:?}"),
    }
}

#[test]
fn test_simple_select() {
    let sql = "SELECT * FROM users";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

#[test]
fn test_select_with_where() {
    let sql = "SELECT * FROM users WHERE age > 18";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

#[test]
fn test_select_with_join() {
    let sql = "SELECT * FROM orders o \
               JOIN customers c ON o.customer_id = c.id";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

#[test]
fn test_select_with_aggregate() {
    let sql = "SELECT region, COUNT(*), SUM(amount) \
               FROM orders GROUP BY region";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok());
}

// ---- DISTINCT tests ----

#[test]
fn test_select_distinct() {
    let sql = "SELECT DISTINCT name FROM users";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Distinct { .. })),
        "expected Distinct node"
    );
}

#[test]
fn test_select_distinct_multiple_cols() {
    let sql = "SELECT DISTINCT dept_id, job_title FROM employees";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(has_node(&result, |r| matches!(r, RelExpr::Distinct { .. })));
}

// ---- ORDER BY tests ----

#[test]
fn test_order_by_asc() {
    let sql = "SELECT * FROM users ORDER BY name ASC";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].direction, SortDirection::Asc);
    }
}

#[test]
fn test_order_by_desc() {
    let sql = "SELECT * FROM users ORDER BY age DESC";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys[0].direction, SortDirection::Desc);
    }
}

#[test]
fn test_order_by_multiple() {
    let sql = "SELECT * FROM users ORDER BY dept ASC, name DESC";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].direction, SortDirection::Asc);
        assert_eq!(keys[1].direction, SortDirection::Desc);
    }
}

#[test]
fn test_order_by_nulls() {
    let sql = "SELECT * FROM users ORDER BY name ASC NULLS FIRST";
    let result = sql_to_relexpr(sql).expect("should parse");
    let sort =
        find_node(&result, |r| matches!(r, RelExpr::Sort { .. })).expect("expected Sort node");
    if let RelExpr::Sort { keys, .. } = sort {
        assert_eq!(keys[0].nulls, NullOrdering::First);
    }
}

// ---- LIMIT/OFFSET tests ----
// Lime grammar does not yet produce Limit nodes (placeholder only).

#[test]
fn test_limit() {
    let sql = "SELECT * FROM users LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 10);
        assert_eq!(*offset, 0);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_limit_offset() {
    let sql = "SELECT * FROM users LIMIT 10 OFFSET 20";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 10);
        assert_eq!(*offset, 20);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_limit_all_no_row_cap() {
    // LIMIT ALL means "no row limit" -> count is the u64::MAX sentinel.
    let sql = "SELECT * FROM users LIMIT ALL";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, u64::MAX);
        assert_eq!(*offset, 0);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_offset_before_limit() {
    // PG accepts OFFSET before LIMIT; the reversed order yields the same plan.
    let sql = "SELECT * FROM users OFFSET 5 LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 10);
        assert_eq!(*offset, 5);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_fetch_first_n_rows_only() {
    // SQL-standard FETCH FIRST n ROWS ONLY == LIMIT n.
    let sql = "SELECT a FROM t FETCH FIRST 3 ROWS ONLY";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 3);
        assert_eq!(*offset, 0);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_offset_rows_fetch_next_rows_only() {
    // OFFSET n ROWS FETCH NEXT m ROWS ONLY == LIMIT m OFFSET n.
    let sql = "SELECT a FROM t OFFSET 5 ROWS FETCH NEXT 3 ROWS ONLY";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 3);
        assert_eq!(*offset, 5);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_fetch_first_row_only_no_count() {
    // FETCH FIRST ROW ONLY (no count) == LIMIT 1.
    let sql = "SELECT a FROM t FETCH FIRST ROW ONLY";
    let result = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::Limit { count, offset, .. } = &result {
        assert_eq!(*count, 1);
        assert_eq!(*offset, 0);
    } else {
        panic!("expected Limit at top level");
    }
}

#[test]
fn test_order_by_with_limit() {
    let sql = "SELECT * FROM users ORDER BY name LIMIT 5";
    let result = sql_to_relexpr(sql).expect("should parse");
    // Should be Limit(Sort(...))
    if let RelExpr::Limit { input, count, .. } = &result {
        assert_eq!(*count, 5);
        assert!(matches!(input.as_ref(), RelExpr::Sort { .. }));
    } else {
        panic!("expected Limit(Sort(...))");
    }
}

// ---- HAVING tests ----

#[test]
fn test_having() {
    let sql = "SELECT dept_id, COUNT(*) \
               FROM employees \
               GROUP BY dept_id \
               HAVING COUNT(*) > 5";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Filter { .. })),
        "expected Filter for HAVING"
    );
}

#[test]
fn test_having_with_group_by() {
    let sql = "SELECT region, SUM(amount) as total \
               FROM orders \
               GROUP BY region \
               HAVING SUM(amount) > 1000";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Aggregate { .. })),
        "expected Aggregate node"
    );
}

// ---- CTE tests ----

#[test]
fn test_simple_cte() {
    let sql = "WITH active AS (SELECT * FROM users WHERE active = true) \
               SELECT * FROM active";
    let result = sql_to_relexpr(sql).expect("should parse");
    let cte = find_node(&result, |r| matches!(r, RelExpr::CTE { .. })).expect("expected CTE node");
    if let RelExpr::CTE { name, .. } = cte {
        assert_eq!(name, "active");
    }
}

#[test]
fn test_multiple_ctes() {
    let sql = "WITH \
                 a AS (SELECT * FROM t1), \
                 b AS (SELECT * FROM t2) \
               SELECT * FROM a";
    let result = sql_to_relexpr(sql).expect("should parse");
    // Outermost should be CTE 'a' wrapping CTE 'b'
    if let RelExpr::CTE { name, body, .. } = &result {
        assert_eq!(name, "a");
        assert!(matches!(body.as_ref(), RelExpr::CTE { .. }));
    } else {
        panic!("expected nested CTEs");
    }
}

// ---- Subquery tests ----

#[test]
fn test_subquery_in_from() {
    let sql = "SELECT * FROM (SELECT id, name FROM users) t";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "subquery in FROM should parse");
}

#[test]
fn test_derived_table_alias_preserved() {
    // FROM (subquery) alias must produce a SubqueryAlias carrying the alias,
    // so outer `alias.col` references resolve (Codeberg #23).
    let sql = "SELECT t.id FROM (SELECT id, name FROM users) t";
    let plan = sql_to_relexpr(sql).expect("derived table should parse");
    let found = find_node(
        &plan,
        |r| matches!(r, RelExpr::SubqueryAlias { alias, .. } if alias == "t"),
    );
    assert!(
        found.is_some(),
        "expected a SubqueryAlias{{alias:\"t\"}} node, got: {plan:?}"
    );
}

#[test]
fn test_subquery_in_where() {
    let sql = "SELECT * FROM orders \
               WHERE customer_id IN \
               (SELECT id FROM customers WHERE active = true)";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "subquery in WHERE should parse");
}

#[test]
fn test_exists_subquery() {
    let sql = "SELECT * FROM customers c \
               WHERE EXISTS \
               (SELECT 1 FROM orders o WHERE o.cust_id = c.id)";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "EXISTS subquery should parse");
}

// ---- Quantified-comparison subquery tests (Codeberg #20) ----
// `x <op> ANY/ALL/SOME (subquery)` must build an Expr::SubQuery that PRESERVES
// the subquery relation, not an inert __<op>_any/__<op>_all Func that drops it.

use ra_core::expr::SubQueryType;

/// Find the `SubQuery` expr inside a `Filter` predicate, if any.
fn filter_subquery(r: &RelExpr) -> Option<(&SubQueryType, &RelExpr)> {
    let filter = find_node(r, |n| matches!(n, RelExpr::Filter { .. }))?;
    let RelExpr::Filter { predicate, .. } = filter else {
        return None;
    };
    if let Expr::SubQuery {
        subquery_type,
        query,
        ..
    } = predicate
    {
        Some((subquery_type, query.as_ref()))
    } else {
        None
    }
}

/// A predicate that is (or contains at the top level) an `__eq_any`-style `Func`.
fn is_quantified_func(e: &Expr) -> bool {
    matches!(e, Expr::Function { name, .. }
        if name.starts_with("__") && (name.ends_with("_any") || name.ends_with("_all"))
            && !name.starts_with("__saoarr_"))
}

#[test]
fn test_eq_any_subquery_preserves_relation() {
    // The exact Codeberg #20 repro: id = ANY(SELECT id FROM u) dropped `u`.
    let sql = "SELECT * FROM t WHERE id = ANY(SELECT id FROM u)";
    let result = sql_to_relexpr(sql).expect("= ANY subquery should parse");

    let (sq_type, query) = filter_subquery(&result)
        .expect("expected a SubQuery in the Filter predicate, not a __eq_any Func");
    assert_eq!(*sq_type, SubQueryType::Any, "= ANY -> SubQueryType::Any");

    // The relation `u` must still be referenced inside the subquery.
    assert!(
        find_node(
            query,
            |n| matches!(n, RelExpr::Scan { table, .. } if table == "u")
        )
        .is_some(),
        "subquery relation `u` must be preserved, got: {query:?}"
    );

    // And it must NOT be the old inert __eq_any Func.
    if let RelExpr::Filter { predicate, .. } =
        find_node(&result, |n| matches!(n, RelExpr::Filter { .. })).expect("Filter")
    {
        assert!(
            !is_quantified_func(predicate),
            "predicate must not be a __<op>_any/_all Func: {predicate:?}"
        );
    }
}

#[test]
fn test_ne_all_subquery_preserves_relation() {
    let sql = "SELECT * FROM t WHERE id <> ALL(SELECT id FROM u)";
    let result = sql_to_relexpr(sql).expect("<> ALL subquery should parse");
    let (sq_type, query) = filter_subquery(&result).expect("expected a SubQuery");
    assert_eq!(*sq_type, SubQueryType::All, "<> ALL -> SubQueryType::All");
    assert!(
        find_node(
            query,
            |n| matches!(n, RelExpr::Scan { table, .. } if table == "u")
        )
        .is_some(),
        "subquery relation `u` must be preserved"
    );
}

#[test]
fn test_gt_any_subquery_preserves_relation() {
    let sql = "SELECT * FROM t WHERE x > ANY(SELECT y FROM u)";
    let result = sql_to_relexpr(sql).expect("> ANY subquery should parse");
    let (sq_type, query) = filter_subquery(&result).expect("expected a SubQuery");
    assert_eq!(*sq_type, SubQueryType::Any);
    assert!(
        find_node(
            query,
            |n| matches!(n, RelExpr::Scan { table, .. } if table == "u")
        )
        .is_some(),
        "subquery relation `u` must be preserved"
    );
}

#[test]
fn test_some_subquery_preserves_relation() {
    // SOME is a synonym for ANY.
    let sql = "SELECT * FROM t WHERE x = SOME(SELECT y FROM u)";
    let result = sql_to_relexpr(sql).expect("= SOME subquery should parse");
    let (sq_type, query) = filter_subquery(&result).expect("expected a SubQuery");
    assert_eq!(*sq_type, SubQueryType::Any, "SOME is a synonym for ANY");
    assert!(
        find_node(
            query,
            |n| matches!(n, RelExpr::Scan { table, .. } if table == "u")
        )
        .is_some(),
        "subquery relation `u` must be preserved"
    );
}

#[test]
fn test_any_array_form_still_scalar_array_op() {
    // The non-subquery array form must NOT become a SubQuery.
    let sql = "SELECT * FROM t WHERE x = ANY(ARRAY[1,2,3])";
    let result = sql_to_relexpr(sql).expect("= ANY(ARRAY[...]) should parse");
    assert!(
        filter_subquery(&result).is_none(),
        "array-expr ANY must not be lowered to a SubQuery"
    );
}

// ---- JOIN type tests ----

#[test]
fn test_left_join() {
    let sql = "SELECT * FROM orders o \
               LEFT JOIN customers c ON o.cust_id = c.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::LeftOuter,
                ..
            }
        )
    })
    .expect("expected LeftOuter Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::LeftOuter);
    }
}

#[test]
fn test_right_join() {
    let sql = "SELECT * FROM orders o \
               RIGHT JOIN customers c ON o.cust_id = c.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::RightOuter,
                ..
            }
        )
    })
    .expect("expected RightOuter Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::RightOuter);
    }
}

#[test]
fn test_full_outer_join() {
    let sql = "SELECT * FROM a \
               FULL OUTER JOIN b ON a.id = b.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::FullOuter,
                ..
            }
        )
    })
    .expect("expected FullOuter Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::FullOuter);
    }
}

#[test]
fn test_cross_join() {
    let sql = "SELECT * FROM a CROSS JOIN b";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join = find_node(&result, |r| {
        matches!(
            r,
            RelExpr::Join {
                join_type: JoinType::Cross,
                ..
            }
        )
    })
    .expect("expected Cross Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::Cross);
    }
}

// ---- Typed JOIN ... USING tests (bug A) ----

#[test]
fn test_left_join_using_preserves_join_type() {
    // LEFT JOIN ... USING(cols) must yield a LeftOuter join, not Inner.
    let sql = "SELECT a FROM t LEFT JOIN u USING (id)";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join =
        find_node(&result, |r| matches!(r, RelExpr::Join { .. })).expect("expected a Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::LeftOuter);
    }
}

#[test]
fn test_inner_join_using_parses() {
    let sql = "SELECT a FROM t INNER JOIN u USING (id)";
    let result = sql_to_relexpr(sql).expect("should parse");
    let join =
        find_node(&result, |r| matches!(r, RelExpr::Join { .. })).expect("expected a Join node");
    if let RelExpr::Join { join_type, .. } = join {
        assert_eq!(*join_type, JoinType::Inner);
    }
}

// ---- Schema-qualified table tests (bug B) ----

#[test]
fn test_schema_qualified_table() {
    // `public.t` scans the qualified relation name `public.t`.
    let sql = "SELECT x FROM public.t";
    let result = sql_to_relexpr(sql).expect("should parse");
    let scan =
        find_node(&result, |r| matches!(r, RelExpr::Scan { .. })).expect("expected a Scan node");
    if let RelExpr::Scan { table, .. } = scan {
        assert_eq!(table, "public.t");
    }
}

#[test]
fn test_three_part_qualified_column() {
    // schema.table.column in the target list parses; the schema qualifier is
    // dropped from the column ref (FROM resolves the relation), leaving the
    // column qualified by the table part — same as the two-part form.
    let sql = "SELECT public.t.x FROM public.t";
    let result = sql_to_relexpr(sql).expect("three-part qualified column should parse");
    // The projected column is `t.x` (table `t`, column `x`).
    let has_col = find_node(&result, |r| matches!(r, RelExpr::Project { .. })).is_some();
    assert!(has_col, "expected a Project over the schema-qualified scan");
}

#[test]
fn test_schema_qualified_table_with_alias() {
    let sql = "SELECT x FROM public.t AS pt";
    let result = sql_to_relexpr(sql).expect("should parse");
    let scan =
        find_node(&result, |r| matches!(r, RelExpr::Scan { .. })).expect("expected a Scan node");
    if let RelExpr::Scan { table, alias, .. } = scan {
        assert_eq!(table, "public.t");
        assert_eq!(alias.as_deref(), Some("pt"));
    }
}

// ---- Window function tests ----
// Lime grammar encodes window functions as regular function calls,
// not as Window RelExpr nodes.

#[test]
fn test_row_number_window() {
    let sql = "SELECT id, ROW_NUMBER() OVER (ORDER BY id) as rn \
               FROM users";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Window { .. })),
        "expected Window node"
    );
}

#[test]
fn test_rank_window_with_partition() {
    let sql = "SELECT dept, salary, \
               RANK() OVER (PARTITION BY dept ORDER BY salary DESC) as rnk \
               FROM employees";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Window { .. })),
        "expected Window node"
    );
}

#[test]
fn test_window_sum() {
    let sql = "SELECT id, \
               SUM(amount) OVER (ORDER BY id) as running_total \
               FROM orders";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Window { .. })),
        "expected Window node"
    );
}

// RA-STEERING #22: window frame bounds must survive the parse so the emitter
// can reproduce `ROWS/RANGE BETWEEN ...`. We assert the WindowExpr carries a
// parsed WindowFrame (mode + start/end bounds), not just a presence flag.
#[test]
fn test_window_frame_rows_between_preserved() {
    use ra_core::algebra::{WindowFrameBound, WindowFrameMode};
    let sql = "SELECT SUM(amount) OVER (                PARTITION BY custkey ORDER BY id                ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)                FROM orders";
    let result = sql_to_relexpr(sql).expect("should parse");
    let win =
        find_node(&result, |r| matches!(r, RelExpr::Window { .. })).expect("expected Window node");
    let RelExpr::Window { functions, .. } = win else {
        unreachable!("matched Window above")
    };
    let frame = functions[0]
        .frame
        .as_ref()
        .expect("frame bounds should be preserved");
    assert_eq!(frame.mode, WindowFrameMode::Rows);
    assert_eq!(frame.start, WindowFrameBound::UnboundedPreceding);
    assert_eq!(frame.end, WindowFrameBound::CurrentRow);
}

#[test]
fn test_window_frame_n_preceding_following_preserved() {
    use ra_core::algebra::{WindowFrameBound, WindowFrameMode};
    let sql = "SELECT SUM(amount) OVER (                PARTITION BY custkey ORDER BY id                ROWS BETWEEN 3 PRECEDING AND 1 FOLLOWING)                FROM orders";
    let result = sql_to_relexpr(sql).expect("should parse");
    let win =
        find_node(&result, |r| matches!(r, RelExpr::Window { .. })).expect("expected Window node");
    let RelExpr::Window { functions, .. } = win else {
        unreachable!("matched Window above")
    };
    let frame = functions[0]
        .frame
        .as_ref()
        .expect("frame bounds should be preserved");
    assert_eq!(frame.mode, WindowFrameMode::Rows);
    assert_eq!(frame.start, WindowFrameBound::Preceding(3));
    assert_eq!(frame.end, WindowFrameBound::Following(1));
}

// A window without a frame clause must not fabricate one.
#[test]
fn test_window_without_frame_has_none() {
    let sql = "SELECT ROW_NUMBER() OVER (PARTITION BY dept ORDER BY id) FROM t";
    let result = sql_to_relexpr(sql).expect("should parse");
    let win =
        find_node(&result, |r| matches!(r, RelExpr::Window { .. })).expect("expected Window node");
    let RelExpr::Window { functions, .. } = win else {
        unreachable!("matched Window above")
    };
    assert!(
        functions[0].frame.is_none(),
        "frameless window should have no frame"
    );
}

// ---- Set operation tests ----

#[test]
fn test_union() {
    let sql = "SELECT id FROM a UNION SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Union { all: false, .. }));
}

#[test]
fn test_union_all() {
    let sql = "SELECT id FROM a UNION ALL SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Union { all: true, .. }));
}

#[test]
fn test_intersect() {
    let sql = "SELECT id FROM a INTERSECT SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Intersect { all: false, .. }));
}

#[test]
fn test_except() {
    let sql = "SELECT id FROM a EXCEPT SELECT id FROM b";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(result, RelExpr::Except { all: false, .. }));
}

// ---- Extended aggregate tests ----
// Lime grammar treats STDDEV/VARIANCE as function calls, not
// Aggregate nodes. Only GROUP BY triggers Aggregate creation.

#[test]
fn test_stddev_aggregate() {
    let sql = "SELECT STDDEV(salary) FROM employees";
    let result = sql_to_relexpr(sql).expect("should parse");
    let agg =
        find_node(&result, |r| matches!(r, RelExpr::Aggregate { .. })).expect("expected Aggregate");
    if let RelExpr::Aggregate { aggregates, .. } = agg {
        assert_eq!(
            aggregates[0].function,
            ra_core::algebra::AggregateFunction::StdDev
        );
    }
}

#[test]
fn test_variance_aggregate() {
    let sql = "SELECT VARIANCE(score) FROM tests";
    let result = sql_to_relexpr(sql).expect("should parse");
    let agg =
        find_node(&result, |r| matches!(r, RelExpr::Aggregate { .. })).expect("expected Aggregate");
    if let RelExpr::Aggregate { aggregates, .. } = agg {
        assert_eq!(
            aggregates[0].function,
            ra_core::algebra::AggregateFunction::Variance
        );
    }
}

// ---- BETWEEN test ----

#[test]
fn test_between() {
    let sql = "SELECT * FROM orders WHERE amount BETWEEN 10 AND 100";
    let result = sql_to_relexpr(sql).expect("should parse");
    let filter =
        find_node(&result, |r| matches!(r, RelExpr::Filter { .. })).expect("expected Filter node");
    if let RelExpr::Filter { predicate, .. } = filter {
        assert!(
            matches!(predicate, Expr::BinOp { op: BinOp::And, .. }),
            "BETWEEN should expand to AND"
        );
    }
}

// ---- CAST test ----

#[test]
fn test_cast() {
    let sql = "SELECT CAST(price AS INTEGER) FROM products";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "CAST should parse");
}

// ---- CASE expression test ----

#[test]
fn test_case_expression() {
    let sql = "SELECT CASE WHEN age > 18 THEN 'adult' \
               ELSE 'minor' END FROM users";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "CASE should parse");
}

// ---- Combination tests ----

#[test]
fn test_cte_with_window() {
    let sql = "WITH ranked AS (\
                 SELECT id, \
                   ROW_NUMBER() OVER (ORDER BY id) as rn \
                 FROM users\
               ) \
               SELECT * FROM ranked WHERE rn <= 10";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "CTE + window should parse");
}

#[test]
fn test_distinct_with_order_by() {
    let sql = "SELECT DISTINCT name FROM users ORDER BY name";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Sort { .. })),
        "expected Sort node"
    );
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Distinct { .. })),
        "expected Distinct node"
    );
}

#[test]
fn test_having_with_limit() {
    let sql = "SELECT dept_id, COUNT(*) as cnt \
               FROM employees \
               GROUP BY dept_id \
               HAVING COUNT(*) > 5 \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        matches!(result, RelExpr::Limit { .. }),
        "expected Limit at top"
    );
}

#[test]
fn test_complex_query() {
    let sql = "WITH dept_stats AS (\
                 SELECT dept_id, AVG(salary) as avg_sal \
                 FROM employees \
                 GROUP BY dept_id \
                 HAVING AVG(salary) > 50000\
               ) \
               SELECT DISTINCT d.dept_id \
               FROM dept_stats d \
               ORDER BY d.dept_id \
               LIMIT 20 OFFSET 5";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_ok(),
        "complex query should parse: {:?}",
        result.err()
    );
}

#[test]
fn test_multiple_from_items() {
    let sql = "SELECT * FROM a, b WHERE a.id = b.id";
    let result = sql_to_relexpr(sql).expect("should parse");
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Join { .. })),
        "expected implicit cross join"
    );
}

#[test]
fn test_join_using() {
    let sql = "SELECT * FROM orders JOIN customers USING (customer_id)";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JOIN USING should parse");
}

// ---- Recursive CTE tests ----
// Lime grammar does not distinguish WITH RECURSIVE from WITH.
// It produces CTE nodes instead of RecursiveCTE nodes.

#[test]
fn test_simple_recursive_cte() {
    let sql = "\
        WITH RECURSIVE counter AS (\
            SELECT n FROM seed_table WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM counter WHERE n < 10\
        ) SELECT * FROM counter";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "simple recursive CTE: {result:?}");
    let plan = result.expect("already checked");
    assert!(
        matches!(&plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE node, got: {plan:?}"
    );
}

#[test]
fn test_recursive_cte_name() {
    let sql = "\
        WITH RECURSIVE nums AS (\
            SELECT val FROM seed WHERE val = 1 \
            UNION ALL \
            SELECT val + 1 FROM nums WHERE val < 5\
        ) SELECT * FROM nums";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE { name, .. } = &plan {
        assert_eq!(name, "nums");
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_base_is_non_recursive() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT id FROM nodes WHERE root = true \
            UNION ALL \
            SELECT e.dst FROM edges e JOIN r ON e.src = r.id\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE {
        base_case, name, ..
    } = &plan
    {
        assert!(
            !base_case.references_cte(name),
            "base case should not reference CTE"
        );
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_recursive_references_cte() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT id FROM nodes WHERE root = true \
            UNION ALL \
            SELECT e.dst FROM edges e JOIN r ON e.src = r.id\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE {
        recursive_case,
        name,
        ..
    } = &plan
    {
        assert!(
            recursive_case.references_cte(name),
            "recursive case should reference CTE"
        );
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_has_cycle_detection() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 10\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    if let RelExpr::RecursiveCTE {
        cycle_detection, ..
    } = &plan
    {
        assert!(
            cycle_detection.is_some(),
            "should have default cycle detection"
        );
        let cd = cycle_detection.as_ref().expect("checked");
        assert_eq!(cd.max_depth, Some(1000));
    } else {
        panic!("expected RecursiveCTE");
    }
}

#[test]
fn test_recursive_cte_with_order_by() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 10\
        ) SELECT * FROM r ORDER BY n";
    let plan = sql_to_relexpr(sql).expect("should parse");
    // RecursiveCTE is the outermost node; the body contains the Sort.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "RecursiveCTE is outermost, got: {plan:?}"
    );
    if let RelExpr::RecursiveCTE { body, .. } = &plan {
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Sort { .. })),
            "Sort should appear in CTE body"
        );
    }
}

#[test]
fn test_recursive_cte_with_limit() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 100\
        ) SELECT * FROM r LIMIT 10";
    let plan = sql_to_relexpr(sql).expect("should parse");
    // RecursiveCTE is outermost; body contains the Limit.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "RecursiveCTE is outermost, got: {plan:?}"
    );
    if let RelExpr::RecursiveCTE { body, .. } = &plan {
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Limit { .. })),
            "Limit should appear in CTE body"
        );
    }
}

#[test]
fn test_non_recursive_with_recursive_keyword() {
    // WITH RECURSIVE keyword but body is not UNION ALL — treated as regular CTE
    let sql = "\
        WITH RECURSIVE t AS (\
            SELECT id FROM users\
        ) SELECT * FROM t";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert!(
        matches!(plan, RelExpr::CTE { .. }),
        "WITH RECURSIVE without UNION ALL body produces CTE, got: {plan:?}"
    );
}

#[test]
fn test_running_totals_query() {
    let sql = "\
        WITH RECURSIVE DatewiseTotal AS (\
            SELECT id, date, department, amount \
            FROM financial_data \
            WHERE department = 'HR' \
                AND date = (SELECT MIN(date) \
                    FROM financial_data \
                    WHERE department = 'HR')\
            UNION ALL \
            SELECT fd.id, fd.date, fd.department, \
                   fd.amount + dt.amount \
            FROM financial_data fd \
            JOIN DatewiseTotal dt \
                ON fd.date = (SELECT MIN(date) \
                    FROM financial_data \
                    WHERE date > dt.date \
                        AND department = 'HR') \
            WHERE fd.department = 'HR'\
        ) \
        SELECT * FROM DatewiseTotal ORDER BY date";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_ok(),
        "running totals query should parse: {result:?}"
    );
    let plan = result.expect("already checked");

    // RecursiveCTE is the outermost node; body contains the Sort.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE at top, got {plan:?}"
    );

    if let RelExpr::RecursiveCTE { name, body, .. } = &plan {
        assert_eq!(name.to_lowercase(), "datewisetotal");
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Sort { .. })),
            "Sort should appear in CTE body"
        );
    }
}

#[test]
fn test_graph_reachability_recursive_cte() {
    let sql = "\
        WITH RECURSIVE reachable AS (\
            SELECT dst FROM edges WHERE src = 1 \
            UNION ALL \
            SELECT e.dst FROM edges e \
            JOIN reachable r ON e.src = r.dst\
        ) SELECT * FROM reachable";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE"
    );
}

#[test]
fn test_fibonacci_recursive_cte() {
    let sql = "\
        WITH RECURSIVE fib AS (\
            SELECT n, a, b FROM seed \
            WHERE n = 1 AND a = 0 AND b = 1 \
            UNION ALL \
            SELECT n + 1, b, a + b FROM fib WHERE n < 20\
        ) SELECT n, a FROM fib";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert!(matches!(plan, RelExpr::RecursiveCTE { .. }));
}

#[test]
fn test_tree_hierarchy_recursive_cte() {
    let sql = "\
        WITH RECURSIVE hierarchy AS (\
            SELECT id, name, parent_id, 0 AS depth \
            FROM employees WHERE parent_id IS NULL \
            UNION ALL \
            SELECT e.id, e.name, e.parent_id, h.depth + 1 \
            FROM employees e \
            JOIN hierarchy h ON e.parent_id = h.id\
        ) SELECT * FROM hierarchy ORDER BY depth, name";
    let plan = sql_to_relexpr(sql).expect("should parse");
    // RecursiveCTE is outermost; Sort is in the body.
    assert!(
        matches!(plan, RelExpr::RecursiveCTE { .. }),
        "expected RecursiveCTE at top, got: {plan:?}"
    );
    if let RelExpr::RecursiveCTE { body, .. } = &plan {
        assert!(
            has_node(body, |r| matches!(r, RelExpr::Sort { .. })),
            "Sort should appear in CTE body (ORDER BY depth, name)"
        );
    }
}

#[test]
fn test_recursive_cte_children_count() {
    let sql = "\
        WITH RECURSIVE r AS (\
            SELECT n FROM seed WHERE n = 1 \
            UNION ALL \
            SELECT n + 1 FROM r WHERE n < 5\
        ) SELECT * FROM r";
    let plan = sql_to_relexpr(sql).expect("should parse");
    assert_eq!(plan.children().len(), 3, "RecursiveCTE has 3 children");
}

// ---- Multi-statement and non-SELECT handling ----

#[test]
fn test_multi_statement_takes_first_select() {
    let sql = "CREATE TABLE t (id INT); \
               SELECT * FROM users WHERE age > 18";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "multi-statement with SELECT should work");
}

#[test]
fn test_select_without_from() {
    let sql = "SELECT 1 + 2";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "SELECT without FROM: {result:?}");
}

// ---- Qualified wildcard and mixed wildcard ----

#[test]
fn test_qualified_wildcard() {
    let sql = "SELECT o.*, u.name \
               FROM orders o JOIN users u ON o.uid = u.id";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "qualified wildcard o.*: {result:?}");
}

#[test]
fn test_wildcard_in_multi_column() {
    let sql = "SELECT *, name FROM users";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "wildcard in multi-column: {result:?}");
}

// ---- IN, LIKE, INTERVAL, DATE ----

#[test]
fn test_in_list() {
    let sql = "SELECT * FROM orders \
               WHERE status IN ('shipped', 'delivered')";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "IN list: {result:?}");
}

#[test]
fn test_like() {
    let sql = "SELECT * FROM users WHERE email LIKE 'a%'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "LIKE: {result:?}");
}

#[test]
fn test_interval() {
    let sql = "SELECT * FROM events \
               WHERE created_at > INTERVAL '1 hour'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "INTERVAL: {result:?}");
}

#[test]
fn test_date_literal() {
    let sql = "SELECT * FROM orders \
               WHERE order_date > DATE '2024-01-01'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "DATE literal: {result:?}");
}

#[test]
fn test_placeholder() {
    let sql = "SELECT * FROM users WHERE id = ?";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "placeholder: {result:?}");
}

#[test]
fn test_extract() {
    let sql = "SELECT EXTRACT(YEAR FROM order_date) \
               FROM orders";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "EXTRACT: {result:?}");
}

// ---- PostgreSQL-specific operators ----

#[test]
fn test_jsonb_contains() {
    let sql = "SELECT * FROM users \
               WHERE data @> '{\"age\": 25}'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB @> operator: {result:?}");
}

#[test]
fn test_jsonb_contained_by() {
    let sql = "SELECT * FROM users \
               WHERE '{\"age\": 25}' <@ data";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB <@ operator: {result:?}");
}

#[test]
fn test_jsonb_path_exists() {
    let sql = "SELECT * FROM users \
               WHERE data @? '$.age ? (@ > 25)'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB @? operator: {result:?}");
}

#[test]
fn test_jsonb_path_match() {
    let sql = "SELECT * FROM users \
               WHERE data @@ '$.status == \"active\"'";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "JSONB @@ operator: {result:?}");
}

#[test]
fn test_documentdb_query() {
    // DocumentDB query with standard PostgreSQL JSONB operators
    let sql = "SELECT document FROM documentdb_api.collection('mydb', 'users') \
               WHERE document @> '{\"age\": {\"$gt\": 25}}' \
               AND document @? '$.status ? (@ == \"active\")'";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_ok(),
        "DocumentDB query with JSONB operators: {result:?}"
    );
}

// ---- Vector Search tests ----
// The old sqlparser pipeline had special-case logic to produce TopK and
// VectorFilter nodes. The Lime grammar produces standard Sort/Filter nodes.

#[test]
fn test_sqlite_vec_topk_l2() {
    // sqlite-vec with vec_distance_l2 function
    let sql = "SELECT * FROM items \
               ORDER BY vec_distance_l2(embedding, vec_f32('[1,2,3]')) \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse sqlite-vec TopK");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 10);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
fn test_sqlite_vec_topk_cosine() {
    // sqlite-vec with cosine distance
    let sql = "SELECT id, vec_distance_cosine(embedding, query_vec) AS similarity \
               FROM items \
               ORDER BY vec_distance_cosine(embedding, query_vec) \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse sqlite-vec cosine");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 10);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::Cosine);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
fn test_sqlite_vec_filter() {
    // sqlite-vec with threshold filter
    let sql = "SELECT * FROM items \
               WHERE vec_distance_l2(embedding, vec_f32('[1,2,3]')) < 0.5";
    let result = sql_to_relexpr(sql).expect("should parse sqlite-vec filter");

    match result {
        RelExpr::VectorFilter {
            threshold, metric, ..
        } => {
            assert_eq!(threshold, 0.5);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected VectorFilter, got {result:?}"),
    }
}

#[test]
fn test_vector_hybrid_search() {
    // Simple vector filter works
    let sql = "SELECT * FROM products \
               WHERE l2_distance(embedding, query_vec) < 0.8";
    let result = sql_to_relexpr(sql).expect("should parse simple vector filter");

    // Should produce VectorFilter
    match result {
        RelExpr::VectorFilter { threshold, .. } => {
            assert_eq!(threshold, 0.8);
        }
        _ => panic!("expected VectorFilter for simple case, got {result:?}"),
    }
}

#[test]
fn test_pgvector_topk_l2_function() {
    // pgvector with l2_distance function
    let sql = "SELECT * FROM items \
               ORDER BY l2_distance(embedding, '[1,2,3]') \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse pgvector L2 TopK");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 10);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
fn test_pgvector_topk_cosine_function() {
    // pgvector with cosine_distance function
    let sql = "SELECT id, text FROM documents \
               ORDER BY cosine_distance(embedding, '[0.1, 0.2, 0.3]') \
               LIMIT 5";
    let result = sql_to_relexpr(sql).expect("should parse pgvector cosine TopK");

    match result {
        RelExpr::TopK { k, metric, .. } => {
            assert_eq!(k, 5);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::Cosine);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

#[test]
fn test_pgvector_filter_function() {
    // pgvector with distance threshold in WHERE using function
    let sql = "SELECT * FROM items WHERE l2_distance(embedding, query_vec) < 0.5";
    let result = sql_to_relexpr(sql).expect("should parse pgvector filter");

    match result {
        RelExpr::VectorFilter {
            threshold, metric, ..
        } => {
            assert_eq!(threshold, 0.5);
            assert_eq!(metric, ra_core::search_types::DistanceMetric::L2);
        }
        _ => panic!("expected VectorFilter, got {result:?}"),
    }
}

#[test]
fn test_vector_without_limit() {
    // Vector ORDER BY without LIMIT should produce regular Sort
    let sql = "SELECT * FROM items ORDER BY l2_distance(embedding, '[1,2,3]')";
    let result = sql_to_relexpr(sql).expect("should parse");

    // Without LIMIT, should be a regular Sort, not TopK
    assert!(
        has_node(&result, |r| matches!(r, RelExpr::Sort { .. })),
        "expected Sort without LIMIT, got {result:?}"
    );
}

#[test]
fn test_vector_multiple_order_by_columns() {
    // Multiple ORDER BY expressions should use regular Sort
    let sql = "SELECT * FROM items \
               ORDER BY l2_distance(embedding, '[1,2,3]'), created_at DESC \
               LIMIT 10";
    let result = sql_to_relexpr(sql).expect("should parse");

    // Multiple ORDER BY => regular Sort + Limit
    match result {
        RelExpr::Limit { input, .. } => {
            assert!(matches!(*input, RelExpr::Sort { .. }));
        }
        _ => panic!("expected Limit(Sort(...)), got {result:?}"),
    }
}

#[test]
fn test_vector_with_projection() {
    // Vector search with specific columns selected
    let sql = "SELECT id, title, cosine_distance(embedding, query) AS similarity \
               FROM documents \
               WHERE cosine_distance(embedding, query) < 0.3 \
               ORDER BY cosine_distance(embedding, query) \
               LIMIT 20";
    let result = sql_to_relexpr(sql).expect("should parse vector with projection");

    // Should have TopK at the top
    match result {
        RelExpr::TopK { k, .. } => {
            assert_eq!(k, 20);
        }
        _ => panic!("expected TopK, got {result:?}"),
    }
}

// ---- COLLATE (postfix operator) + aggregate FILTER (WHERE ...) ----
//
// COLLATE parses to a `__collate(expr, 'name')` Function marker; aggregate
// FILTER parses to a `__filter(pred)` sentinel arg on the aggregate call
// (or `__window_filter(pred)` inside a `__window_*` marker with OVER). We
// assert the markers are present in the parsed tree so the emitter can
// round-trip them (the emitter round-trip is covered in ra-dialect).

/// True if any `Expr` in the tree (projection columns / predicates) contains a
/// Function whose name equals `marker` (searched recursively through exprs).
fn has_expr_marker(r: &RelExpr, marker: &str) -> bool {
    fn in_expr(e: &Expr, marker: &str) -> bool {
        match e {
            Expr::Function { name, args } => {
                name == marker || args.iter().any(|a| in_expr(a, marker))
            }
            Expr::BinOp { left, right, .. } => in_expr(left, marker) || in_expr(right, marker),
            Expr::UnaryOp { operand, .. } => in_expr(operand, marker),
            Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => in_expr(expr, marker),
            Expr::Case {
                operand,
                when_clauses,
                else_result,
            } => {
                operand.as_deref().is_some_and(|o| in_expr(o, marker))
                    || when_clauses
                        .iter()
                        .any(|(w, t)| in_expr(w, marker) || in_expr(t, marker))
                    || else_result.as_deref().is_some_and(|el| in_expr(el, marker))
            }
            Expr::Array(items) => items.iter().any(|a| in_expr(a, marker)),
            _ => false,
        }
    }
    fn in_rel(r: &RelExpr, marker: &str) -> bool {
        let hit = match r {
            RelExpr::Project { columns, .. } => columns.iter().any(|c| in_expr(&c.expr, marker)),
            RelExpr::Filter { predicate, .. } => in_expr(predicate, marker),
            _ => false,
        };
        hit || r.children().into_iter().any(|c| in_rel(c, marker))
    }
    in_rel(r, marker)
}

#[test]
fn test_collate_in_select() {
    let sql = "SELECT x COLLATE \"C\" FROM t";
    let plan = sql_to_relexpr(sql).expect("COLLATE in select should parse");
    assert!(
        has_expr_marker(&plan, "__collate"),
        "expected a __collate marker in the projection, got {plan:?}"
    );
}

#[test]
fn test_collate_in_where() {
    let sql = "SELECT a FROM t WHERE name COLLATE \"C\" = 'x'";
    let plan = sql_to_relexpr(sql).expect("COLLATE in WHERE should parse");
    assert!(
        has_expr_marker(&plan, "__collate"),
        "expected a __collate marker in the predicate, got {plan:?}"
    );
}

// ---- ROW(...) constructor (Codeberg #25) ----
//
// The bare tuple `(a, b)` and the explicit `ROW(...)` keyword form both parse
// to a `__row_constructor(...)` Function marker. The single-element `ROW(a)`
// and empty `ROW()` are only expressible via the keyword form.

#[test]
fn test_row_single_element() {
    let sql = "SELECT ROW(a) FROM t";
    let plan = sql_to_relexpr(sql).expect("ROW(a) should parse");
    assert!(
        has_expr_marker(&plan, "__row_constructor"),
        "expected a __row_constructor marker in the projection, got {plan:?}"
    );
}

#[test]
fn test_row_multi_element() {
    let sql = "SELECT ROW(a, b, c) FROM t";
    let plan = sql_to_relexpr(sql).expect("ROW(a, b, c) should parse");
    assert!(
        has_expr_marker(&plan, "__row_constructor"),
        "expected a __row_constructor marker in the projection, got {plan:?}"
    );
}

#[test]
fn test_row_empty() {
    let sql = "SELECT ROW()";
    let plan = sql_to_relexpr(sql).expect("ROW() should parse");
    assert!(
        has_expr_marker(&plan, "__row_constructor"),
        "expected a __row_constructor marker for empty ROW(), got {plan:?}"
    );
}

#[test]
fn test_row_bare_tuple_still_parses() {
    let sql = "SELECT (a, b) FROM t";
    let plan = sql_to_relexpr(sql).expect("bare tuple (a, b) should parse");
    assert!(
        has_expr_marker(&plan, "__row_constructor"),
        "bare tuple must also produce __row_constructor, got {plan:?}"
    );
}

#[test]
fn test_agg_filter() {
    let sql = "SELECT count(*) FILTER (WHERE x > 0) FROM t";
    let plan = sql_to_relexpr(sql).expect("aggregate FILTER should parse");
    assert!(
        has_expr_marker(&plan, "__filter"),
        "expected a __filter sentinel on the aggregate, got {plan:?}"
    );
    // Faithful representation (not a CASE rewrite): the aggregate is preserved.
    assert!(
        has_node(&plan, |r| matches!(r, RelExpr::Aggregate { .. })),
        "expected an Aggregate node"
    );
}

#[test]
fn test_agg_filter_over() {
    let sql = "SELECT sum(v) FILTER (WHERE k = 1) OVER (PARTITION BY g) FROM t";
    let plan = sql_to_relexpr(sql).expect("aggregate FILTER + OVER should parse");
    assert!(
        has_expr_marker(&plan, "__window_filter"),
        "expected a __window_filter sentinel in the window marker, got {plan:?}"
    );
    assert!(
        has_node(&plan, |r| matches!(r, RelExpr::Window { .. })),
        "expected a Window node"
    );
}

#[test]
fn test_collate_inside_agg_filter() {
    // Combined form from the PG regress corpus.
    let sql = "SELECT max(a COLLATE \"C\") FILTER (WHERE i <> 0) FROM t";
    let plan = sql_to_relexpr(sql).expect("COLLATE inside FILTERed agg should parse");
    assert!(has_expr_marker(&plan, "__collate"), "expected __collate");
    assert!(has_expr_marker(&plan, "__filter"), "expected __filter");
}

// ---- Codeberg #25: column-alias lists on derived tables / VALUES ----
// `(subquery) AS v(c1,c2)` and `(VALUES ...) AS v(col)`. Represented as a
// rename `Project` wrapped in `SubqueryAlias` (no RelExpr model change).

/// Find the first `SubqueryAlias` anywhere in the tree and return its inner
/// relation.
fn find_subquery_alias(r: &RelExpr) -> Option<(&str, &RelExpr)> {
    if let RelExpr::SubqueryAlias { alias, input } = r {
        return Some((alias.as_str(), input.as_ref()));
    }
    r.children().into_iter().find_map(find_subquery_alias)
}

/// Collect the output-column aliases of a `Project` (None where absent).
fn project_aliases(r: &RelExpr) -> Option<Vec<Option<String>>> {
    if let RelExpr::Project { columns, .. } = r {
        Some(columns.iter().map(|c| c.alias.clone()).collect())
    } else {
        None
    }
}

#[test]
fn derived_table_column_alias_renames_in_place() {
    // `(SELECT a, b FROM t) AS v(x, y)` renames the subquery's top-level
    // projection aliases positionally: a -> x, b -> y.
    let sql = "SELECT x FROM (SELECT a, b FROM t) AS v(x, y)";
    let plan = sql_to_relexpr(sql).expect("derived-table col-alias should parse");
    let (alias, input) = find_subquery_alias(&plan).expect("expected a SubqueryAlias");
    assert_eq!(alias, "v", "derived table keeps its alias name");
    let aliases = project_aliases(input).expect("subquery input should be a Project");
    assert_eq!(
        aliases,
        vec![Some("x".to_owned()), Some("y".to_owned())],
        "output columns renamed positionally to x, y"
    );
}

#[test]
fn values_column_alias_single() {
    // `(VALUES (1),(2)) AS v(col)` wraps the VALUES in a positional rename
    // Project (column1 AS col), since Values has no model-level column names.
    let sql = "SELECT col FROM (VALUES (1),(2)) AS v(col)";
    let plan = sql_to_relexpr(sql).expect("VALUES single col-alias should parse");
    let (alias, input) = find_subquery_alias(&plan).expect("expected a SubqueryAlias");
    assert_eq!(alias, "v");
    let aliases = project_aliases(input).expect("subquery input should be a rename Project");
    assert_eq!(aliases, vec![Some("col".to_owned())]);
    // The rename Project must sit over a Values relation.
    if let RelExpr::Project { input: inner, .. } = input {
        assert!(
            has_node(inner, |r| matches!(r, RelExpr::Values { .. })),
            "rename Project should wrap the VALUES relation, got {inner:?}"
        );
    } else {
        panic!("expected a rename Project, got {input:?}");
    }
}

#[test]
fn values_column_alias_multi() {
    let sql = "SELECT x, y FROM (VALUES (1,2),(3,4)) AS v(x, y)";
    let plan = sql_to_relexpr(sql).expect("VALUES multi col-alias should parse");
    let (alias, input) = find_subquery_alias(&plan).expect("expected a SubqueryAlias");
    assert_eq!(alias, "v");
    let aliases = project_aliases(input).expect("subquery input should be a rename Project");
    assert_eq!(aliases, vec![Some("x".to_owned()), Some("y".to_owned())]);
}

#[test]
fn plain_subquery_alias_still_parses_without_rename() {
    // Regression: `(subquery) AS v` (no column list) must still parse and must
    // NOT force output aliases (a, b keep their own names).
    let sql = "SELECT a FROM (SELECT a, b FROM t) AS v";
    let plan = sql_to_relexpr(sql).expect("plain derived-table alias should parse");
    let (alias, input) = find_subquery_alias(&plan).expect("expected a SubqueryAlias");
    assert_eq!(alias, "v");
    let aliases = project_aliases(input).expect("subquery input should be a Project");
    assert_eq!(
        aliases,
        vec![None, None],
        "no column list -> no forced output aliases"
    );
}

// ---- Codeberg #25: PostgreSQL bit-shift operators (<<, >>) ----
// Represented as function-call markers __shl / __shr (like the -> / @>
// JSON operators) to avoid BinOp enum churn through the e-graph.

#[test]
fn shl_operator_parses_as_marker() {
    let sql = "SELECT a << 2 FROM t";
    let plan = sql_to_relexpr(sql).expect("<< should parse");
    assert!(
        has_expr_marker(&plan, "__shl"),
        "expected a __shl marker in the projection, got {plan:?}"
    );
}

#[test]
fn shr_operator_parses_as_marker() {
    let sql = "SELECT b >> 1 FROM t";
    let plan = sql_to_relexpr(sql).expect(">> should parse");
    assert!(
        has_expr_marker(&plan, "__shr"),
        "expected a __shr marker in the projection, got {plan:?}"
    );
}

#[test]
fn shift_in_where_parses() {
    // `<<` / `>>` bind tighter than comparison, so `(flags >> 3) = 1`
    // parses with the shift under the equality.
    let sql = "SELECT a FROM t WHERE flags >> 3 = 1";
    let plan = sql_to_relexpr(sql).expect("shift in WHERE should parse");
    assert!(
        has_expr_marker(&plan, "__shr"),
        "expected a __shr marker in the filter, got {plan:?}"
    );
}

#[test]
fn schema_qualified_insert_target() {
    // #25: INSERT INTO schema.table — target relation name is "schema.table".
    let plan = sql_to_relexpr("INSERT INTO myschema.t (a, b) VALUES (1, 2)")
        .expect("schema-qualified insert should parse");
    assert!(
        format!("{plan:?}").contains("myschema.t"),
        "target should be schema.table, got: {plan:?}"
    );
    // bare-IDENT insert still parses.
    sql_to_relexpr("INSERT INTO t (a) VALUES (1)").expect("bare insert should parse");
}

#[test]
fn schema_qualified_update_target() {
    // #25: UPDATE schema.table [AS alias].
    let plan = sql_to_relexpr("UPDATE s.t SET a = 1 WHERE b = 2")
        .expect("schema-qualified update should parse");
    assert!(format!("{plan:?}").contains("s.t"), "got: {plan:?}");
    sql_to_relexpr("UPDATE s.t AS x SET a = 1").expect("aliased schema update should parse");
    sql_to_relexpr("UPDATE t SET a = 1").expect("bare update should parse");
}

#[test]
fn schema_qualified_delete_target() {
    // #25: DELETE FROM schema.table [AS alias].
    let plan = sql_to_relexpr("DELETE FROM s.t WHERE a = 1")
        .expect("schema-qualified delete should parse");
    assert!(format!("{plan:?}").contains("s.t"), "got: {plan:?}");
    sql_to_relexpr("DELETE FROM s.t AS x WHERE a = 1").expect("aliased schema delete should parse");
    sql_to_relexpr("DELETE FROM t WHERE a = 1").expect("bare delete should parse");
}

#[test]
fn array_type_cast_colon() {
    // #25: expr::type[] array-type cast (precedence: COLONCOLON < LBRACKET so
    // the type_name array suffix shifts instead of an empty subscript).
    let plan = sql_to_relexpr("SELECT x::int[] FROM t").expect("x::int[] should parse");
    assert!(format!("{plan:?}").contains("int[]"), "got: {plan:?}");
    sql_to_relexpr("SELECT '{4,140}'::float8[]").expect("string::float8[] should parse");
    sql_to_relexpr("SELECT x::text[] FROM t").expect("x::text[] should parse");
}

#[test]
fn array_type_cast_function() {
    // #25: CAST(expr AS type[]).
    sql_to_relexpr("SELECT CAST(x AS int[]) FROM t").expect("CAST AS int[] should parse");
}

#[test]
fn cast_precedence_unchanged() {
    // The COLONCOLON precedence addition must not change existing cast parsing.
    sql_to_relexpr("SELECT x::int + 1 FROM t").expect("x::int + 1");
    sql_to_relexpr("SELECT (x + 1)::int FROM t").expect("(x+1)::int");
    sql_to_relexpr("SELECT x::numeric(10,2) FROM t").expect("x::numeric(10,2)");
    // subscript of an array cast still composes when parenthesized.
    sql_to_relexpr("SELECT (x::int[])[1] FROM t").expect("(x::int[])[1]");
    // plain subscript still parses.
    sql_to_relexpr("SELECT a[1] FROM t").expect("a[1]");
}

#[test]
fn multiword_typed_literal_with_time_zone() {
    // #25: `timestamp with time zone 'lit'` / `time with time zone 'lit'` —
    // the type is discarded, the string value is a Const::String.
    sql_to_relexpr("SELECT timestamp with time zone '2024-01-01 00:00:00+00'")
        .expect("timestamp with time zone should parse");
    sql_to_relexpr("SELECT time with time zone '12:00:00+00'")
        .expect("time with time zone should parse");
    // The WITH-anchored form must not break implicit column aliasing.
    let plan = sql_to_relexpr("SELECT a b FROM t").expect("SELECT a b (implicit alias)");
    assert!(
        format!("{plan:?}").contains("\"b\"") || format!("{plan:?}").contains("alias"),
        "got: {plan:?}"
    );
}

// ---- Codeberg #25: DEFAULT-as-value in VALUES, FROM/DELETE/UPDATE ONLY ----

/// The first row of the topmost Values relation in the tree.
fn first_values_row(r: &RelExpr) -> Option<&Vec<Expr>> {
    match find_node(r, |n| matches!(n, RelExpr::Values { .. })) {
        Some(RelExpr::Values { rows }) => rows.first(),
        _ => None,
    }
}

fn is_default_marker(e: &Expr) -> bool {
    matches!(e, Expr::Function { name, args } if name.eq_ignore_ascii_case("__default") && args.is_empty())
}

#[test]
fn default_value_in_values_single() {
    // Bare VALUES with a single DEFAULT column.
    let plan = sql_to_relexpr("VALUES (default)").expect("VALUES (default) should parse");
    let row = first_values_row(&plan).expect("expected a Values row");
    assert_eq!(row.len(), 1, "one column, got {row:?}");
    assert!(
        is_default_marker(&row[0]),
        "expected __default marker, got {row:?}"
    );
}

#[test]
fn default_value_in_values_mixed() {
    // DEFAULT mixed with real values — marker in slot 0, constants after.
    let plan =
        sql_to_relexpr("VALUES (default, 11, 12)").expect("VALUES (default, 11, 12) should parse");
    let row = first_values_row(&plan).expect("expected a Values row");
    assert_eq!(row.len(), 3, "three columns, got {row:?}");
    assert!(
        is_default_marker(&row[0]),
        "slot 0 should be __default, got {row:?}"
    );
    assert!(
        !is_default_marker(&row[1]),
        "slot 1 should be a constant, got {row:?}"
    );

    // Also reachable through a full INSERT ... VALUES (default, ...).
    sql_to_relexpr("INSERT INTO t (a, b) VALUES (default, 5)")
        .expect("INSERT ... VALUES (default, 5) should parse");
}

#[test]
fn from_only_scans_bare_table() {
    // ONLY t restricts to a table excluding inheritance children. The bare
    // relation name matches PG's tables() fact (so the parse oracle stays
    // equal), and the `only` flag is carried on RelExpr::Scan so the emitter
    // re-prints ONLY (Codeberg #28: dropping it is a wrong answer).
    let plan =
        sql_to_relexpr("SELECT avg(x) FROM ONLY student").expect("FROM ONLY student should parse");
    assert!(
        has_node(
            &plan,
            |n| matches!(n, RelExpr::Scan { table, only, .. } if table == "student" && *only)
        ),
        "expected Scan(student, only=true), got {plan:?}"
    );
    assert!(
        !has_node(
            &plan,
            |n| matches!(n, RelExpr::Scan { table, .. } if table.contains("only") || table.contains("ONLY"))
        ),
        "ONLY must not leak into the relation name: {plan:?}"
    );

    // Plain FROM (no ONLY) must NOT set the flag.
    let plain = sql_to_relexpr("SELECT avg(x) FROM student").expect("FROM student should parse");
    assert!(
        has_node(
            &plain,
            |n| matches!(n, RelExpr::Scan { table, only, .. } if table == "student" && !*only)
        ),
        "expected Scan(student, only=false) for plain FROM, got {plain:?}"
    );

    // ONLY with an alias keeps the alias, the bare table name, and only=true.
    let aliased =
        sql_to_relexpr("SELECT * FROM ONLY road r").expect("FROM ONLY road r should parse");
    assert!(
        has_node(
            &aliased,
            |n| matches!(n, RelExpr::Scan { table, alias, only } if table == "road" && alias.as_deref() == Some("r") && *only)
        ),
        "expected Scan(road AS r, only=true), got {aliased:?}"
    );
}

#[test]
fn delete_and_update_only_parse() {
    // DELETE FROM ONLY t and UPDATE ONLY t parse to DML envelopes carrying the
    // ONLY flag (Codeberg #28).
    let del = sql_to_relexpr("DELETE FROM ONLY parent WHERE a = 1")
        .expect("DELETE FROM ONLY parent should parse");
    assert!(
        matches!(&del, RelExpr::Delete { table, only, .. } if table == "parent" && *only),
        "expected Delete(parent, only=true), got {del:?}"
    );
    let upd = sql_to_relexpr("UPDATE ONLY t SET a = 1").expect("UPDATE ONLY t should parse");
    assert!(
        matches!(&upd, RelExpr::Update { table, only, .. } if table == "t" && *only),
        "expected Update(t, only=true), got {upd:?}"
    );

    // Plain DELETE/UPDATE (no ONLY) must NOT set the flag.
    let del0 = sql_to_relexpr("DELETE FROM parent WHERE a = 1").expect("plain DELETE should parse");
    assert!(
        matches!(&del0, RelExpr::Delete { only, .. } if !*only),
        "expected Delete(only=false) for plain DELETE, got {del0:?}"
    );
    let upd0 = sql_to_relexpr("UPDATE t SET a = 1").expect("plain UPDATE should parse");
    assert!(
        matches!(&upd0, RelExpr::Update { only, .. } if !*only),
        "expected Update(only=false) for plain UPDATE, got {upd0:?}"
    );
}

#[test]
fn cast_preserves_typmod() {
    // #28: typmod on casts must survive parse->emit (dropping it truncates
    // strings / flips bit() comparisons). Verified via the Debug tree carrying
    // the typmod in the cast type string.
    for (sql, ty) in [
        ("SELECT x::char(9) FROM t", "char(9)"),
        ("SELECT x::bit(32) FROM t", "bit(32)"),
        ("SELECT x::numeric(10,2) FROM t", "numeric(10,2)"),
    ] {
        let plan = sql_to_relexpr(sql).expect("typmod cast should parse");
        assert!(
            format!("{plan:?}").contains(ty),
            "expected {ty} in {plan:?}"
        );
    }
    // plain cast unaffected.
    sql_to_relexpr("SELECT x::int FROM t").expect("plain cast");
}
