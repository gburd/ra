//! Grammar-guided SQL expression generator.
//!
//! Generates syntactically valid [`RelExpr`] trees following the Lime
//! grammar structure. Uses proptest strategies to systematically
//! explore the SQL expression space.

use proptest::prelude::*;
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};

/// Configuration for SQL generation depth and complexity.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Maximum expression tree depth.
    pub max_depth: u32,
    /// Maximum number of tables in a single query.
    pub max_tables: usize,
    /// Maximum number of columns in projections.
    pub max_projection_cols: usize,
    /// Maximum number of sort keys.
    pub max_sort_keys: usize,
    /// Maximum number of aggregate expressions.
    pub max_aggregates: usize,
    /// Maximum number of group-by keys.
    pub max_group_by: usize,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_tables: 5,
            max_projection_cols: 4,
            max_sort_keys: 3,
            max_aggregates: 3,
            max_group_by: 2,
        }
    }
}

/// Grammar-guided SQL expression generator.
///
/// Produces arbitrary [`RelExpr`] trees that cover the full SQL
/// grammar surface area supported by the Lime parser.
#[derive(Debug, Clone)]
pub struct SqlGenerator {
    config: GeneratorConfig,
}

impl SqlGenerator {
    /// Create a generator with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: GeneratorConfig::default(),
        }
    }

    /// Create a generator with custom configuration.
    #[must_use]
    pub fn with_config(config: GeneratorConfig) -> Self {
        Self { config }
    }

    /// Return a proptest strategy that generates arbitrary SQL
    /// relational expressions.
    pub fn strategy(&self) -> impl Strategy<Value = RelExpr> {
        arb_rel_expr(self.config.max_depth)
    }

    /// Return a strategy for simple SELECT queries only.
    pub fn select_strategy(&self) -> impl Strategy<Value = RelExpr> {
        (
            prop::collection::vec(arb_projection_column(), 1..=3),
            arb_scan(),
        )
            .prop_map(|(columns, input)| RelExpr::Project {
                columns,
                input: Box::new(input),
            })
    }

    /// Return a strategy for join-heavy queries.
    pub fn join_strategy(&self) -> impl Strategy<Value = RelExpr> {
        arb_join_query(self.config.max_depth.min(2))
    }

    /// Return a strategy for aggregate queries.
    pub fn aggregate_strategy(&self) -> impl Strategy<Value = RelExpr> {
        arb_aggregate_query()
    }

    /// Return a strategy for set operation queries.
    pub fn set_op_strategy(&self) -> impl Strategy<Value = RelExpr> {
        arb_set_operation()
    }
}

impl Default for SqlGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------------
// Table and column name strategies
// -------------------------------------------------------------------

fn arb_table_name() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("users".to_owned()),
        Just("orders".to_owned()),
        Just("products".to_owned()),
        Just("customers".to_owned()),
        Just("items".to_owned()),
        Just("categories".to_owned()),
        Just("inventory".to_owned()),
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
        Just("created_at".to_owned()),
        Just("category_id".to_owned()),
    ]
}

// -------------------------------------------------------------------
// Leaf expression strategies
// -------------------------------------------------------------------

fn arb_const() -> impl Strategy<Value = Const> {
    prop_oneof![
        Just(Const::Null),
        any::<bool>().prop_map(Const::Bool),
        (-1000i64..1000).prop_map(Const::Int),
        Just(Const::String("test".to_owned())),
        Just(Const::String("active".to_owned())),
    ]
}

fn arb_column_ref() -> impl Strategy<Value = ColumnRef> {
    arb_column_name().prop_map(ColumnRef::new)
}

fn arb_column_expr() -> impl Strategy<Value = Expr> {
    arb_column_ref().prop_map(Expr::Column)
}

fn arb_const_expr() -> impl Strategy<Value = Expr> {
    arb_const().prop_map(Expr::Const)
}

// -------------------------------------------------------------------
// Operator strategies
// -------------------------------------------------------------------

fn arb_comparison_op() -> impl Strategy<Value = BinOp> {
    prop_oneof![
        Just(BinOp::Eq),
        Just(BinOp::Ne),
        Just(BinOp::Lt),
        Just(BinOp::Le),
        Just(BinOp::Gt),
        Just(BinOp::Ge),
    ]
}

fn arb_arithmetic_op() -> impl Strategy<Value = BinOp> {
    prop_oneof![
        Just(BinOp::Add),
        Just(BinOp::Sub),
        Just(BinOp::Mul),
    ]
}

fn arb_logical_op() -> impl Strategy<Value = BinOp> {
    prop_oneof![Just(BinOp::And), Just(BinOp::Or),]
}

fn arb_binop() -> impl Strategy<Value = BinOp> {
    prop_oneof![arb_comparison_op(), arb_arithmetic_op(), arb_logical_op(),]
}

fn arb_unaryop() -> impl Strategy<Value = UnaryOp> {
    prop_oneof![
        Just(UnaryOp::Not),
        Just(UnaryOp::IsNull),
        Just(UnaryOp::IsNotNull),
        Just(UnaryOp::Neg),
    ]
}

// -------------------------------------------------------------------
// Scalar expression strategy (recursive)
// -------------------------------------------------------------------

/// Generate arbitrary scalar expressions up to `depth`.
pub fn arb_expr(depth: u32) -> impl Strategy<Value = Expr> {
    let leaf = prop_oneof![arb_column_expr(), arb_const_expr(),];

    leaf.prop_recursive(depth, 64, 2, |inner| {
        prop_oneof![
            // Binary operation
            (arb_binop(), inner.clone(), inner.clone()).prop_map(
                |(op, left, right)| Expr::BinOp {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                }
            ),
            // Unary operation
            (arb_unaryop(), inner).prop_map(|(op, operand)| {
                Expr::UnaryOp {
                    op,
                    operand: Box::new(operand),
                }
            }),
        ]
    })
}

/// Generate a simple predicate (column op constant).
pub fn arb_simple_predicate() -> impl Strategy<Value = Expr> {
    (arb_column_expr(), arb_comparison_op(), arb_const_expr()).prop_map(
        |(col, op, val)| Expr::BinOp {
            op,
            left: Box::new(col),
            right: Box::new(val),
        },
    )
}

/// Generate an equality join predicate (col = col).
pub fn arb_eq_join_pred() -> impl Strategy<Value = Expr> {
    (arb_column_expr(), arb_column_expr()).prop_map(|(l, r)| {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(l),
            right: Box::new(r),
        }
    })
}

// -------------------------------------------------------------------
// Relational algebra node strategies
// -------------------------------------------------------------------

fn arb_scan() -> impl Strategy<Value = RelExpr> {
    arb_table_name().prop_map(|t| RelExpr::Scan {
        table: t,
        alias: None,
    })
}

fn arb_scan_with_alias() -> impl Strategy<Value = RelExpr> {
    (arb_table_name(), "[a-z]{1,3}").prop_map(|(t, alias)| {
        RelExpr::Scan {
            table: t,
            alias: Some(alias),
        }
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
    (arb_expr(0), arb_sort_direction(), arb_null_ordering()).prop_map(
        |(expr, direction, nulls)| SortKey {
            expr,
            direction,
            nulls,
        },
    )
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

// -------------------------------------------------------------------
// Compound relational expression strategies
// -------------------------------------------------------------------

fn arb_join_query(depth: u32) -> impl Strategy<Value = RelExpr> {
    let leaf = arb_scan();
    leaf.prop_recursive(depth, 64, 4, |inner| {
        (arb_join_type(), arb_eq_join_pred(), inner.clone(), inner)
            .prop_map(
                |(join_type, condition, left, right)| RelExpr::Join {
                    join_type,
                    condition,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            )
    })
}

fn arb_aggregate_query() -> impl Strategy<Value = RelExpr> {
    (
        prop::collection::vec(arb_expr(0), 0..=2),
        prop::collection::vec(arb_aggregate_expr(), 1..=3),
        arb_scan(),
    )
        .prop_map(|(group_by, aggregates, input)| RelExpr::Aggregate {
            group_by,
            aggregates,
            input: Box::new(input),
        })
}

fn arb_set_operation() -> impl Strategy<Value = RelExpr> {
    (any::<bool>(), arb_scan(), arb_scan(), 0..3u8).prop_map(
        |(all, left, right, op_type)| match op_type {
            0 => RelExpr::Union {
                all,
                left: Box::new(left),
                right: Box::new(right),
            },
            1 => RelExpr::Intersect {
                all,
                left: Box::new(left),
                right: Box::new(right),
            },
            _ => RelExpr::Except {
                all,
                left: Box::new(left),
                right: Box::new(right),
            },
        },
    )
}

/// Generate arbitrary relational expressions up to `depth`.
pub fn arb_rel_expr(depth: u32) -> impl Strategy<Value = RelExpr> {
    let leaf = prop_oneof![arb_scan(), arb_scan_with_alias(),];

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
                prop::collection::vec(arb_projection_column(), 1..=4),
                inner.clone()
            )
                .prop_map(|(columns, input)| {
                    RelExpr::Project {
                        columns,
                        input: Box::new(input),
                    }
                }),
            // Join
            (
                arb_join_type(),
                arb_expr(1),
                inner.clone(),
                inner.clone()
            )
                .prop_map(|(join_type, condition, left, right)| {
                    RelExpr::Join {
                        join_type,
                        condition,
                        left: Box::new(left),
                        right: Box::new(right),
                    }
                }),
            // Limit
            (0u64..100, 0u64..50, inner.clone()).prop_map(
                |(count, offset, input)| {
                    RelExpr::Limit {
                        count,
                        offset,
                        input: Box::new(input),
                    }
                }
            ),
            // Sort
            (
                prop::collection::vec(arb_sort_key(), 1..=3),
                inner.clone()
            )
                .prop_map(|(keys, input)| {
                    RelExpr::Sort {
                        keys,
                        input: Box::new(input),
                    }
                }),
            // Aggregate
            (
                prop::collection::vec(arb_expr(0), 0..=2),
                prop::collection::vec(arb_aggregate_expr(), 1..=3),
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
            (any::<bool>(), inner.clone(), inner.clone()).prop_map(
                |(all, left, right)| {
                    RelExpr::Union {
                        all,
                        left: Box::new(left),
                        right: Box::new(right),
                    }
                }
            ),
            // Intersect
            (any::<bool>(), inner.clone(), inner.clone()).prop_map(
                |(all, left, right)| {
                    RelExpr::Intersect {
                        all,
                        left: Box::new(left),
                        right: Box::new(right),
                    }
                }
            ),
            // Except
            (any::<bool>(), inner.clone(), inner.clone()).prop_map(
                |(all, left, right)| {
                    RelExpr::Except {
                        all,
                        left: Box::new(left),
                        right: Box::new(right),
                    }
                }
            ),
            // Distinct
            inner.clone().prop_map(|input| {
                RelExpr::Distinct {
                    input: Box::new(input),
                }
            }),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    #[test]
    fn generator_produces_expressions() {
        let gen = SqlGenerator::new();
        let mut runner = TestRunner::default();
        for _ in 0..10 {
            let val = gen
                .strategy()
                .new_tree(&mut runner)
                .expect("strategy should generate")
                .current();
            // Every generated value should be a valid RelExpr
            assert!(matches!(
                val,
                RelExpr::Scan { .. }
                    | RelExpr::Filter { .. }
                    | RelExpr::Project { .. }
                    | RelExpr::Join { .. }
                    | RelExpr::Aggregate { .. }
                    | RelExpr::Sort { .. }
                    | RelExpr::Limit { .. }
                    | RelExpr::Union { .. }
                    | RelExpr::Intersect { .. }
                    | RelExpr::Except { .. }
                    | RelExpr::Distinct { .. }
            ));
        }
    }

    #[test]
    fn join_strategy_produces_joins() {
        let gen = SqlGenerator::new();
        let mut runner = TestRunner::default();
        let mut saw_join = false;
        for _ in 0..20 {
            let val = gen
                .join_strategy()
                .new_tree(&mut runner)
                .expect("join strategy")
                .current();
            if matches!(val, RelExpr::Join { .. }) {
                saw_join = true;
            }
        }
        assert!(saw_join, "join strategy should produce at least one Join");
    }
}
