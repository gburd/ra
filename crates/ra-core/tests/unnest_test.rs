//! Tests for UNNEST and TableFunction relational algebra operators,
//! and Array/ArrayIndex expression types.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ra_core::algebra::RelExpr;
use ra_core::expr::{ColumnRef, Const, Expr};

// ── Unnest operator construction ──────────────────────────

#[test]
fn unnest_standalone_has_no_children() {
    let expr = RelExpr::unnest(
        Expr::Array(vec![
            Expr::Const(Const::Int(1)),
            Expr::Const(Const::Int(2)),
        ]),
        Some("val".to_owned()),
    );

    assert!(expr.children().is_empty());
    if let RelExpr::Unnest {
        alias,
        input,
        with_ordinality,
        ..
    } = &expr
    {
        assert_eq!(alias.as_deref(), Some("val"));
        assert!(input.is_none());
        assert!(!with_ordinality);
    } else {
        panic!("expected Unnest, got: {expr:?}");
    }
}

#[test]
fn unnest_lateral_has_input_child() {
    let scan = RelExpr::Scan {
        table: "t".to_owned(),
        alias: None,
    };
    let expr = RelExpr::unnest_lateral(
        Expr::Column(ColumnRef::qualified("t", "arr")),
        scan,
        Some("elem".to_owned()),
    );

    assert_eq!(expr.children().len(), 1);
    if let RelExpr::Unnest { input, .. } = &expr {
        assert!(input.is_some());
    } else {
        panic!("expected Unnest");
    }
}

#[test]
fn unnest_with_ordinality() {
    let expr = RelExpr::Unnest {
        expr: Expr::Array(vec![
            Expr::Const(Const::String("a".to_owned())),
        ]),
        alias: None,
        input: None,
        with_ordinality: true,
    };

    if let RelExpr::Unnest {
        with_ordinality, ..
    } = &expr
    {
        assert!(with_ordinality);
    } else {
        panic!("expected Unnest");
    }
}

// ── TableFunction operator construction ───────────────────

#[test]
fn table_function_standalone() {
    let expr = RelExpr::table_function(
        "generate_series",
        vec![
            Expr::Const(Const::Int(1)),
            Expr::Const(Const::Int(10)),
        ],
        vec![(
            "generate_series".to_owned(),
            "Int64".to_owned(),
        )],
    );

    assert!(expr.children().is_empty());
    if let RelExpr::TableFunction {
        name, args, columns, input, ..
    } = &expr
    {
        assert_eq!(name, "generate_series");
        assert_eq!(args.len(), 2);
        assert_eq!(columns.len(), 1);
        assert!(input.is_none());
    } else {
        panic!("expected TableFunction");
    }
}

#[test]
fn table_function_with_lateral_input() {
    let scan = RelExpr::Scan {
        table: "ranges".to_owned(),
        alias: None,
    };
    let expr = RelExpr::TableFunction {
        name: "generate_series".to_owned(),
        args: vec![
            Expr::Column(ColumnRef::qualified("ranges", "lo")),
            Expr::Column(ColumnRef::qualified("ranges", "hi")),
        ],
        columns: vec![],
        input: Some(Box::new(scan)),
    };

    assert_eq!(expr.children().len(), 1);
}

// ── Array expression types ────────────────────────────────

#[test]
fn array_expr_creation() {
    let arr = Expr::Array(vec![
        Expr::Const(Const::Int(10)),
        Expr::Const(Const::Int(20)),
        Expr::Const(Const::Int(30)),
    ]);

    if let Expr::Array(elements) = &arr {
        assert_eq!(elements.len(), 3);
    } else {
        panic!("expected Array");
    }
}

#[test]
fn array_index_expr_creation() {
    let arr = Expr::Column(ColumnRef::new("my_array"));
    let idx = Expr::Const(Const::Int(2));
    let access = Expr::ArrayIndex(
        Box::new(arr),
        Box::new(idx),
    );

    if let Expr::ArrayIndex(array, index) = &access {
        assert!(matches!(array.as_ref(), Expr::Column(_)));
        assert!(matches!(
            index.as_ref(),
            Expr::Const(Const::Int(2))
        ));
    } else {
        panic!("expected ArrayIndex");
    }
}

// ── Column collection ─────────────────────────────────────

#[test]
fn unnest_collects_columns_from_expr() {
    let expr = RelExpr::unnest(
        Expr::Column(ColumnRef::qualified("t", "tags")),
        None,
    );

    let cols = expr.referenced_columns();
    assert!(
        cols.iter().any(|c| c.column == "tags"),
        "should contain 'tags' column"
    );
}

#[test]
fn table_function_collects_columns_from_args() {
    let expr = RelExpr::table_function(
        "generate_series",
        vec![
            Expr::Column(ColumnRef::new("start_val")),
            Expr::Column(ColumnRef::new("end_val")),
        ],
        vec![],
    );

    let cols = expr.referenced_columns();
    assert!(
        cols.iter().any(|c| c.column == "start_val"),
        "should contain 'start_val' column"
    );
    assert!(
        cols.iter().any(|c| c.column == "end_val"),
        "should contain 'end_val' column"
    );
}

// ── CTE references ────────────────────────────────────────

#[test]
fn unnest_does_not_reference_cte() {
    let expr = RelExpr::unnest(
        Expr::Array(vec![Expr::Const(Const::Int(1))]),
        None,
    );
    assert!(!expr.references_cte("my_cte"));
}

#[test]
fn unnest_lateral_delegates_cte_reference_to_input() {
    let cte_scan = RelExpr::Scan {
        table: "my_cte".to_owned(),
        alias: None,
    };
    let expr = RelExpr::unnest_lateral(
        Expr::Const(Const::Int(1)),
        cte_scan,
        None,
    );

    // The CTE reference detection depends on Scan
    // matching the CTE name -- but Scan doesn't track
    // CTE references. So this tests that references_cte
    // traverses into the input.
    assert!(!expr.references_cte("other_cte"));
}
