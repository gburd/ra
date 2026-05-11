//! Pre-optimization pass: convert subquery predicates to joins.
//!
//! This pass walks the `RelExpr` tree bottom-up and converts subquery
//! expressions inside `Filter` predicates into equivalent join forms.
//! By running before e-graph conversion, we avoid needing to represent
//! `Expr::SubQuery` in the `RelLang` e-graph language.
//!
//! # Transformations
//!
//! | Input pattern | Output |
//! |---|---|
//! | `Filter(x IN (SELECT col FROM Q), R)` | `SemiJoin(R.x = Q.col, R, Q)` |
//! | `Filter(x NOT IN (SELECT col FROM Q), R)` | `AntiJoin(R.x = Q.col, R, Q)` |
//! | `Filter(EXISTS (SELECT ... FROM Q WHERE corr), R)` | `SemiJoin(corr, R, Q)` |
//! | `Filter(NOT EXISTS (...), R)` | `AntiJoin(corr, R, Q)` |
//! | `Filter(x = (SELECT scalar), R)` | `Filter(x = Q.col, CrossJoin(R, Q))` |

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, SubQueryType, UnaryOp};

/// Decorrelate subquery expressions in a `RelExpr` tree.
///
/// Recursively walks the tree bottom-up. For each `Filter` node whose
/// predicate contains a subquery expression, replaces the filter with
/// the appropriate join form.
///
/// Returns the transformed tree. If no subqueries are present, the tree
/// is returned unchanged (structurally identical clone).
#[must_use]
pub fn decorrelate(expr: &RelExpr) -> RelExpr {
    match expr {
        RelExpr::Filter { predicate, input } => {
            // First, recursively decorrelate the input
            let new_input = decorrelate(input);

            // Check if the predicate contains a subquery
            if let Some(result) = try_decorrelate_predicate(predicate, new_input.clone()) {
                result
            } else {
                // No subquery in predicate; rebuild with decorrelated input
                RelExpr::Filter {
                    predicate: predicate.clone(),
                    input: Box::new(new_input),
                }
            }
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => RelExpr::Join {
            join_type: *join_type,
            condition: condition.clone(),
            left: Box::new(decorrelate(left)),
            right: Box::new(decorrelate(right)),
        },
        RelExpr::Project { columns, input } => RelExpr::Project {
            columns: columns.clone(),
            input: Box::new(decorrelate(input)),
        },
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => RelExpr::Aggregate {
            group_by: group_by.clone(),
            aggregates: aggregates.clone(),
            input: Box::new(decorrelate(input)),
        },
        RelExpr::Sort { keys, input } => RelExpr::Sort {
            keys: keys.clone(),
            input: Box::new(decorrelate(input)),
        },
        RelExpr::Limit {
            count,
            offset,
            input,
        } => RelExpr::Limit {
            count: *count,
            offset: *offset,
            input: Box::new(decorrelate(input)),
        },
        RelExpr::Distinct { input } => RelExpr::Distinct {
            input: Box::new(decorrelate(input)),
        },
        RelExpr::Union { left, right, all } => RelExpr::Union {
            left: Box::new(decorrelate(left)),
            right: Box::new(decorrelate(right)),
            all: *all,
        },
        RelExpr::Intersect { left, right, all } => RelExpr::Intersect {
            left: Box::new(decorrelate(left)),
            right: Box::new(decorrelate(right)),
            all: *all,
        },
        RelExpr::Except { left, right, all } => RelExpr::Except {
            left: Box::new(decorrelate(left)),
            right: Box::new(decorrelate(right)),
            all: *all,
        },
        RelExpr::CTE {
            name,
            definition,
            body,
        } => RelExpr::CTE {
            name: name.clone(),
            definition: Box::new(decorrelate(definition)),
            body: Box::new(decorrelate(body)),
        },
        RelExpr::Window { functions, input } => RelExpr::Window {
            functions: functions.clone(),
            input: Box::new(decorrelate(input)),
        },
        // Leaf nodes and nodes without subexpression inputs
        _ => expr.clone(),
    }
}

/// Try to decorrelate a filter predicate containing a subquery.
///
/// Returns `Some(new_rel_expr)` if the predicate contains a subquery
/// that was successfully converted to a join. Returns `None` if no
/// subquery is present or if the pattern isn't supported.
fn try_decorrelate_predicate(predicate: &Expr, input: RelExpr) -> Option<RelExpr> {
    match predicate {
        // Direct subquery in filter position
        Expr::SubQuery {
            subquery_type,
            query,
            test_expr,
        } => decorrelate_subquery(subquery_type, query, test_expr.as_deref(), input),

        // NOT wrapping a subquery: NOT EXISTS → AntiJoin, NOT IN → AntiJoin
        Expr::UnaryOp {
            op: UnaryOp::Not,
            operand,
        } => {
            if let Expr::SubQuery {
                subquery_type,
                query,
                test_expr,
            } = operand.as_ref()
            {
                decorrelate_negated_subquery(subquery_type, query, test_expr.as_deref(), input)
            } else {
                None
            }
        }

        // Binary operations: handle AND specially, then scalar subquery comparisons
        Expr::BinOp { op, left, right } => {
            // AND: try to decorrelate each conjunct
            if *op == BinOp::And {
                if let Some(result) = try_decorrelate_predicate(left, input.clone()) {
                    return Some(RelExpr::Filter {
                        predicate: *right.clone(),
                        input: Box::new(result),
                    });
                }
                if let Some(result) = try_decorrelate_predicate(right, input) {
                    return Some(RelExpr::Filter {
                        predicate: *left.clone(),
                        input: Box::new(result),
                    });
                }
                return None;
            }

            // Check right side for scalar subquery
            if let Expr::SubQuery {
                subquery_type: SubQueryType::Scalar,
                query,
                ..
            } = right.as_ref()
            {
                return decorrelate_scalar_comparison(*op, left, query, input);
            }
            // Check left side for scalar subquery
            if let Expr::SubQuery {
                subquery_type: SubQueryType::Scalar,
                query,
                ..
            } = left.as_ref()
            {
                return decorrelate_scalar_comparison(*op, right, query, input);
            }
            None
        }

        _ => None,
    }
}

/// Convert a subquery expression to a join form.
fn decorrelate_subquery(
    subquery_type: &SubQueryType,
    query: &RelExpr,
    test_expr: Option<&Expr>,
    input: RelExpr,
) -> Option<RelExpr> {
    let decorrelated_query = decorrelate(query);

    match subquery_type {
        SubQueryType::In => {
            // x IN (SELECT col FROM Q) → SemiJoin(x = Q.col, input, Q)
            let condition = build_in_condition(test_expr, &decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Semi,
                condition,
                left: Box::new(input),
                right: Box::new(decorrelated_query),
            })
        }
        SubQueryType::Exists => {
            // EXISTS (SELECT ... FROM Q WHERE corr) → SemiJoin(corr, input, Q)
            let (inner_query, condition) = extract_correlation_predicate(&decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Semi,
                condition,
                left: Box::new(input),
                right: Box::new(inner_query),
            })
        }
        SubQueryType::Any => {
            // x op ANY (SELECT col FROM Q) → SemiJoin(x op Q.col, input, Q)
            let condition = build_in_condition(test_expr, &decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Semi,
                condition,
                left: Box::new(input),
                right: Box::new(decorrelated_query),
            })
        }
        SubQueryType::All => {
            // x op ALL (SELECT col FROM Q) → AntiJoin(NOT(x op Q.col), input, Q)
            let base_cond = build_in_condition(test_expr, &decorrelated_query);
            let negated = Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(base_cond),
            };
            Some(RelExpr::Join {
                join_type: JoinType::Anti,
                condition: negated,
                left: Box::new(input),
                right: Box::new(decorrelated_query),
            })
        }
        SubQueryType::Scalar => {
            // Scalar subquery in filter position without comparison:
            // WHERE (SELECT 1) → WHERE TRUE (if constant)
            // For non-constant scalar subqueries, we can't safely decorrelate
            // without a comparison operator, so return None.
            None
        }
    }
}

/// Convert a negated subquery expression to a join form.
fn decorrelate_negated_subquery(
    subquery_type: &SubQueryType,
    query: &RelExpr,
    test_expr: Option<&Expr>,
    input: RelExpr,
) -> Option<RelExpr> {
    let decorrelated_query = decorrelate(query);

    match subquery_type {
        SubQueryType::Exists => {
            // NOT EXISTS (SELECT ... FROM Q WHERE corr) → AntiJoin(corr, input, Q)
            let (inner_query, condition) = extract_correlation_predicate(&decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Anti,
                condition,
                left: Box::new(input),
                right: Box::new(inner_query),
            })
        }
        SubQueryType::In => {
            // NOT IN (SELECT col FROM Q) → AntiJoin(x = Q.col, input, Q)
            let condition = build_in_condition(test_expr, &decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Anti,
                condition,
                left: Box::new(input),
                right: Box::new(decorrelated_query),
            })
        }
        SubQueryType::Any => {
            // NOT (x op ANY (...)) → AntiJoin(x op Q.col, input, Q)
            let condition = build_in_condition(test_expr, &decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Anti,
                condition,
                left: Box::new(input),
                right: Box::new(decorrelated_query),
            })
        }
        SubQueryType::All => {
            // NOT (x op ALL (...)) → SemiJoin(NOT(x op Q.col), input, Q)
            let base_cond = build_in_condition(test_expr, &decorrelated_query);
            let negated = Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(base_cond),
            };
            Some(RelExpr::Join {
                join_type: JoinType::Semi,
                condition: negated,
                left: Box::new(input),
                right: Box::new(decorrelated_query),
            })
        }
        SubQueryType::Scalar => None,
    }
}

/// Convert a scalar comparison with a subquery into a cross join + filter.
///
/// `x = (SELECT val FROM T)` becomes:
/// `Filter(x = Q.col, CrossJoin(input, Q))`
///
/// Always succeeds (returns `Some`) but returns `Option` for interface
/// consistency with `try_decorrelate_predicate`.
#[expect(clippy::unnecessary_wraps)]
fn decorrelate_scalar_comparison(
    op: BinOp,
    other_side: &Expr,
    subquery: &RelExpr,
    input: RelExpr,
) -> Option<RelExpr> {
    let decorrelated_query = decorrelate(subquery);

    // Extract the output column of the scalar subquery
    let subquery_col = first_output_column(&decorrelated_query);

    // Build comparison: other_side op subquery_col
    let condition = Expr::BinOp {
        op,
        left: Box::new(other_side.clone()),
        right: Box::new(subquery_col),
    };

    // CrossJoin(input, subquery) then Filter
    let cross = RelExpr::Join {
        join_type: JoinType::Cross,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(input),
        right: Box::new(decorrelated_query),
    };

    Some(RelExpr::Filter {
        predicate: condition,
        input: Box::new(cross),
    })
}

/// Build an equality condition for IN/ANY/ALL subqueries.
///
/// Given `test_expr` (e.g., the `x` in `x IN (SELECT col FROM T)`)
/// and the subquery, builds `test_expr = first_output_col(subquery)`.
fn build_in_condition(test_expr: Option<&Expr>, subquery: &RelExpr) -> Expr {
    let subquery_col = first_output_column(subquery);
    let left = test_expr
        .cloned()
        .unwrap_or(Expr::Const(Const::Bool(true)));

    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(subquery_col),
    }
}

/// Extract the first output column expression from a subquery.
///
/// For `SELECT col FROM T`, returns a column reference to `col`.
/// For projections, uses the first projected column.
/// Falls back to a generic column reference if structure is unclear.
fn first_output_column(query: &RelExpr) -> Expr {
    match query {
        RelExpr::Project { columns, .. } => {
            if let Some(first) = columns.first() {
                first.expr.clone()
            } else {
                Expr::Column(ColumnRef::new("__subquery_col"))
            }
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Distinct { input } => first_output_column(input),
        _ => {
            // For scan nodes, we don't know the schema, so use a
            // placeholder that downstream passes can resolve.
            Expr::Column(ColumnRef::new("__subquery_col"))
        }
    }
}

/// Extract correlation predicate from a subquery.
///
/// For `EXISTS (SELECT * FROM T WHERE T.x = outer.y)`, extracts:
/// - The inner query (SELECT * FROM T) without the correlation filter
/// - The correlation predicate (T.x = outer.y)
///
/// If no explicit correlation filter exists, returns the query as-is
/// with a TRUE condition (uncorrelated EXISTS → semi join with TRUE).
fn extract_correlation_predicate(query: &RelExpr) -> (RelExpr, Expr) {
    match query {
        RelExpr::Filter { predicate, input } => {
            // The filter predicate is the correlation condition
            (*input.clone(), predicate.clone())
        }
        _ => {
            // No correlation filter; uncorrelated EXISTS
            // SemiJoin with TRUE condition preserves all outer rows
            // when the subquery returns at least one row.
            (query.clone(), Expr::Const(Const::Bool(true)))
        }
    }
}

/// Check if an expression contains any subquery.
pub fn contains_subquery(expr: &Expr) -> bool {
    match expr {
        Expr::SubQuery { .. } => true,
        Expr::BinOp { left, right, .. } => contains_subquery(left) || contains_subquery(right),
        Expr::UnaryOp { operand, .. } => contains_subquery(operand),
        Expr::Function { args, .. } => args.iter().any(contains_subquery),
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            operand.as_ref().is_some_and(|e| contains_subquery(e))
                || when_clauses
                    .iter()
                    .any(|(c, r)| contains_subquery(c) || contains_subquery(r))
                || else_result.as_ref().is_some_and(|e| contains_subquery(e))
        }
        Expr::Cast { expr, .. } => contains_subquery(expr),
        _ => false,
    }
}

/// Check if a `RelExpr` tree contains any subquery expressions.
#[must_use]
pub fn tree_contains_subquery(rel: &RelExpr) -> bool {
    match rel {
        RelExpr::Filter { predicate, input } => {
            contains_subquery(predicate) || tree_contains_subquery(input)
        }
        RelExpr::Join {
            condition,
            left,
            right,
            ..
        } => {
            contains_subquery(condition)
                || tree_contains_subquery(left)
                || tree_contains_subquery(right)
        }
        RelExpr::Project { columns, input } => {
            columns.iter().any(|c| contains_subquery(&c.expr)) || tree_contains_subquery(input)
        }
        _ => {
            // Check children recursively
            rel.children().iter().any(|c| tree_contains_subquery(c))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr, SubQueryType};

    #[test]
    fn in_subquery_becomes_semi_join() {
        // SELECT * FROM orders WHERE id IN (SELECT order_id FROM returns)
        let subquery = RelExpr::scan("returns");
        let predicate = Expr::SubQuery {
            subquery_type: SubQueryType::In,
            query: Box::new(subquery),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
        };
        let input = RelExpr::scan("orders").filter(predicate);

        let result = decorrelate(&input);
        match &result {
            RelExpr::Join {
                join_type: JoinType::Semi,
                ..
            } => {} // Success
            other => panic!("Expected SemiJoin, got: {other:?}"),
        }
    }

    #[test]
    fn not_in_subquery_becomes_anti_join() {
        // SELECT * FROM orders WHERE id NOT IN (SELECT order_id FROM returns)
        let subquery = RelExpr::scan("returns");
        let predicate = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::SubQuery {
                subquery_type: SubQueryType::In,
                query: Box::new(subquery),
                test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
            }),
        };
        let input = RelExpr::scan("orders").filter(predicate);

        let result = decorrelate(&input);
        match &result {
            RelExpr::Join {
                join_type: JoinType::Anti,
                ..
            } => {} // Success
            other => panic!("Expected AntiJoin, got: {other:?}"),
        }
    }

    #[test]
    fn exists_subquery_becomes_semi_join() {
        // SELECT * FROM t WHERE EXISTS (SELECT 1 FROM s WHERE s.id = t.id)
        let correlated_filter = RelExpr::scan("s").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("s", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("t", "id"))),
        });
        let predicate = Expr::SubQuery {
            subquery_type: SubQueryType::Exists,
            query: Box::new(correlated_filter),
            test_expr: None,
        };
        let input = RelExpr::scan("t").filter(predicate);

        let result = decorrelate(&input);
        match &result {
            RelExpr::Join {
                join_type: JoinType::Semi,
                condition,
                ..
            } => {
                // Condition should be s.id = t.id
                match condition {
                    Expr::BinOp {
                        op: BinOp::Eq, ..
                    } => {}
                    other => panic!("Expected equality condition, got: {other:?}"),
                }
            }
            other => panic!("Expected SemiJoin, got: {other:?}"),
        }
    }

    #[test]
    fn not_exists_becomes_anti_join() {
        // SELECT * FROM t WHERE NOT EXISTS (SELECT 1 FROM s WHERE s.id = t.id)
        let correlated_filter = RelExpr::scan("s").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("s", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("t", "id"))),
        });
        let predicate = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::SubQuery {
                subquery_type: SubQueryType::Exists,
                query: Box::new(correlated_filter),
                test_expr: None,
            }),
        };
        let input = RelExpr::scan("t").filter(predicate);

        let result = decorrelate(&input);
        match &result {
            RelExpr::Join {
                join_type: JoinType::Anti,
                ..
            } => {} // Success
            other => panic!("Expected AntiJoin, got: {other:?}"),
        }
    }

    #[test]
    fn scalar_subquery_becomes_cross_join_filter() {
        // SELECT * FROM t WHERE 1 = (SELECT 1)
        let subquery = RelExpr::scan("dual");
        let predicate = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Const(Const::Int(1))),
            right: Box::new(Expr::SubQuery {
                subquery_type: SubQueryType::Scalar,
                query: Box::new(subquery),
                test_expr: None,
            }),
        };
        let input = RelExpr::scan("t").filter(predicate);

        let result = decorrelate(&input);
        match &result {
            RelExpr::Filter {
                input: cross_join, ..
            } => match cross_join.as_ref() {
                RelExpr::Join {
                    join_type: JoinType::Cross,
                    ..
                } => {} // Success
                other => panic!("Expected CrossJoin inside filter, got: {other:?}"),
            },
            other => panic!("Expected Filter over CrossJoin, got: {other:?}"),
        }
    }

    #[test]
    fn no_subquery_returns_unchanged() {
        let input = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("x"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });

        let result = decorrelate(&input);
        assert_eq!(result, input);
    }

    #[test]
    fn nested_subquery_decorrelated() {
        // Subquery inside a subquery's input
        let inner_sq = RelExpr::scan("c").filter(Expr::SubQuery {
            subquery_type: SubQueryType::In,
            query: Box::new(RelExpr::scan("d")),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
        });
        let outer_pred = Expr::SubQuery {
            subquery_type: SubQueryType::Exists,
            query: Box::new(inner_sq),
            test_expr: None,
        };
        let input = RelExpr::scan("a").filter(outer_pred);

        let result = decorrelate(&input);
        // Outer should be SemiJoin
        match &result {
            RelExpr::Join {
                join_type: JoinType::Semi,
                right,
                ..
            } => {
                // Inner subquery in right side should also be decorrelated
                match right.as_ref() {
                    RelExpr::Join {
                        join_type: JoinType::Semi,
                        ..
                    } => {} // Inner also became semi join
                    other => panic!("Expected inner SemiJoin, got: {other:?}"),
                }
            }
            other => panic!("Expected outer SemiJoin, got: {other:?}"),
        }
    }

    #[test]
    fn contains_subquery_detection() {
        let with_sq = Expr::SubQuery {
            subquery_type: SubQueryType::Scalar,
            query: Box::new(RelExpr::scan("t")),
            test_expr: None,
        };
        assert!(contains_subquery(&with_sq));

        let without_sq = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("x"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        };
        assert!(!contains_subquery(&without_sq));
    }
}
