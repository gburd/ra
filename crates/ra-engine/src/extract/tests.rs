#![expect(clippy::expect_used, clippy::panic, clippy::unwrap_used, reason = "test code")]

use std::collections::HashMap;

use egg::RecExpr;
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_stats::accuracy::Staleness;

use crate::egraph::{to_rec_expr, EGraphError, Optimizer, RelLang};

use super::api::{extract_best, extract_best_with_staleness};
use super::convert::rec_expr_to_rel_expr;
use super::cost::RelCostFn;

#[test]
fn extract_simple_scan() {
    let expr = RelExpr::scan("users");
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn extract_filter() {
    let expr = RelExpr::scan("users").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("age"))),
        right: Box::new(Expr::Const(Const::Int(18))),
    });
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn extract_join() {
    let expr = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
        },
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn extract_limit() {
    let expr = RelExpr::scan("users").limit(10, 5);
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn extract_union() {
    let expr = RelExpr::Union {
        all: true,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn extract_project() {
    let expr = RelExpr::scan("users").project(vec![
        ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("name")),
            alias: None,
        },
        ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("age")),
            alias: Some("user_age".into()),
        },
    ]);
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn extract_aggregate() {
    let expr = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("dept"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: None,
            distinct: false,
            alias: Some("cnt".into()),
        }],
        input: Box::new(RelExpr::scan("employees")),
    };
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn extract_sort() {
    let expr = RelExpr::Sort {
        keys: vec![SortKey {
            expr: Expr::Column(ColumnRef::new("name")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(RelExpr::scan("users")),
    };
    let rec = to_rec_expr(&expr).expect("conversion should succeed");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
    assert_eq!(result, expr);
}

#[test]
fn optimizer_selects_plan() {
    let mut optimizer = Optimizer::new();
    optimizer.add_table_stats(
        "big_table",
        ra_core::statistics::Statistics::new(1_000_000.0),
    );
    optimizer.add_table_stats("small_table", ra_core::statistics::Statistics::new(100.0));

    let expr = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Column(ColumnRef::new("b"))),
        },
        left: Box::new(RelExpr::scan("big_table")),
        right: Box::new(RelExpr::scan("small_table")),
    };

    let result = optimizer
        .optimize(&expr)
        .expect("optimization should succeed");
    assert!(matches!(result, RelExpr::Join { .. }));
}

// -- Scan with alias --

#[test]
fn extract_scan_alias() {
    let expr = RelExpr::Scan {
        table: "users".into(),
        alias: Some("u".into()),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Set operations: intersect and except --

#[test]
fn extract_intersect() {
    let expr = RelExpr::Intersect {
        all: false,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_except() {
    let expr = RelExpr::Except {
        all: true,
        left: Box::new(RelExpr::scan("x")),
        right: Box::new(RelExpr::scan("y")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_union_not_all() {
    let expr = RelExpr::Union {
        all: false,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- CTE and RecursiveCTE --

#[test]
fn extract_cte() {
    let expr = RelExpr::CTE {
        name: "temp".into(),
        definition: Box::new(RelExpr::scan("orders")),
        body: Box::new(RelExpr::scan("temp")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_recursive_cte() {
    let expr = RelExpr::RecursiveCTE {
        name: "ancestors".into(),
        base_case: Box::new(RelExpr::scan("people")),
        recursive_case: Box::new(RelExpr::scan("ancestors")),
        body: Box::new(RelExpr::scan("ancestors")),
        cycle_detection: Some(ra_core::algebra::CycleDetection {
            track_columns: vec![],
            max_depth: Some(1000),
            cycle_mark_column: None,
            path_column: None,
        }),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Distinct --

#[test]
fn extract_distinct() {
    let expr = RelExpr::scan("users").distinct();
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Values --

#[test]
fn extract_values() {
    let expr = RelExpr::Values {
        rows: vec![
            vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::String("a".into())),
            ],
            vec![
                Expr::Const(Const::Int(2)),
                Expr::Const(Const::String("b".into())),
            ],
        ],
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Window --

#[test]
fn extract_window_row_number() {
    use ra_core::algebra::{
        WindowExpr as WExpr, WindowFrame, WindowFrameBound, WindowFrameMode, WindowFunction as WFn,
    };
    let expr = RelExpr::Window {
        functions: vec![WExpr {
            function: WFn::RowNumber,
            arg: None,
            partition_by: vec![Expr::Column(ColumnRef::new("dept"))],
            order_by: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("salary")),
                direction: SortDirection::Desc,
                nulls: NullOrdering::Last,
            }],
            frame: Some(WindowFrame {
                mode: WindowFrameMode::Rows,
                start: WindowFrameBound::UnboundedPreceding,
                end: WindowFrameBound::CurrentRow,
            }),
            alias: Some("rn".into()),
        }],
        input: Box::new(RelExpr::scan("employees")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_window_no_frame() {
    use ra_core::algebra::{WindowExpr as WExpr, WindowFunction as WFn};
    let expr = RelExpr::Window {
        functions: vec![WExpr {
            function: WFn::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            partition_by: vec![],
            order_by: vec![],
            frame: None,
            alias: None,
        }],
        input: Box::new(RelExpr::scan("sales")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_window_frame_range_following() {
    use ra_core::algebra::{
        WindowExpr as WExpr, WindowFrame, WindowFrameBound, WindowFrameMode, WindowFunction as WFn,
    };
    let expr = RelExpr::Window {
        functions: vec![WExpr {
            function: WFn::Avg,
            arg: Some(Expr::Column(ColumnRef::new("price"))),
            partition_by: vec![],
            order_by: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("ts")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::First,
            }],
            frame: Some(WindowFrame {
                mode: WindowFrameMode::Range,
                start: WindowFrameBound::Preceding(3),
                end: WindowFrameBound::Following(3),
            }),
            alias: Some("moving_avg".into()),
        }],
        input: Box::new(RelExpr::scan("ticks")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_window_frame_groups_unbounded() {
    use ra_core::algebra::{
        WindowExpr as WExpr, WindowFrame, WindowFrameBound, WindowFrameMode, WindowFunction as WFn,
    };
    let expr = RelExpr::Window {
        functions: vec![WExpr {
            function: WFn::Count,
            arg: None,
            partition_by: vec![],
            order_by: vec![],
            frame: Some(WindowFrame {
                mode: WindowFrameMode::Groups,
                start: WindowFrameBound::UnboundedPreceding,
                end: WindowFrameBound::UnboundedFollowing,
            }),
            alias: None,
        }],
        input: Box::new(RelExpr::scan("t")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- All join types --

#[test]
fn extract_left_outer_join() {
    let cond = Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("id"))),
        right: Box::new(Expr::Column(ColumnRef::new("fk"))),
    };
    let expr = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: cond,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_right_outer_join() {
    let cond = Expr::Const(Const::Bool(true));
    let expr = RelExpr::Join {
        join_type: JoinType::RightOuter,
        condition: cond,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_full_outer_join() {
    let cond = Expr::Const(Const::Bool(true));
    let expr = RelExpr::Join {
        join_type: JoinType::FullOuter,
        condition: cond,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_cross_join() {
    let cond = Expr::Const(Const::Bool(true));
    let expr = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: cond,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_semi_join() {
    let cond = Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("id"))),
        right: Box::new(Expr::Column(ColumnRef::new("ref"))),
    };
    let expr = RelExpr::Join {
        join_type: JoinType::Semi,
        condition: cond,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_anti_join() {
    let cond = Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("id"))),
        right: Box::new(Expr::Column(ColumnRef::new("ref"))),
    };
    let expr = RelExpr::Join {
        join_type: JoinType::Anti,
        condition: cond,
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- All aggregate functions --

#[test]
fn extract_aggregate_sum() {
    let expr = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(RelExpr::scan("sales")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_aggregate_avg_distinct() {
    let expr = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("category"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Avg,
            arg: Some(Expr::Column(ColumnRef::new("price"))),
            distinct: true,
            alias: Some("avg_price".into()),
        }],
        input: Box::new(RelExpr::scan("products")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_aggregate_min_max() {
    let expr = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Min,
                arg: Some(Expr::Column(ColumnRef::new("created"))),
                distinct: false,
                alias: Some("earliest".into()),
            },
            AggregateExpr {
                function: AggregateFunction::Max,
                arg: Some(Expr::Column(ColumnRef::new("created"))),
                distinct: false,
                alias: Some("latest".into()),
            },
        ],
        input: Box::new(RelExpr::scan("events")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- All scalar expression types --

#[test]
fn extract_qualified_column() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::qualified("t", "id"))),
        right: Box::new(Expr::Const(Const::Int(1))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_const_null() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("x"))),
        right: Box::new(Expr::Const(Const::Null)),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_const_bool() {
    let expr = RelExpr::scan("t").filter(Expr::Const(Const::Bool(false)));
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_const_float() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("price"))),
        right: Box::new(Expr::Const(Const::Float(9.99))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_const_string() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("name"))),
        right: Box::new(Expr::Const(Const::String("Alice".into()))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- All binary operators --

#[test]
fn extract_all_binary_operators() {
    let ops = [
        BinOp::Add,
        BinOp::Sub,
        BinOp::Mul,
        BinOp::Div,
        BinOp::Mod,
        BinOp::Eq,
        BinOp::Ne,
        BinOp::Lt,
        BinOp::Le,
        BinOp::Gt,
        BinOp::Ge,
        BinOp::And,
        BinOp::Or,
        BinOp::Concat,
    ];
    for op in &ops {
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: *op,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Column(ColumnRef::new("b"))),
        });
        let rec = to_rec_expr(&expr).unwrap_or_else(|e| panic!("to_rec_expr for {op:?}: {e}"));
        let result =
            rec_expr_to_rel_expr(&rec).unwrap_or_else(|e| panic!("extraction for {op:?}: {e}"));
        assert_eq!(result, expr, "round-trip failed for {op:?}");
    }
}

#[test]
fn extract_json_access() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::JsonAccess,
        left: Box::new(Expr::Column(ColumnRef::new("data"))),
        right: Box::new(Expr::Const(Const::String("key".into()))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Unary operators --

#[test]
fn extract_not() {
    let expr = RelExpr::scan("t").filter(Expr::UnaryOp {
        op: ra_core::expr::UnaryOp::Not,
        operand: Box::new(Expr::Column(ColumnRef::new("active"))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_is_null() {
    let expr = RelExpr::scan("t").filter(Expr::UnaryOp {
        op: ra_core::expr::UnaryOp::IsNull,
        operand: Box::new(Expr::Column(ColumnRef::new("x"))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_is_not_null() {
    let expr = RelExpr::scan("t").filter(Expr::UnaryOp {
        op: ra_core::expr::UnaryOp::IsNotNull,
        operand: Box::new(Expr::Column(ColumnRef::new("x"))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_neg() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::Neg,
            operand: Box::new(Expr::Column(ColumnRef::new("x"))),
        }),
        right: Box::new(Expr::Const(Const::Int(0))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Function calls --

#[test]
fn extract_function_call() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Function {
            name: "LENGTH".into(),
            args: vec![Expr::Column(ColumnRef::new("name"))],
        }),
        right: Box::new(Expr::Const(Const::Int(5))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Array and ArrayIndex --

#[test]
fn extract_array() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Array(vec![
            Expr::Const(Const::Int(1)),
            Expr::Const(Const::Int(2)),
        ])),
        right: Box::new(Expr::Column(ColumnRef::new("arr"))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_array_index() {
    let expr = RelExpr::scan("t").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::ArrayIndex(
            Box::new(Expr::Column(ColumnRef::new("arr"))),
            Box::new(Expr::Const(Const::Int(0))),
        )),
        right: Box::new(Expr::Const(Const::Int(42))),
    });
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Unnest --

#[test]
fn extract_unnest() {
    let expr = RelExpr::Unnest {
        expr: Expr::Column(ColumnRef::new("tags")),
        alias: Some("tag".into()),
        input: None,
        with_ordinality: false,
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_unnest_lateral() {
    let expr = RelExpr::Unnest {
        expr: Expr::Column(ColumnRef::new("items")),
        alias: None,
        input: Some(Box::new(RelExpr::scan("orders"))),
        with_ordinality: true,
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- MultiUnnest --

#[test]
fn extract_multi_unnest() {
    let expr = RelExpr::MultiUnnest {
        exprs: vec![
            Expr::Column(ColumnRef::new("arr1")),
            Expr::Column(ColumnRef::new("arr2")),
        ],
        aliases: vec![Some("a".into()), None],
        with_ordinality: true,
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- TableFunction --

#[test]
fn extract_table_function() {
    let expr = RelExpr::TableFunction {
        name: "generate_series".into(),
        args: vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(10))],
        columns: vec![],
        input: None,
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- IndexOnlyScan --

#[test]
fn extract_index_only_scan() {
    let expr = RelExpr::IndexOnlyScan {
        table: "users".into(),
        index: "idx_users_email".into(),
        columns: vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("email")),
            alias: None,
        }],
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("email"))),
            right: Box::new(Expr::Const(Const::String("test@example.com".into()))),
        },
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    // Physical scans lower to logical Project(Filter(Scan)).
    let expected = RelExpr::Project {
        columns: vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("email")),
            alias: None,
        }],
        input: Box::new(RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("email"))),
                right: Box::new(Expr::Const(Const::String("test@example.com".into()))),
            },
            input: Box::new(RelExpr::Scan { table: "users".into(), alias: None }),
        }),
    };
    assert_eq!(result, expected);
}

// -- MvScan --

#[test]
fn extract_mv_scan_with_alias() {
    let expr = RelExpr::MvScan {
        view_name: "sales_summary".into(),
        alias: Some("ss".into()),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_mv_scan_no_alias() {
    let expr = RelExpr::MvScan {
        view_name: "daily_totals".into(),
        alias: None,
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Bitmap operators --

#[test]
fn extract_bitmap_index_scan() {
    let expr = RelExpr::BitmapIndexScan {
        table: "orders".into(),
        index: "idx_status".into(),
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("status"))),
            right: Box::new(Expr::Const(Const::String("active".into()))),
        },
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    let expected = RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("status"))),
            right: Box::new(Expr::Const(Const::String("active".into()))),
        },
        input: Box::new(RelExpr::Scan { table: "orders".into(), alias: None }),
    };
    assert_eq!(result, expected);
}

#[test]
fn extract_bitmap_and() {
    let scan1 = RelExpr::BitmapIndexScan {
        table: "t".into(),
        index: "idx_a".into(),
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        },
    };
    let scan2 = RelExpr::BitmapIndexScan {
        table: "t".into(),
        index: "idx_b".into(),
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("b"))),
            right: Box::new(Expr::Const(Const::Int(2))),
        },
    };
    let expr = RelExpr::BitmapAnd {
        inputs: vec![Box::new(scan1), Box::new(scan2)],
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    let mk = |c: &str, v: i64| RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(c))),
            right: Box::new(Expr::Const(Const::Int(v))),
        },
        input: Box::new(RelExpr::Scan { table: "t".into(), alias: None }),
    };
    let expected = RelExpr::BitmapAnd { inputs: vec![Box::new(mk("a",1)), Box::new(mk("b",2))] };
    assert_eq!(result, expected);
}

#[test]
fn extract_bitmap_or() {
    let scan1 = RelExpr::BitmapIndexScan {
        table: "t".into(),
        index: "idx_x".into(),
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("x"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        },
    };
    let scan2 = RelExpr::BitmapIndexScan {
        table: "t".into(),
        index: "idx_y".into(),
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("y"))),
            right: Box::new(Expr::Const(Const::Int(2))),
        },
    };
    let expr = RelExpr::BitmapOr {
        inputs: vec![Box::new(scan1), Box::new(scan2)],
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    let mk = |c: &str, v: i64| RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(c))),
            right: Box::new(Expr::Const(Const::Int(v))),
        },
        input: Box::new(RelExpr::Scan { table: "t".into(), alias: None }),
    };
    let expected = RelExpr::BitmapOr { inputs: vec![Box::new(mk("x",1)), Box::new(mk("y",2))] };
    assert_eq!(result, expected);
}

#[test]
fn extract_bitmap_heap_scan() {
    let bitmap = RelExpr::BitmapIndexScan {
        table: "orders".into(),
        index: "idx_date".into(),
        predicate: Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("date"))),
            right: Box::new(Expr::Const(Const::String("2024-01-01".into()))),
        },
    };
    let expr = RelExpr::BitmapHeapScan {
        table: "orders".into(),
        bitmap: Box::new(bitmap),
        recheck_cond: Some(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("date"))),
            right: Box::new(Expr::Const(Const::String("2024-01-01".into()))),
        }),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    // BitmapHeapScan lowers to Filter(Scan) using the recheck condition.
    let expected = RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("date"))),
            right: Box::new(Expr::Const(Const::String("2024-01-01".into()))),
        },
        input: Box::new(RelExpr::Scan { table: "orders".into(), alias: None }),
    };
    assert_eq!(result, expected);
}

// -- Sort with multiple keys and directions --

#[test]
fn extract_sort_multiple_keys() {
    let expr = RelExpr::Sort {
        keys: vec![
            SortKey {
                expr: Expr::Column(ColumnRef::new("dept")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::First,
            },
            SortKey {
                expr: Expr::Column(ColumnRef::new("salary")),
                direction: SortDirection::Desc,
                nulls: NullOrdering::Last,
            },
        ],
        input: Box::new(RelExpr::scan("employees")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Error paths --

#[test]
fn rec_expr_to_rel_expr_empty() {
    let rec: RecExpr<RelLang> = RecExpr::default();
    let err = rec_expr_to_rel_expr(&rec).unwrap_err();
    match err {
        EGraphError::ExtractionError(msg) => {
            assert!(msg.contains("empty"), "expected 'empty' in: {msg}");
        }
        other => panic!("expected ExtractionError, got: {other:?}"),
    }
}

// -- Nested / complex expressions --

#[test]
fn extract_nested_filter_project_join() {
    let expr = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("o", "user_id"))),
        },
        left: Box::new(
            RelExpr::Scan {
                table: "users".into(),
                alias: Some("u".into()),
            }
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
        ),
        right: Box::new(
            RelExpr::Scan {
                table: "orders".into(),
                alias: Some("o".into()),
            }
            .project(vec![
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("user_id")),
                    alias: None,
                },
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("total")),
                    alias: Some("order_total".into()),
                },
            ]),
        ),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

#[test]
fn extract_subquery_filter_aggregate_limit() {
    let inner = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("category"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            distinct: false,
            alias: Some("total".into()),
        }],
        input: Box::new(RelExpr::scan("transactions")),
    };
    let expr = inner
        .filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("total"))),
            right: Box::new(Expr::Const(Const::Int(1000))),
        })
        .limit(10, 0);
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- RelCostFn tests --

#[test]
fn rel_cost_fn_scan_scales_with_bandwidth() {
    use egg::CostFunction;
    let slow = ra_hardware::HardwareProfile {
        storage_bandwidth_gbps: 1.0,
        ..ra_hardware::HardwareProfile::cpu_only()
    };
    let fast = ra_hardware::HardwareProfile {
        storage_bandwidth_gbps: 10.0,
        ..ra_hardware::HardwareProfile::cpu_only()
    };
    let expr = RelExpr::scan("t");
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let nodes = rec.as_ref();

    let cost_slow = {
        let mut cf = RelCostFn::new(slow);
        cf.cost(&nodes[nodes.len() - 1], |child| {
            let mut cf2 = RelCostFn::new(ra_hardware::HardwareProfile {
                storage_bandwidth_gbps: 1.0,
                ..ra_hardware::HardwareProfile::cpu_only()
            });
            cf2.cost(&nodes[usize::from(child)], |_| 0.0)
        })
    };
    let cost_fast = {
        let mut cf = RelCostFn::new(fast);
        cf.cost(&nodes[nodes.len() - 1], |child| {
            let mut cf2 = RelCostFn::new(ra_hardware::HardwareProfile {
                storage_bandwidth_gbps: 10.0,
                ..ra_hardware::HardwareProfile::cpu_only()
            });
            cf2.cost(&nodes[usize::from(child)], |_| 0.0)
        })
    };
    assert!(
        cost_slow > cost_fast,
        "slower bandwidth should cost more: {cost_slow} vs {cost_fast}"
    );
}

#[test]
fn rel_cost_fn_join_scales_with_cache() {
    use egg::CostFunction;
    let small_cache = ra_hardware::HardwareProfile {
        l3_cache_bytes: 8 * 1024 * 1024, // 8MB
        ..ra_hardware::HardwareProfile::cpu_only()
    };
    let large_cache = ra_hardware::HardwareProfile {
        l3_cache_bytes: 64 * 1024 * 1024, // 64MB
        ..ra_hardware::HardwareProfile::cpu_only()
    };

    let expr = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let nodes = rec.as_ref();

    let cost_small = {
        let mut cf = RelCostFn::new(small_cache);
        cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
    };
    let cost_large = {
        let mut cf = RelCostFn::new(large_cache);
        cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
    };
    assert!(
        cost_small > cost_large,
        "small cache should cost more: {cost_small} vs {cost_large}"
    );
}

#[test]
fn rel_cost_fn_sort_scales_with_cores() {
    use egg::CostFunction;
    let few_cores = ra_hardware::HardwareProfile {
        cpu_cores: 2,
        ..ra_hardware::HardwareProfile::cpu_only()
    };
    let many_cores = ra_hardware::HardwareProfile {
        cpu_cores: 32,
        ..ra_hardware::HardwareProfile::cpu_only()
    };

    let expr = RelExpr::Sort {
        keys: vec![SortKey {
            expr: Expr::Column(ColumnRef::new("x")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(RelExpr::scan("t")),
    };
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let nodes = rec.as_ref();

    let cost_few = {
        let mut cf = RelCostFn::new(few_cores);
        cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
    };
    let cost_many = {
        let mut cf = RelCostFn::new(many_cores);
        cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
    };
    assert!(
        cost_few > cost_many,
        "fewer cores should cost more: {cost_few} vs {cost_many}"
    );
}

#[test]
fn rel_cost_fn_filter_scales_with_simd() {
    use egg::CostFunction;
    let narrow_simd = ra_hardware::HardwareProfile {
        simd_width_bits: 128,
        ..ra_hardware::HardwareProfile::cpu_only()
    };
    let wide_simd = ra_hardware::HardwareProfile {
        simd_width_bits: 512,
        ..ra_hardware::HardwareProfile::cpu_only()
    };

    let expr = RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let nodes = rec.as_ref();

    let cost_narrow = {
        let mut cf = RelCostFn::new(narrow_simd);
        cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
    };
    let cost_wide = {
        let mut cf = RelCostFn::new(wide_simd);
        cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
    };
    assert!(
        cost_narrow > cost_wide,
        "narrow SIMD should cost more: {cost_narrow} vs {cost_wide}"
    );
}

// -- extract_best and extract_best_with_staleness --

#[test]
fn extract_best_without_stats() {
    use crate::analysis::RelAnalysis;
    let expr = RelExpr::scan("users").filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("age"))),
        right: Box::new(Expr::Const(Const::Int(18))),
    });
    let hw = ra_hardware::HardwareProfile::cpu_only();
    let mut egraph = egg::EGraph::<RelLang, RelAnalysis>::default();
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let root = egraph.add_expr(&rec);

    let stats: HashMap<String, ra_core::statistics::Statistics> = HashMap::new();
    let result = extract_best(&egraph, root, &stats, &hw, crate::cost::LiveConditions::NEUTRAL, None).expect("extraction should succeed");
    assert!(matches!(result, RelExpr::Filter { .. }));
}

#[test]
fn extract_best_with_stats() {
    use crate::analysis::RelAnalysis;
    let expr = RelExpr::scan("users");
    let hw = ra_hardware::HardwareProfile::cpu_only();
    let mut egraph = egg::EGraph::<RelLang, RelAnalysis>::default();
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let root = egraph.add_expr(&rec);

    let mut stats = HashMap::new();
    stats.insert(
        "users".to_string(),
        ra_core::statistics::Statistics::new(10000.0),
    );
    let result = extract_best(&egraph, root, &stats, &hw, crate::cost::LiveConditions::NEUTRAL, None).expect("extraction should succeed");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

#[test]
fn extract_best_with_staleness_fn() {
    use crate::analysis::RelAnalysis;
    let expr = RelExpr::scan("users");
    let hw = ra_hardware::HardwareProfile::cpu_only();
    let mut egraph = egg::EGraph::<RelLang, RelAnalysis>::default();
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let root = egraph.add_expr(&rec);

    let mut stats = HashMap::new();
    stats.insert(
        "users".to_string(),
        ra_core::statistics::Statistics::new(5000.0),
    );
    let mut staleness = HashMap::new();
    staleness.insert("users".to_string(), Staleness::Fresh);
    let result = extract_best_with_staleness(&egraph, root, &stats, &staleness, &hw)
        .expect("extraction should succeed");
    assert!(matches!(result, RelExpr::Scan { .. }));
}

// -- Window function variant coverage --

#[test]
fn extract_window_all_ranking_functions() {
    use ra_core::algebra::{WindowExpr as WExpr, WindowFunction as WFn};
    let funcs = [
        WFn::Rank,
        WFn::DenseRank,
        WFn::PercentRank,
        WFn::Ntile,
        WFn::Lag,
        WFn::Lead,
        WFn::FirstValue,
        WFn::LastValue,
        WFn::NthValue,
        WFn::Min,
        WFn::Max,
    ];
    for wfn in &funcs {
        let expr = RelExpr::Window {
            functions: vec![WExpr {
                function: *wfn,
                arg: Some(Expr::Column(ColumnRef::new("col"))),
                partition_by: vec![],
                order_by: vec![],
                frame: None,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        let rec = to_rec_expr(&expr).unwrap_or_else(|e| panic!("to_rec_expr for {wfn:?}: {e}"));
        let result =
            rec_expr_to_rel_expr(&rec).unwrap_or_else(|e| panic!("extraction for {wfn:?}: {e}"));
        assert_eq!(result, expr, "round-trip failed for {wfn:?}");
    }
}

// -- Limit with zero offset --

#[test]
fn extract_limit_zero_offset() {
    let expr = RelExpr::scan("t").limit(100, 0);
    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}

// -- Complex nested plan --

#[test]
fn extract_complex_plan() {
    let base = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("o", "uid"))),
        },
        left: Box::new(RelExpr::Scan {
            table: "users".into(),
            alias: Some("u".into()),
        }),
        right: Box::new(RelExpr::Scan {
            table: "orders".into(),
            alias: Some("o".into()),
        }),
    };
    let agg = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::qualified("u", "name"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::qualified("o", "amount"))),
            distinct: false,
            alias: Some("total".into()),
        }],
        input: Box::new(base),
    };
    let expr = RelExpr::Sort {
        keys: vec![SortKey {
            expr: Expr::Column(ColumnRef::new("total")),
            direction: SortDirection::Desc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(agg),
    }
    .limit(10, 0);

    let rec = to_rec_expr(&expr).expect("to_rec_expr");
    let result = rec_expr_to_rel_expr(&rec).expect("extraction");
    assert_eq!(result, expr);
}
