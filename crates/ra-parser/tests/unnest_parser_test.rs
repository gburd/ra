//! Tests for UNNEST and table function SQL parsing.
//!
//! Validates that `sql_to_relexpr` correctly converts SQL
//! UNNEST, generate_series, and array expressions into the
//! appropriate `RelExpr` and `Expr` nodes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ra_core::algebra::RelExpr;
use ra_core::expr::{Const, Expr};
use ra_parser::sql_to_relexpr;

// ── UNNEST parsing ────────────────────────────────────────

#[test]
fn parse_unnest_array_literal() {
    let sql = "SELECT * FROM UNNEST(ARRAY[1, 2, 3]) AS t(val)";
    let expr = sql_to_relexpr(sql).expect("UNNEST with array literal should parse");

    // The top level should be a Project wrapping an Unnest
    fn find_unnest(e: &RelExpr) -> bool {
        match e {
            RelExpr::Unnest { .. } => true,
            _ => e.children().iter().any(|c| find_unnest(c)),
        }
    }
    assert!(
        find_unnest(&expr),
        "should contain an Unnest node, got: {expr:?}"
    );
}

#[test]
#[ignore] // UNNEST WITH ORDINALITY not yet implemented
fn parse_unnest_with_ordinality() {
    let sql = "\
        SELECT * FROM UNNEST(ARRAY[10, 20, 30]) \
        WITH ORDINALITY AS t(val, ord)";
    let expr = sql_to_relexpr(sql).expect("UNNEST WITH ORDINALITY should parse");

    fn find_unnest_ordinality(e: &RelExpr) -> bool {
        match e {
            RelExpr::Unnest {
                with_ordinality, ..
            } => *with_ordinality,
            _ => e.children().iter().any(|c| find_unnest_ordinality(c)),
        }
    }
    assert!(
        find_unnest_ordinality(&expr),
        "should have with_ordinality = true"
    );
}

#[test]
fn parse_unnest_column_ref() {
    // UNNEST used as a function call (not keyword syntax)
    let sql = "SELECT * FROM unnest(ARRAY[1, 2]) AS vals";
    let result = sql_to_relexpr(sql);
    // This may parse as either UNNEST keyword or Function -
    // either way it should succeed
    assert!(
        result.is_ok(),
        "unnest function call should parse: {result:?}"
    );
}

// ── generate_series parsing ───────────────────────────────

#[test]
fn parse_generate_series_basic() {
    let sql = "SELECT * FROM generate_series(1, 10) AS gs(val)";
    let expr = sql_to_relexpr(sql).expect("generate_series should parse");

    fn find_table_func(e: &RelExpr) -> Option<String> {
        match e {
            RelExpr::TableFunction { name, .. } => Some(name.clone()),
            _ => e.children().iter().find_map(|c| find_table_func(c)),
        }
    }

    assert_eq!(
        find_table_func(&expr),
        Some("generate_series".to_owned()),
        "should contain a generate_series TableFunction"
    );
}

#[test]
fn parse_generate_series_with_step() {
    let sql = "SELECT * FROM generate_series(1, 100, 10) AS gs";
    let expr = sql_to_relexpr(sql).expect("generate_series with step should parse");

    fn count_args(e: &RelExpr) -> Option<usize> {
        match e {
            RelExpr::TableFunction { args, .. } => Some(args.len()),
            _ => e.children().iter().find_map(|c| count_args(c)),
        }
    }

    assert_eq!(count_args(&expr), Some(3), "should have 3 arguments");
}

// ── Array expression parsing ──────────────────────────────

#[test]
fn parse_array_literal_in_select() {
    let sql = "SELECT ARRAY[1, 2, 3]";
    let expr = sql_to_relexpr(sql).expect("ARRAY literal in SELECT should parse");

    // Dig into the projection to find the Array expr
    fn find_array(e: &RelExpr) -> bool {
        match e {
            RelExpr::Project { columns, .. } => {
                columns.iter().any(|c| matches!(&c.expr, Expr::Array(_)))
            }
            _ => e.children().iter().any(|c| find_array(c)),
        }
    }

    assert!(find_array(&expr), "should contain an Array expression");
}

#[test]
#[ignore] // Array subscript not yet supported in expression converter
fn parse_array_subscript() {
    let sql = "SELECT arr[2] FROM t";
    let expr = sql_to_relexpr(sql).expect("array subscript should parse");

    fn find_array_index(e: &RelExpr) -> bool {
        match e {
            RelExpr::Project { columns, .. } => columns
                .iter()
                .any(|c| matches!(&c.expr, Expr::ArrayIndex(_, _))),
            _ => e.children().iter().any(|c| find_array_index(c)),
        }
    }

    assert!(
        find_array_index(&expr),
        "should contain an ArrayIndex expression"
    );
}

// ── Empty array / edge cases ──────────────────────────────

#[test]
fn parse_empty_array() {
    let sql = "SELECT ARRAY[]";
    let result = sql_to_relexpr(sql);
    // Empty ARRAY literal may or may not parse depending
    // on dialect; if it does, verify the structure
    if let Ok(expr) = result {
        fn find_empty_array(e: &RelExpr) -> bool {
            match e {
                RelExpr::Project { columns, .. } => columns.iter().any(|c| {
                    matches!(
                        &c.expr,
                        Expr::Array(elems) if elems.is_empty()
                    )
                }),
                _ => e.children().iter().any(|c| find_empty_array(c)),
            }
        }
        assert!(find_empty_array(&expr));
    }
    // If it doesn't parse, that's acceptable too
}

#[test]
fn parse_nested_array() {
    let sql = "SELECT ARRAY[ARRAY[1, 2], ARRAY[3, 4]]";
    let result = sql_to_relexpr(sql);
    assert!(result.is_ok(), "nested arrays should parse: {result:?}");
}

// ── Array in WHERE clause ─────────────────────────────────

#[test]
#[ignore] // Array subscript not yet supported in expression converter
fn parse_array_subscript_in_where() {
    let sql = "SELECT * FROM t WHERE arr[1] = 42";
    let expr = sql_to_relexpr(sql).expect("array subscript in WHERE should parse");

    fn find_filter_with_array_index(e: &RelExpr) -> bool {
        match e {
            RelExpr::Filter { predicate, .. } => contains_array_index(predicate),
            _ => e.children().iter().any(|c| find_filter_with_array_index(c)),
        }
    }

    fn contains_array_index(e: &Expr) -> bool {
        match e {
            Expr::ArrayIndex(_, _) => true,
            Expr::BinOp { left, right, .. } => {
                contains_array_index(left) || contains_array_index(right)
            }
            _ => false,
        }
    }

    assert!(
        find_filter_with_array_index(&expr),
        "filter should contain ArrayIndex"
    );
}

// ── UNNEST in JOIN context ────────────────────────────────

#[test]
fn parse_unnest_in_cross_join() {
    let sql = "\
        SELECT t.id, u.val \
        FROM t \
        CROSS JOIN UNNEST(ARRAY[1, 2, 3]) AS u(val)";
    let result = sql_to_relexpr(sql);
    assert!(
        result.is_ok(),
        "UNNEST in CROSS JOIN should parse: {result:?}"
    );
}

// ── Builder method tests ──────────────────────────────────

#[test]
fn unnest_builder_sets_defaults() {
    let expr = RelExpr::unnest(Expr::Const(Const::Int(1)), None);

    if let RelExpr::Unnest {
        alias,
        input,
        with_ordinality,
        ..
    } = &expr
    {
        assert!(alias.is_none());
        assert!(input.is_none());
        assert!(!with_ordinality);
    } else {
        panic!("expected Unnest");
    }
}

#[test]
fn table_function_builder() {
    let expr = RelExpr::table_function(
        "my_func",
        vec![Expr::Const(Const::Int(42))],
        vec![("col1".to_owned(), "TEXT".to_owned())],
    );

    if let RelExpr::TableFunction {
        name,
        args,
        columns,
        input,
    } = &expr
    {
        assert_eq!(name, "my_func");
        assert_eq!(args.len(), 1);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].0, "col1");
        assert!(input.is_none());
    } else {
        panic!("expected TableFunction");
    }
}
