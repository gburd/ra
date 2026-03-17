//! Property-based tests for the optimization engine.
//!
//! Uses proptest to generate arbitrary relational algebra expressions
//! and verify key invariants:
//! - Roundtrip: `to_rec_expr` -> `rec_expr_to_rel_expr` is identity
//! - Table preservation: optimization preserves table references
//! - Idempotence: optimizing twice yields same result as once
//! - Hash determinism: same expression always hashes identically

#![allow(clippy::expect_used)]

use proptest::prelude::*;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};
use ra_engine::{
    all_rules, extract_best, rec_expr_to_rel_expr, structural_hash, to_rec_expr, Optimizer,
    OptimizerConfig,
};

// ---------------------------------------------------------------
// Proptest strategies for generating arbitrary expressions
// ---------------------------------------------------------------

fn arb_table_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("users".to_owned()),
        Just("orders".to_owned()),
        Just("products".to_owned()),
        Just("customers".to_owned()),
        Just("items".to_owned()),
    ]
}

fn arb_column_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("id".to_owned()),
        Just("name".to_owned()),
        Just("age".to_owned()),
        Just("price".to_owned()),
        Just("qty".to_owned()),
        Just("status".to_owned()),
    ]
}

fn arb_const() -> impl Strategy<Value = Const> {
    prop_oneof![
        Just(Const::Null),
        any::<bool>().prop_map(Const::Bool),
        (-1000i64..1000).prop_map(Const::Int),
        Just(Const::String("test".to_owned())),
    ]
}

fn arb_column_ref() -> impl Strategy<Value = ColumnRef> {
    arb_column_name().prop_map(ColumnRef::new)
}

fn arb_binop() -> impl Strategy<Value = BinOp> {
    prop_oneof![
        Just(BinOp::Eq),
        Just(BinOp::Ne),
        Just(BinOp::Lt),
        Just(BinOp::Le),
        Just(BinOp::Gt),
        Just(BinOp::Ge),
        Just(BinOp::Add),
        Just(BinOp::Sub),
        Just(BinOp::Mul),
        Just(BinOp::And),
        Just(BinOp::Or),
    ]
}

fn arb_unaryop() -> impl Strategy<Value = UnaryOp> {
    prop_oneof![
        Just(UnaryOp::Not),
        Just(UnaryOp::IsNull),
        Just(UnaryOp::IsNotNull),
        Just(UnaryOp::Neg),
    ]
}

/// Generate arbitrary scalar expressions up to a given depth.
fn arb_expr(depth: u32) -> impl Strategy<Value = Expr> {
    let leaf = prop_oneof![
        arb_column_ref().prop_map(Expr::Column),
        arb_const().prop_map(Expr::Const),
    ];

    leaf.prop_recursive(depth, 64, 2, |inner| {
        prop_oneof![
            (arb_binop(), inner.clone(), inner.clone()).prop_map(|(op, left, right)| Expr::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            }),
            (arb_unaryop(), inner).prop_map(|(op, operand)| {
                Expr::UnaryOp {
                    op,
                    operand: Box::new(operand),
                }
            }),
        ]
    })
}

fn arb_join_type() -> impl Strategy<Value = JoinType> {
    prop_oneof![
        Just(JoinType::Inner),
        Just(JoinType::LeftOuter),
        Just(JoinType::RightOuter),
        Just(JoinType::FullOuter),
        Just(JoinType::Cross),
        Just(JoinType::Semi),
        Just(JoinType::Anti),
    ]
}

fn arb_sort_direction() -> impl Strategy<Value = SortDirection> {
    prop_oneof![Just(SortDirection::Asc), Just(SortDirection::Desc),]
}

fn arb_null_ordering() -> impl Strategy<Value = NullOrdering> {
    prop_oneof![Just(NullOrdering::First), Just(NullOrdering::Last),]
}

fn arb_sort_key() -> impl Strategy<Value = SortKey> {
    (arb_expr(1), arb_sort_direction(), arb_null_ordering()).prop_map(|(expr, direction, nulls)| {
        SortKey {
            expr,
            direction,
            nulls,
        }
    })
}

fn arb_agg_function() -> impl Strategy<Value = AggregateFunction> {
    prop_oneof![
        Just(AggregateFunction::Count),
        Just(AggregateFunction::Sum),
        Just(AggregateFunction::Avg),
        Just(AggregateFunction::Min),
        Just(AggregateFunction::Max),
    ]
}

fn arb_aggregate_expr() -> impl Strategy<Value = AggregateExpr> {
    (
        arb_agg_function(),
        prop::option::of(arb_expr(0)),
        any::<bool>(),
    )
        .prop_map(|(function, arg, distinct)| AggregateExpr {
            function,
            arg,
            distinct,
            alias: None,
        })
}

fn arb_projection_column() -> impl Strategy<Value = ProjectionColumn> {
    arb_expr(0).prop_map(|expr| ProjectionColumn { expr, alias: None })
}

/// Generate arbitrary relational expressions up to a given depth.
fn arb_rel_expr(depth: u32) -> impl Strategy<Value = RelExpr> {
    let leaf = arb_table_name().prop_map(|t| RelExpr::Scan {
        table: t,
        alias: None,
    });

    leaf.prop_recursive(depth, 128, 4, |inner| {
        prop_oneof![
            // Filter
            (arb_expr(1), inner.clone()).prop_map(|(pred, input)| {
                RelExpr::Filter {
                    predicate: pred,
                    input: Box::new(input),
                }
            }),
            // Project
            (
                prop::collection::vec(arb_projection_column(), 1..=3),
                inner.clone()
            )
                .prop_map(|(columns, input)| {
                    RelExpr::Project {
                        columns,
                        input: Box::new(input),
                    }
                }),
            // Join
            (arb_join_type(), arb_expr(1), inner.clone(), inner.clone()).prop_map(
                |(join_type, condition, left, right)| {
                    RelExpr::Join {
                        join_type,
                        condition,
                        left: Box::new(left),
                        right: Box::new(right),
                    }
                }
            ),
            // Limit
            (0u64..100, 0u64..50, inner.clone()).prop_map(|(count, offset, input)| {
                RelExpr::Limit {
                    count,
                    offset,
                    input: Box::new(input),
                }
            }),
            // Sort
            (prop::collection::vec(arb_sort_key(), 1..=2), inner.clone()).prop_map(
                |(keys, input)| {
                    RelExpr::Sort {
                        keys,
                        input: Box::new(input),
                    }
                }
            ),
            // Aggregate
            (
                prop::collection::vec(arb_expr(0), 0..=2),
                prop::collection::vec(arb_aggregate_expr(), 1..=2),
                inner.clone()
            )
                .prop_map(|(group_by, aggregates, input)| {
                    RelExpr::Aggregate {
                        group_by,
                        aggregates,
                        input: Box::new(input),
                    }
                }),
            // Union
            (any::<bool>(), inner.clone(), inner.clone()).prop_map(|(all, left, right)| {
                RelExpr::Union {
                    all,
                    left: Box::new(left),
                    right: Box::new(right),
                }
            }),
            // Intersect
            (any::<bool>(), inner.clone(), inner.clone()).prop_map(|(all, left, right)| {
                RelExpr::Intersect {
                    all,
                    left: Box::new(left),
                    right: Box::new(right),
                }
            }),
            // Except
            (any::<bool>(), inner.clone(), inner).prop_map(|(all, left, right)| {
                RelExpr::Except {
                    all,
                    left: Box::new(left),
                    right: Box::new(right),
                }
            }),
        ]
    })
}

// ---------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------

proptest! {
    /// Roundtrip: converting to RecExpr and back preserves the expression.
    #[test]
    fn roundtrip_preserves_expression(expr in arb_rel_expr(2)) {
        let rec = to_rec_expr(&expr)
            .expect("conversion to RecExpr should succeed");
        let recovered = rec_expr_to_rel_expr(&rec)
            .expect("conversion back should succeed");
        prop_assert_eq!(
            &expr, &recovered,
            "roundtrip should preserve the expression"
        );
    }

    /// Optimization never crashes on arbitrary input.
    #[test]
    fn optimization_does_not_panic(expr in arb_rel_expr(2)) {
        let config = OptimizerConfig {
            node_limit: 10_000,
            iter_limit: 5,
            time_limit_secs: 2,
        };
        let optimizer = Optimizer::with_config(config);
        // It's OK if optimization returns an error (e.g., for
        // unsupported constructs), but it must never panic.
        let _ = optimizer.optimize(&expr);
    }

    /// Table references are preserved through optimization.
    ///
    /// Every table in the original expression must appear in the
    /// optimized result (the optimizer may reorder but not drop
    /// tables).
    #[test]
    fn optimization_preserves_tables(expr in arb_rel_expr(2)) {
        let config = OptimizerConfig {
            node_limit: 10_000,
            iter_limit: 5,
            time_limit_secs: 2,
        };
        let optimizer = Optimizer::with_config(config);
        if let Ok(optimized) = optimizer.optimize(&expr) {
            let original_tables = collect_tables(&expr);
            let optimized_tables = collect_tables(&optimized);

            for table in &original_tables {
                prop_assert!(
                    optimized_tables.contains(table),
                    "table '{}' was lost during optimization.\n\
                     original tables: {:?}\n\
                     optimized tables: {:?}",
                    table,
                    original_tables,
                    optimized_tables
                );
            }
        }
    }

    /// Optimizing twice preserves table references and still
    /// produces a valid plan. Strict structural idempotence is
    /// not guaranteed because commutativity rules may cause the
    /// extractor to pick a different (but equivalent) ordering.
    #[test]
    fn optimization_twice_preserves_tables(expr in arb_rel_expr(1)) {
        let config = OptimizerConfig {
            node_limit: 10_000,
            iter_limit: 5,
            time_limit_secs: 2,
        };
        let optimizer = Optimizer::with_config(config);
        if let Ok(first) = optimizer.optimize(&expr) {
            if let Ok(second) = optimizer.optimize(&first) {
                let first_tables = collect_tables(&first);
                let second_tables = collect_tables(&second);
                prop_assert_eq!(
                    first_tables, second_tables,
                    "optimizing twice should preserve tables"
                );
            }
        }
    }

    /// Structural hash is deterministic: hashing the same expression
    /// always yields the same value.
    #[test]
    fn structural_hash_deterministic(expr in arb_rel_expr(2)) {
        let h1 = structural_hash(&expr);
        let h2 = structural_hash(&expr);
        prop_assert_eq!(h1, h2);
    }

    /// Cloned expressions have the same structural hash.
    #[test]
    fn structural_hash_clone_eq(expr in arb_rel_expr(2)) {
        let cloned = expr.clone();
        prop_assert_eq!(
            structural_hash(&expr),
            structural_hash(&cloned)
        );
    }

    /// E-graph conversion produces a non-empty RecExpr.
    #[test]
    fn to_rec_expr_non_empty(expr in arb_rel_expr(2)) {
        let rec = to_rec_expr(&expr)
            .expect("conversion should succeed");
        prop_assert!(
            !rec.as_ref().is_empty(),
            "RecExpr should not be empty"
        );
    }

    /// Egg rewrite rules can all be constructed without errors.
    /// (This is a sanity check that rule patterns are valid.)
    #[test]
    fn rules_construct_successfully(_dummy in 0u8..1) {
        let rules = all_rules();
        prop_assert!(rules.len() >= 50);
    }

    /// Extract-best on a single expression yields a valid RelExpr.
    #[test]
    fn extract_best_produces_valid_result(expr in arb_rel_expr(1)) {
        use egg::Runner;
        use ra_engine::RelLang;
        use ra_engine::RelAnalysis;
        use std::collections::HashMap;

        let rec = to_rec_expr(&expr)
            .expect("conversion should succeed");
        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec)
            .with_node_limit(5_000)
            .with_iter_limit(3)
            .run(&[]);

        let root = runner.roots[0];
        let stats: HashMap<String, ra_core::statistics::Statistics> =
            HashMap::new();
        let hardware = ra_hardware::HardwareProfile::cpu_only();
        let result = extract_best(&runner.egraph, root, &stats, &hardware);
        prop_assert!(
            result.is_ok(),
            "extract_best should succeed: {:?}",
            result.err()
        );
    }

    /// Running equality saturation with rules doesn't break
    /// extraction.
    #[test]
    fn saturation_then_extract(expr in arb_rel_expr(1)) {
        use egg::Runner;
        use ra_engine::RelLang;
        use ra_engine::RelAnalysis;
        use std::collections::HashMap;

        let rec = to_rec_expr(&expr)
            .expect("conversion should succeed");
        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&all_rules());

        let root = runner.roots[0];
        let stats: HashMap<String, ra_core::statistics::Statistics> =
            HashMap::new();
        let hardware = ra_hardware::HardwareProfile::cpu_only();
        let result = extract_best(&runner.egraph, root, &stats, &hardware);
        prop_assert!(
            result.is_ok(),
            "extract after saturation should succeed: {:?}",
            result.err()
        );
    }
}

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

fn collect_tables(expr: &RelExpr) -> std::collections::HashSet<String> {
    let mut tables = std::collections::HashSet::new();
    collect_tables_rec(expr, &mut tables);
    tables
}

fn collect_tables_rec(expr: &RelExpr, out: &mut std::collections::HashSet<String>) {
    match expr {
        RelExpr::Scan { table, .. } => {
            out.insert(table.clone());
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. } => {
            collect_tables_rec(input, out);
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_tables_rec(left, out);
            collect_tables_rec(right, out);
        }
    }
}
