//! Property-based tests for algebraic properties of optimization
//! rules.
//!
//! Verifies that rewrite rules preserve core algebraic invariants:
//! - Join commutativity: A JOIN B ~ B JOIN A
//! - Filter pushdown equivalence: filter + join ~ join + filter
//! - Projection pushdown: project(project(x)) ~ project(x)
//! - Expression simplification: semantic identity preservation
//! - Set operation commutativity: union/intersect symmetry
//! - Limit/sort interactions: redundant sort elimination
//! - Aggregate-sort independence: sort below aggregate is irrelevant

#![allow(clippy::expect_used)]

use egg::{EGraph, Runner};
use proptest::prelude::*;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};
use ra_engine::{
    all_rules, to_rec_expr, RelAnalysis, RelLang,
};

// ---------------------------------------------------------------
// Strategies (reuse the patterns from the main proptest file but
// tailored for algebraic property tests)
// ---------------------------------------------------------------

fn arb_table() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("t1".to_owned()),
        Just("t2".to_owned()),
        Just("t3".to_owned()),
    ]
}

fn arb_col() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("a".to_owned()),
        Just("b".to_owned()),
        Just("c".to_owned()),
        Just("d".to_owned()),
    ]
}

fn arb_column_expr() -> impl Strategy<Value = Expr> {
    arb_col().prop_map(|c| Expr::Column(ColumnRef::new(c)))
}

fn arb_int_const() -> impl Strategy<Value = Expr> {
    (-100i64..100).prop_map(|i| Expr::Const(Const::Int(i)))
}

fn arb_simple_pred() -> impl Strategy<Value = Expr> {
    (arb_column_expr(), arb_int_const()).prop_map(|(col, val)| {
        Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(col),
            right: Box::new(val),
        }
    })
}

fn arb_eq_pred() -> impl Strategy<Value = Expr> {
    (arb_column_expr(), arb_column_expr()).prop_map(|(l, r)| {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(l),
            right: Box::new(r),
        }
    })
}

fn arb_agg_func() -> impl Strategy<Value = AggregateFunction> {
    prop_oneof![
        Just(AggregateFunction::Count),
        Just(AggregateFunction::Sum),
        Just(AggregateFunction::Avg),
        Just(AggregateFunction::Min),
        Just(AggregateFunction::Max),
    ]
}

// ---------------------------------------------------------------
// Helper: check if two RelExpr land in the same e-class after
// equality saturation with all rewrite rules.
// ---------------------------------------------------------------

fn in_same_eclass(a: &RelExpr, b: &RelExpr) -> bool {
    let rec_a = match to_rec_expr(a) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let rec_b = match to_rec_expr(b) {
        Ok(r) => r,
        Err(_) => return false,
    };

    let mut egraph: EGraph<RelLang, RelAnalysis> = EGraph::default();
    let id_a = egraph.add_expr(&rec_a);
    let id_b = egraph.add_expr(&rec_b);

    let runner = Runner::default()
        .with_egraph(egraph)
        .with_node_limit(20_000)
        .with_iter_limit(10)
        .run(&all_rules());

    runner.egraph.find(id_a) == runner.egraph.find(id_b)
}

/// Verify that an expression can survive roundtrip through e-graph
/// conversion and that rules don't crash.
fn rules_dont_crash(expr: &RelExpr) -> bool {
    let rec = match to_rec_expr(expr) {
        Ok(r) => r,
        Err(_) => return true, // unsupported construct is fine
    };
    let _runner: Runner<RelLang, RelAnalysis> = Runner::default()
        .with_expr(&rec)
        .with_node_limit(10_000)
        .with_iter_limit(5)
        .run(&all_rules());
    true
}

// ---------------------------------------------------------------
// Property tests: Join commutativity
// ---------------------------------------------------------------

proptest! {
    /// Inner join is commutative: A INNER JOIN B ~ B INNER JOIN A
    #[test]
    fn inner_join_commutativity(
        t1 in arb_table(),
        t2 in arb_table(),
        pred in arb_eq_pred(),
    ) {
        let left = RelExpr::Scan { table: t1.clone(), alias: None };
        let right = RelExpr::Scan { table: t2.clone(), alias: None };

        let join_ab = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: pred.clone(),
            left: Box::new(left.clone()),
            right: Box::new(right.clone()),
        };
        let join_ba = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: pred,
            left: Box::new(right),
            right: Box::new(left),
        };

        prop_assert!(
            in_same_eclass(&join_ab, &join_ba),
            "inner join commutativity failed for {} JOIN {}",
            t1, t2
        );
    }

    /// Cross join is commutative: A CROSS B ~ B CROSS A
    #[test]
    fn cross_join_commutativity(
        t1 in arb_table(),
        t2 in arb_table(),
    ) {
        let left = RelExpr::Scan { table: t1.clone(), alias: None };
        let right = RelExpr::Scan { table: t2.clone(), alias: None };
        let cond = Expr::Const(Const::Bool(true));

        let cross_ab = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: cond.clone(),
            left: Box::new(left.clone()),
            right: Box::new(right.clone()),
        };
        let cross_ba = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: cond,
            left: Box::new(right),
            right: Box::new(left),
        };

        prop_assert!(
            in_same_eclass(&cross_ab, &cross_ba),
            "cross join commutativity failed for {} CROSS {}",
            t1, t2
        );
    }

    /// Inner join associativity: (A JOIN B) JOIN C ~ A JOIN (B JOIN C)
    #[test]
    fn inner_join_associativity(
        t1 in arb_table(),
        t2 in arb_table(),
        t3 in arb_table(),
        c1 in arb_eq_pred(),
        c2 in arb_eq_pred(),
    ) {
        let a = RelExpr::Scan { table: t1, alias: None };
        let b = RelExpr::Scan { table: t2, alias: None };
        let c = RelExpr::Scan { table: t3, alias: None };

        // (A JOIN B) JOIN C
        let left_deep = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: c1.clone(),
            left: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: c2.clone(),
                left: Box::new(a.clone()),
                right: Box::new(b.clone()),
            }),
            right: Box::new(c.clone()),
        };

        // A JOIN (B JOIN C)
        let right_deep = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: c2,
            left: Box::new(a),
            right: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: c1,
                left: Box::new(b),
                right: Box::new(c),
            }),
        };

        prop_assert!(
            in_same_eclass(&left_deep, &right_deep),
            "join associativity failed"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Filter pushdown equivalence
// ---------------------------------------------------------------

proptest! {
    /// Filter through inner join: filter(pred, A JOIN B) ~
    /// A JOIN B with pred pushed to either side.
    #[test]
    fn filter_pushdown_through_inner_join(
        t1 in arb_table(),
        t2 in arb_table(),
        join_pred in arb_eq_pred(),
        filter_pred in arb_simple_pred(),
    ) {
        let left = RelExpr::Scan { table: t1, alias: None };
        let right = RelExpr::Scan { table: t2, alias: None };

        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: join_pred.clone(),
            left: Box::new(left.clone()),
            right: Box::new(right.clone()),
        };

        // filter on top of join
        let filter_over_join = RelExpr::Filter {
            predicate: filter_pred.clone(),
            input: Box::new(join),
        };

        // filter pushed to left side of join
        let filter_pushed_left = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: join_pred,
            left: Box::new(RelExpr::Filter {
                predicate: filter_pred,
                input: Box::new(left),
            }),
            right: Box::new(right),
        };

        prop_assert!(
            in_same_eclass(&filter_over_join, &filter_pushed_left),
            "filter pushdown through inner join failed"
        );
    }

    /// Merging adjacent filters: filter(p1, filter(p2, x)) ~
    /// filter(p1 AND p2, x)
    #[test]
    fn filter_merge_equivalence(
        t in arb_table(),
        p1 in arb_simple_pred(),
        p2 in arb_simple_pred(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };

        let stacked = RelExpr::Filter {
            predicate: p1.clone(),
            input: Box::new(RelExpr::Filter {
                predicate: p2.clone(),
                input: Box::new(scan.clone()),
            }),
        };

        let merged = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(p1),
                right: Box::new(p2),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&stacked, &merged),
            "filter merge equivalence failed"
        );
    }

    /// Splitting conjunctive filter: filter(p1 AND p2, x) ~
    /// filter(p1, filter(p2, x))
    #[test]
    fn filter_split_equivalence(
        t in arb_table(),
        p1 in arb_simple_pred(),
        p2 in arb_simple_pred(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };

        let conj = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(p1.clone()),
                right: Box::new(p2.clone()),
            },
            input: Box::new(scan.clone()),
        };

        let split = RelExpr::Filter {
            predicate: p1,
            input: Box::new(RelExpr::Filter {
                predicate: p2,
                input: Box::new(scan),
            }),
        };

        prop_assert!(
            in_same_eclass(&conj, &split),
            "filter split equivalence failed"
        );
    }

    /// Filter through union: filter(p, A UNION ALL B) ~
    /// filter(p, A) UNION ALL filter(p, B)
    #[test]
    fn filter_through_union(
        t1 in arb_table(),
        t2 in arb_table(),
        pred in arb_simple_pred(),
    ) {
        let a = RelExpr::Scan { table: t1, alias: None };
        let b = RelExpr::Scan { table: t2, alias: None };

        let filter_over_union = RelExpr::Filter {
            predicate: pred.clone(),
            input: Box::new(RelExpr::Union {
                all: true,
                left: Box::new(a.clone()),
                right: Box::new(b.clone()),
            }),
        };

        let filter_pushed = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::Filter {
                predicate: pred.clone(),
                input: Box::new(a),
            }),
            right: Box::new(RelExpr::Filter {
                predicate: pred,
                input: Box::new(b),
            }),
        };

        prop_assert!(
            in_same_eclass(&filter_over_union, &filter_pushed),
            "filter through union failed"
        );
    }

    /// Filter TRUE is identity: filter(TRUE, x) ~ x
    #[test]
    fn filter_true_is_identity(t in arb_table()) {
        let scan = RelExpr::Scan { table: t, alias: None };

        let filtered = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(scan.clone()),
        };

        prop_assert!(
            in_same_eclass(&filtered, &scan),
            "filter(TRUE, x) should equal x"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Projection pushdown
// ---------------------------------------------------------------

proptest! {
    /// project(c1, project(c2, x)) ~ project(c1, x)
    /// (outer projection makes inner redundant)
    #[test]
    fn project_merge(
        t in arb_table(),
        col1 in arb_col(),
        col2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };

        let inner_cols = vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new(col2)),
            alias: None,
        }];
        let outer_cols = vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new(col1.clone())),
            alias: None,
        }];

        let double_project = RelExpr::Project {
            columns: outer_cols.clone(),
            input: Box::new(RelExpr::Project {
                columns: inner_cols,
                input: Box::new(scan.clone()),
            }),
        };

        let single_project = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new(col1)),
                alias: None,
            }],
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&double_project, &single_project),
            "project merge failed"
        );
    }

    /// filter through project: filter(p, project(c, x)) ~
    /// project(c, filter(p, x))
    #[test]
    fn filter_through_project(
        t in arb_table(),
        col in arb_col(),
        pred in arb_simple_pred(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let cols = vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new(col)),
            alias: None,
        }];

        let filter_over_project = RelExpr::Filter {
            predicate: pred.clone(),
            input: Box::new(RelExpr::Project {
                columns: cols.clone(),
                input: Box::new(scan.clone()),
            }),
        };

        let project_over_filter = RelExpr::Project {
            columns: cols,
            input: Box::new(RelExpr::Filter {
                predicate: pred,
                input: Box::new(scan),
            }),
        };

        prop_assert!(
            in_same_eclass(
                &filter_over_project,
                &project_over_filter,
            ),
            "filter through project failed"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Expression simplification
// ---------------------------------------------------------------

proptest! {
    /// Double negation: NOT(NOT(x)) ~ x
    #[test]
    fn double_negation_elimination(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let col_expr = Expr::Column(ColumnRef::new(col));

        let double_not = RelExpr::Filter {
            predicate: Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(col_expr.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let plain = RelExpr::Filter {
            predicate: col_expr,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&double_not, &plain),
            "double negation elimination failed"
        );
    }

    /// DeMorgan: NOT(a AND b) ~ NOT(a) OR NOT(b)
    #[test]
    fn demorgan_and_to_or(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let not_and = RelExpr::Filter {
            predicate: Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(Expr::BinOp {
                    op: BinOp::And,
                    left: Box::new(a.clone()),
                    right: Box::new(b.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let or_nots = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(a),
                }),
                right: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(b),
                }),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&not_and, &or_nots),
            "DeMorgan AND->OR failed"
        );
    }

    /// DeMorgan: NOT(a OR b) ~ NOT(a) AND NOT(b)
    #[test]
    fn demorgan_or_to_and(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let not_or = RelExpr::Filter {
            predicate: Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(Expr::BinOp {
                    op: BinOp::Or,
                    left: Box::new(a.clone()),
                    right: Box::new(b.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let and_nots = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(a),
                }),
                right: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    operand: Box::new(b),
                }),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&not_or, &and_nots),
            "DeMorgan OR->AND failed"
        );
    }

    /// AND identity: x AND TRUE ~ x
    #[test]
    fn and_true_identity(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let and_true = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(x.clone()),
                right: Box::new(Expr::Const(Const::Bool(true))),
            },
            input: Box::new(scan.clone()),
        };

        let just_x = RelExpr::Filter {
            predicate: x,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&and_true, &just_x),
            "AND TRUE identity failed"
        );
    }

    /// OR FALSE identity: x OR FALSE ~ x
    #[test]
    fn or_false_identity(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let or_false = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(x.clone()),
                right: Box::new(Expr::Const(Const::Bool(false))),
            },
            input: Box::new(scan.clone()),
        };

        let just_x = RelExpr::Filter {
            predicate: x,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&or_false, &just_x),
            "OR FALSE identity failed"
        );
    }

    /// AND idempotence: x AND x ~ x
    #[test]
    fn and_idempotent(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let and_self = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(x.clone()),
                right: Box::new(x.clone()),
            },
            input: Box::new(scan.clone()),
        };

        let just_x = RelExpr::Filter {
            predicate: x,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&and_self, &just_x),
            "AND idempotence failed"
        );
    }

    /// OR idempotence: x OR x ~ x
    #[test]
    fn or_idempotent(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let or_self = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(x.clone()),
                right: Box::new(x.clone()),
            },
            input: Box::new(scan.clone()),
        };

        let just_x = RelExpr::Filter {
            predicate: x,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&or_self, &just_x),
            "OR idempotence failed"
        );
    }

    /// Arithmetic: x + 0 ~ x
    #[test]
    fn add_zero_identity(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let add_zero = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(x.clone()),
                right: Box::new(Expr::Const(Const::Int(0))),
            },
            input: Box::new(scan.clone()),
        };

        let just_x = RelExpr::Filter {
            predicate: x,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&add_zero, &just_x),
            "x + 0 identity failed"
        );
    }

    /// Arithmetic: x * 1 ~ x
    #[test]
    fn mul_one_identity(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let mul_one = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Mul,
                left: Box::new(x.clone()),
                right: Box::new(Expr::Const(Const::Int(1))),
            },
            input: Box::new(scan.clone()),
        };

        let just_x = RelExpr::Filter {
            predicate: x,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&mul_one, &just_x),
            "x * 1 identity failed"
        );
    }

    /// Arithmetic: x - x ~ 0 (DuckDB rule)
    #[test]
    fn sub_self_is_zero(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let sub_self = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Sub,
                left: Box::new(x.clone()),
                right: Box::new(x),
            },
            input: Box::new(scan.clone()),
        };

        let zero = RelExpr::Filter {
            predicate: Expr::Const(Const::Int(0)),
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&sub_self, &zero),
            "x - x should equal 0"
        );
    }

    /// Double arithmetic negation: -(-x) ~ x
    #[test]
    fn double_arith_negation(
        t in arb_table(),
        col in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let x = Expr::Column(ColumnRef::new(col));

        let double_neg = RelExpr::Filter {
            predicate: Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    operand: Box::new(x.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let plain = RelExpr::Filter {
            predicate: x,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&double_neg, &plain),
            "double arithmetic negation failed"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Comparison inversion (DuckDB rules)
// ---------------------------------------------------------------

proptest! {
    /// NOT(a < b) ~ a >= b
    #[test]
    fn not_lt_is_ge(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let not_lt = RelExpr::Filter {
            predicate: Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(Expr::BinOp {
                    op: BinOp::Lt,
                    left: Box::new(a.clone()),
                    right: Box::new(b.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let ge = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Ge,
                left: Box::new(a),
                right: Box::new(b),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&not_lt, &ge),
            "NOT(a < b) should equal a >= b"
        );
    }

    /// NOT(a = b) ~ a != b
    #[test]
    fn not_eq_is_ne(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let not_eq = RelExpr::Filter {
            predicate: Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(a.clone()),
                    right: Box::new(b.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let ne = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Ne,
                left: Box::new(a),
                right: Box::new(b),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&not_eq, &ne),
            "NOT(a = b) should equal a != b"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Set operation commutativity
// ---------------------------------------------------------------

proptest! {
    /// Union commutativity: A UNION B ~ B UNION A
    #[test]
    fn union_commutativity(
        t1 in arb_table(),
        t2 in arb_table(),
        all in any::<bool>(),
    ) {
        let a = RelExpr::Scan { table: t1, alias: None };
        let b = RelExpr::Scan { table: t2, alias: None };

        let union_ab = RelExpr::Union {
            all,
            left: Box::new(a.clone()),
            right: Box::new(b.clone()),
        };
        let union_ba = RelExpr::Union {
            all,
            left: Box::new(b),
            right: Box::new(a),
        };

        prop_assert!(
            in_same_eclass(&union_ab, &union_ba),
            "union commutativity failed"
        );
    }

    /// Intersect commutativity: A INTERSECT B ~ B INTERSECT A
    #[test]
    fn intersect_commutativity(
        t1 in arb_table(),
        t2 in arb_table(),
        all in any::<bool>(),
    ) {
        let a = RelExpr::Scan { table: t1, alias: None };
        let b = RelExpr::Scan { table: t2, alias: None };

        let int_ab = RelExpr::Intersect {
            all,
            left: Box::new(a.clone()),
            right: Box::new(b.clone()),
        };
        let int_ba = RelExpr::Intersect {
            all,
            left: Box::new(b),
            right: Box::new(a),
        };

        prop_assert!(
            in_same_eclass(&int_ab, &int_ba),
            "intersect commutativity failed"
        );
    }

    /// Intersect self is identity: A INTERSECT A ~ A
    #[test]
    fn intersect_self_identity(
        t in arb_table(),
        all in any::<bool>(),
    ) {
        let a = RelExpr::Scan { table: t, alias: None };

        let int_self = RelExpr::Intersect {
            all,
            left: Box::new(a.clone()),
            right: Box::new(a.clone()),
        };

        prop_assert!(
            in_same_eclass(&int_self, &a),
            "intersect self should be identity"
        );
    }

    /// Except self is empty: A EXCEPT A ~ filter(FALSE, A)
    #[test]
    fn except_self_is_empty(
        t in arb_table(),
        all in any::<bool>(),
    ) {
        let a = RelExpr::Scan { table: t, alias: None };

        let except_self = RelExpr::Except {
            all,
            left: Box::new(a.clone()),
            right: Box::new(a.clone()),
        };

        let empty = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(false)),
            input: Box::new(a),
        };

        prop_assert!(
            in_same_eclass(&except_self, &empty),
            "except self should produce empty result"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Limit/Sort interactions
// ---------------------------------------------------------------

proptest! {
    /// Limit through project: limit(n, project(c, x)) ~
    /// project(c, limit(n, x))
    #[test]
    fn limit_through_project(
        t in arb_table(),
        col in arb_col(),
        n in 1u64..50,
        off in 0u64..10,
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let cols = vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new(col)),
            alias: None,
        }];

        let limit_over_project = RelExpr::Limit {
            count: n,
            offset: off,
            input: Box::new(RelExpr::Project {
                columns: cols.clone(),
                input: Box::new(scan.clone()),
            }),
        };

        let project_over_limit = RelExpr::Project {
            columns: cols,
            input: Box::new(RelExpr::Limit {
                count: n,
                offset: off,
                input: Box::new(scan),
            }),
        };

        prop_assert!(
            in_same_eclass(
                &limit_over_project,
                &project_over_limit,
            ),
            "limit through project failed"
        );
    }

    /// Redundant sort elimination: sort(k1, sort(k2, x)) ~
    /// sort(k1, x)
    #[test]
    fn redundant_sort_eliminated(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };

        let k1 = vec![SortKey {
            expr: Expr::Column(ColumnRef::new(c1)),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }];
        let k2 = vec![SortKey {
            expr: Expr::Column(ColumnRef::new(c2)),
            direction: SortDirection::Desc,
            nulls: NullOrdering::First,
        }];

        let double_sort = RelExpr::Sort {
            keys: k1.clone(),
            input: Box::new(RelExpr::Sort {
                keys: k2,
                input: Box::new(scan.clone()),
            }),
        };

        let single_sort = RelExpr::Sort {
            keys: k1,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&double_sort, &single_sort),
            "redundant sort elimination failed"
        );
    }

    /// Sort below aggregate is irrelevant (DuckDB rule):
    /// aggregate(g, a, sort(k, x)) ~ aggregate(g, a, x)
    #[test]
    fn sort_below_aggregate_irrelevant(
        t in arb_table(),
        group_col in arb_col(),
        agg_col in arb_col(),
        sort_col in arb_col(),
        func in arb_agg_func(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let groups = vec![Expr::Column(ColumnRef::new(group_col))];
        let aggs = vec![AggregateExpr {
            function: func,
            arg: Some(Expr::Column(ColumnRef::new(agg_col))),
            distinct: false,
            alias: None,
        }];

        let sort_keys = vec![SortKey {
            expr: Expr::Column(ColumnRef::new(sort_col)),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }];

        let agg_over_sort = RelExpr::Aggregate {
            group_by: groups.clone(),
            aggregates: aggs.clone(),
            input: Box::new(RelExpr::Sort {
                keys: sort_keys,
                input: Box::new(scan.clone()),
            }),
        };

        let agg_direct = RelExpr::Aggregate {
            group_by: groups,
            aggregates: aggs,
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&agg_over_sort, &agg_direct),
            "sort below aggregate should be eliminated"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Commutativity of scalar operators
// ---------------------------------------------------------------

proptest! {
    /// Addition commutativity: a + b ~ b + a
    #[test]
    fn addition_commutativity(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let a_plus_b = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(a.clone()),
                right: Box::new(b.clone()),
            },
            input: Box::new(scan.clone()),
        };

        let b_plus_a = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(b),
                right: Box::new(a),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&a_plus_b, &b_plus_a),
            "addition commutativity failed"
        );
    }

    /// Multiplication commutativity: a * b ~ b * a
    #[test]
    fn multiplication_commutativity(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let a_times_b = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Mul,
                left: Box::new(a.clone()),
                right: Box::new(b.clone()),
            },
            input: Box::new(scan.clone()),
        };

        let b_times_a = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Mul,
                left: Box::new(b),
                right: Box::new(a),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&a_times_b, &b_times_a),
            "multiplication commutativity failed"
        );
    }

    /// Equality commutativity: a = b ~ b = a
    #[test]
    fn equality_commutativity(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let a_eq_b = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(a.clone()),
                right: Box::new(b.clone()),
            },
            input: Box::new(scan.clone()),
        };

        let b_eq_a = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(b),
                right: Box::new(a),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&a_eq_b, &b_eq_a),
            "equality commutativity failed"
        );
    }

    /// Comparison flip: a < b ~ b > a
    #[test]
    fn lt_to_gt_flip(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let a_lt_b = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(a.clone()),
                right: Box::new(b.clone()),
            },
            input: Box::new(scan.clone()),
        };

        let b_gt_a = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(b),
                right: Box::new(a),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&a_lt_b, &b_gt_a),
            "lt-to-gt flip failed"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Aggregate optimization
// ---------------------------------------------------------------

proptest! {
    /// Filter below aggregate: filter(p, agg(g, a, x)) ~
    /// agg(g, a, filter(p, x))
    #[test]
    fn filter_below_aggregate(
        t in arb_table(),
        group_col in arb_col(),
        agg_col in arb_col(),
        pred in arb_simple_pred(),
        func in arb_agg_func(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let groups = vec![Expr::Column(ColumnRef::new(group_col))];
        let aggs = vec![AggregateExpr {
            function: func,
            arg: Some(Expr::Column(ColumnRef::new(agg_col))),
            distinct: false,
            alias: None,
        }];

        let filter_over_agg = RelExpr::Filter {
            predicate: pred.clone(),
            input: Box::new(RelExpr::Aggregate {
                group_by: groups.clone(),
                aggregates: aggs.clone(),
                input: Box::new(scan.clone()),
            }),
        };

        let agg_over_filter = RelExpr::Aggregate {
            group_by: groups,
            aggregates: aggs,
            input: Box::new(RelExpr::Filter {
                predicate: pred,
                input: Box::new(scan),
            }),
        };

        prop_assert!(
            in_same_eclass(&filter_over_agg, &agg_over_filter),
            "filter below aggregate failed"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: SQLite-inspired rules
// ---------------------------------------------------------------

proptest! {
    /// Range collapse: a >= b AND a <= b ~ a = b
    #[test]
    fn range_collapse_to_eq(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));

        let range = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::BinOp {
                    op: BinOp::Ge,
                    left: Box::new(a.clone()),
                    right: Box::new(b.clone()),
                }),
                right: Box::new(Expr::BinOp {
                    op: BinOp::Le,
                    left: Box::new(a.clone()),
                    right: Box::new(b.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let eq = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(a),
                right: Box::new(b),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&range, &eq),
            "range collapse to equality failed"
        );
    }

    /// OR distribution: (a AND b) OR (a AND c) ~ a AND (b OR c)
    #[test]
    fn or_distribution(
        t in arb_table(),
        c1 in arb_col(),
        c2 in arb_col(),
        c3 in arb_col(),
    ) {
        let scan = RelExpr::Scan { table: t, alias: None };
        let a = Expr::Column(ColumnRef::new(c1));
        let b = Expr::Column(ColumnRef::new(c2));
        let c = Expr::Column(ColumnRef::new(c3));

        let distributed = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Or,
                left: Box::new(Expr::BinOp {
                    op: BinOp::And,
                    left: Box::new(a.clone()),
                    right: Box::new(b.clone()),
                }),
                right: Box::new(Expr::BinOp {
                    op: BinOp::And,
                    left: Box::new(a.clone()),
                    right: Box::new(c.clone()),
                }),
            },
            input: Box::new(scan.clone()),
        };

        let factored = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(a),
                right: Box::new(Expr::BinOp {
                    op: BinOp::Or,
                    left: Box::new(b),
                    right: Box::new(c),
                }),
            },
            input: Box::new(scan),
        };

        prop_assert!(
            in_same_eclass(&distributed, &factored),
            "OR distribution failed"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Cartesian product / cross join
// ---------------------------------------------------------------

proptest! {
    /// Cartesian product with filter becomes inner join:
    /// filter(pred, A CROSS B) ~ A INNER JOIN B ON pred
    #[test]
    fn cartesian_to_inner_join(
        t1 in arb_table(),
        t2 in arb_table(),
        pred in arb_eq_pred(),
    ) {
        let a = RelExpr::Scan { table: t1, alias: None };
        let b = RelExpr::Scan { table: t2, alias: None };

        let cartesian_with_filter = RelExpr::Filter {
            predicate: pred.clone(),
            input: Box::new(RelExpr::Join {
                join_type: JoinType::Cross,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(a.clone()),
                right: Box::new(b.clone()),
            }),
        };

        let inner_join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: pred,
            left: Box::new(a),
            right: Box::new(b),
        };

        prop_assert!(
            in_same_eclass(
                &cartesian_with_filter,
                &inner_join,
            ),
            "cartesian product + filter should become inner join"
        );
    }
}

// ---------------------------------------------------------------
// Property tests: Optimization robustness
// ---------------------------------------------------------------

proptest! {
    /// Optimization never crashes on joins with arbitrary predicates.
    #[test]
    fn join_optimization_robust(
        t1 in arb_table(),
        t2 in arb_table(),
        pred in arb_eq_pred(),
    ) {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: pred,
            left: Box::new(RelExpr::Scan {
                table: t1,
                alias: None,
            }),
            right: Box::new(RelExpr::Scan {
                table: t2,
                alias: None,
            }),
        };
        prop_assert!(rules_dont_crash(&expr));
    }

    /// Optimization never crashes on filter chains.
    #[test]
    fn filter_chain_robust(
        t in arb_table(),
        p1 in arb_simple_pred(),
        p2 in arb_simple_pred(),
        p3 in arb_simple_pred(),
    ) {
        let expr = RelExpr::Filter {
            predicate: p1,
            input: Box::new(RelExpr::Filter {
                predicate: p2,
                input: Box::new(RelExpr::Filter {
                    predicate: p3,
                    input: Box::new(RelExpr::Scan {
                        table: t,
                        alias: None,
                    }),
                }),
            }),
        };
        prop_assert!(rules_dont_crash(&expr));
    }

    /// Optimization never crashes on aggregate expressions.
    #[test]
    fn aggregate_optimization_robust(
        t in arb_table(),
        group_col in arb_col(),
        agg_col in arb_col(),
        func in arb_agg_func(),
    ) {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new(group_col))],
            aggregates: vec![AggregateExpr {
                function: func,
                arg: Some(Expr::Column(ColumnRef::new(agg_col))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::Scan {
                table: t,
                alias: None,
            }),
        };
        prop_assert!(rules_dont_crash(&expr));
    }
}
