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

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, ProjectionColumn, RelExpr,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, SubQueryType, UnaryOp};

use crate::correlation_analysis;

/// Decorrelate subquery expressions in a `RelExpr` tree.
///
/// Recursively walks the tree bottom-up. For each `Filter` node whose
/// predicate contains a subquery expression, replaces the filter with
/// the appropriate join form.
///
/// Returns the transformed tree. If no subqueries are present, the tree
/// is returned unchanged (structurally identical clone).
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "single-pass walk over RelExpr; per-variant decorrelation is clearer inline"
)]
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
        } => {
            let new_left = decorrelate(left);
            let new_right = decorrelate(right);
            // If join condition contains a subquery, convert to
            // CrossJoin + Filter and re-decorrelate the filter.
            if contains_subquery(condition) {
                let cross = RelExpr::Join {
                    join_type: JoinType::Cross,
                    condition: Expr::Const(Const::Bool(true)),
                    left: Box::new(new_left),
                    right: Box::new(new_right),
                };
                let filter = RelExpr::Filter {
                    predicate: condition.clone(),
                    input: Box::new(cross),
                };
                decorrelate(&filter)
            } else {
                RelExpr::Join {
                    join_type: *join_type,
                    condition: condition.clone(),
                    left: Box::new(new_left),
                    right: Box::new(new_right),
                }
            }
        }
        RelExpr::Project { columns, input } => {
            // Subqueries in projection columns (scalar `(SELECT ...)`) are
            // handled by the plan builder as SubPlan/Param nodes, which is
            // correct for correlated cases; leave the columns intact and only
            // decorrelate the input.
            RelExpr::Project {
                columns: columns.clone(),
                input: Box::new(decorrelate(input)),
            }
        }
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
        RelExpr::Insert {
            table,
            columns,
            source,
            on_conflict,
            returning,
        } => RelExpr::Insert {
            table: table.clone(),
            columns: columns.clone(),
            source: Box::new(decorrelate(source)),
            on_conflict: on_conflict.clone(),
            returning: returning.clone(),
        },
        RelExpr::Update {
            table,
            assignments,
            filter,
            from,
            returning,
        } => {
            let new_from = from.as_deref().map(|f| Box::new(decorrelate(f)));
            let new_filter = filter.as_ref().map(decorrelate_scalar_subqueries);
            let new_assignments = assignments
                .iter()
                .map(|(col, expr)| (col.clone(), decorrelate_scalar_subqueries(expr)))
                .collect();
            RelExpr::Update {
                table: table.clone(),
                assignments: new_assignments,
                filter: new_filter,
                from: new_from,
                returning: returning.clone(),
            }
        }
        RelExpr::Delete {
            table,
            filter,
            using,
            returning,
        } => {
            let new_using = using.as_deref().map(|u| Box::new(decorrelate(u)));
            let new_filter = filter.as_ref().map(decorrelate_scalar_subqueries);
            RelExpr::Delete {
                table: table.clone(),
                filter: new_filter,
                using: new_using,
                returning: returning.clone(),
            }
        }
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
            // AND: try to decorrelate each conjunct, then recurse
            // on the remaining predicate so nested subqueries are
            // also transformed.
            if *op == BinOp::And {
                if let Some(result) = try_decorrelate_predicate(left, input.clone()) {
                    let wrapped = RelExpr::Filter {
                        predicate: *right.clone(),
                        input: Box::new(result),
                    };
                    return Some(decorrelate(&wrapped));
                }
                if let Some(result) = try_decorrelate_predicate(right, input) {
                    let wrapped = RelExpr::Filter {
                        predicate: *left.clone(),
                        input: Box::new(result),
                    };
                    return Some(decorrelate(&wrapped));
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

            // Nested subquery: e.g., `agg > 0.95 * (SELECT scalar)`
            // Use replace_subquery_in_expr to hoist into CrossJoin.
            if contains_subquery(left) || contains_subquery(right) {
                let mut counter = 0usize;
                let (new_pred, new_input) = replace_subquery_in_expr(
                    &Expr::BinOp {
                        op: *op,
                        left: left.clone(),
                        right: right.clone(),
                    },
                    input,
                    &mut counter,
                );
                return Some(RelExpr::Filter {
                    predicate: new_pred,
                    input: Box::new(new_input),
                });
            }
            None
        }

        other => {
            if contains_subquery(other) {
                let mut counter = 0usize;
                let (new_pred, new_input) =
                    replace_subquery_in_expr(other, input, &mut counter);
                Some(RelExpr::Filter {
                    predicate: new_pred,
                    input: Box::new(new_input),
                })
            } else {
                None
            }
        }
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
            // x IN (SELECT col FROM Q WHERE corr) → SemiJoin(x = col AND corr).
            // The correlation predicate must move into the join condition; a
            // nested-loop semi-join has no way to evaluate a correlated filter
            // left inside the inner side (it would reference the outer row).
            let in_cond = build_in_condition(test_expr, &decorrelated_query);
            let (inner_query, corr) = extract_correlation_predicate(&decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Semi,
                condition: and_exprs(in_cond, corr),
                left: Box::new(input),
                right: Box::new(inner_query),
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
            // x op ANY (SELECT col FROM Q WHERE corr) → SemiJoin(x op col AND corr)
            let any_cond = build_in_condition(test_expr, &decorrelated_query);
            let (inner_query, corr) = extract_correlation_predicate(&decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Semi,
                condition: and_exprs(any_cond, corr),
                left: Box::new(input),
                right: Box::new(inner_query),
            })
        }
        SubQueryType::All => {
            // x op ALL (SELECT col FROM Q WHERE corr) →
            //   AntiJoin(corr AND NOT(x op col), input, Q)
            let base_cond = build_in_condition(test_expr, &decorrelated_query);
            let negated = Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(base_cond),
            };
            let (inner_query, corr) = extract_correlation_predicate(&decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Anti,
                condition: and_exprs(negated, corr),
                left: Box::new(input),
                right: Box::new(inner_query),
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
            // NOT IN is NOT a plain anti-join: SQL NULL semantics make
            // `x NOT IN (S)` yield NULL (→ no row) whenever S contains a NULL
            // or x is NULL, which an anti-join does not reproduce. PostgreSQL
            // only rewrites it to an anti-join when both sides are provably
            // NOT NULL. Decline to decorrelate so the predicate stays a
            // sub-query and the plan builder falls back to PG (correct NULL
            // handling) rather than emitting an unsound anti-join.
            None
        }
        SubQueryType::Any => {
            // NOT (x op ANY (...)) → AntiJoin(x op col AND corr)
            let any_cond = build_in_condition(test_expr, &decorrelated_query);
            let (inner_query, corr) = extract_correlation_predicate(&decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Anti,
                condition: and_exprs(any_cond, corr),
                left: Box::new(input),
                right: Box::new(inner_query),
            })
        }
        SubQueryType::All => {
            // NOT (x op ALL (...)) → SemiJoin(NOT(x op col) AND corr)
            let base_cond = build_in_condition(test_expr, &decorrelated_query);
            let negated = Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(base_cond),
            };
            let (inner_query, corr) = extract_correlation_predicate(&decorrelated_query);
            Some(RelExpr::Join {
                join_type: JoinType::Semi,
                condition: and_exprs(negated, corr),
                left: Box::new(input),
                right: Box::new(inner_query),
            })
        }
        SubQueryType::Scalar => None,
    }
}

/// Convert a scalar comparison with a subquery into a join + filter.
///
/// For **uncorrelated** scalar subqueries:
/// `x = (SELECT val FROM T)` becomes `Filter(x = Q.col, CrossJoin(input, Q))`
///
/// For **correlated** scalar aggregate subqueries (e.g., TPC-H Q20):
/// `x > (SELECT agg(...) FROM T WHERE t.a = outer.b AND local_preds)`
/// becomes `Filter(x > __agg, LeftJoin(input, Aggregate(...), on correlation))`
fn decorrelate_scalar_comparison(
    op: BinOp,
    other_side: &Expr,
    subquery: &RelExpr,
    input: RelExpr,
) -> Option<RelExpr> {
    // Try correlated aggregate decorrelation first
    if let Some(result) = try_decorrelate_correlated_scalar(op, other_side, subquery, input) {
        return Some(result);
    }

    // Fallback: uncorrelated (or non-aggregate-correlatable) scalar subquery.
    // Decline — leaving `Filter(x op (SELECT ...))` intact routes it to the
    // plan builder's scalar-subquery path (EXPR_SUBLINK SubPlan / InitPlan +
    // PARAM_EXEC), which renders natively and mirrors PostgreSQL. A CrossJoin
    // with the subquery is not renderable when the join side is an Aggregate
    // (and is unsafe for correlated cases), so it is no longer emitted.
    None
}

/// Build an equality condition for IN/ANY/ALL subqueries.
///
/// Given `test_expr` (e.g., the `x` in `x IN (SELECT col FROM T)`)
/// and the subquery, builds `test_expr = first_output_col(subquery)`.
/// Combine two predicates with AND, dropping a trivial `TRUE` (so an
/// uncorrelated subquery keeps its single-clause join condition).
fn and_exprs(a: Expr, b: Expr) -> Expr {
    match (&a, &b) {
        (Expr::Const(Const::Bool(true)), _) => b,
        (_, Expr::Const(Const::Bool(true))) => a,
        _ => Expr::BinOp {
            op: BinOp::And,
            left: Box::new(a),
            right: Box::new(b),
        },
    }
}

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
        RelExpr::Project { columns, input } => {
            if let Some(first) = columns.first() {
                // Qualify a bare output column with the sub-query's own
                // relation so a join condition built from it resolves to the
                // inner table — not a same-named column of the outer relation
                // (which produced `outer.a = outer.a`, an always-true clause
                // that silently broke IN / NOT IN).
                match &first.expr {
                    Expr::Column(c) if c.table.is_none() => match leaf_scan_rel(input) {
                        Some(rel) => Expr::Column(ColumnRef::qualified(rel, c.column.clone())),
                        None => first.expr.clone(),
                    },
                    _ => first.expr.clone(),
                }
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

/// Alias (or table name) of the sub-query's leaf scan, used to qualify a bare
/// output column. Returns `None` for multi-relation bodies (ambiguous).
fn leaf_scan_rel(query: &RelExpr) -> Option<String> {
    match query {
        RelExpr::Scan { table, alias } => Some(alias.clone().unwrap_or_else(|| table.clone())),
        RelExpr::Filter { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Distinct { input }
        | RelExpr::Project { input, .. } => leaf_scan_rel(input),
        _ => None,
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
        // Look through Project and Limit nodes (e.g., SELECT 1 FROM ... WHERE corr,
        // or SELECT ... LIMIT 1) — both look through to the inner correlation.
        RelExpr::Project { input, .. } | RelExpr::Limit { input, .. } => {
            extract_correlation_predicate(input)
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

/// Walk a scalar `Expr` and recursively decorrelate any embedded subqueries.
///
/// For DML assignment/filter expressions that may contain scalar subqueries,
/// this returns the expression unchanged if no subquery is present. When a
/// subquery is found we cannot fully decorrelate in isolation (that requires
/// the relational context), so we recursively decorrelate the subquery's
/// internal relation while preserving the scalar wrapper.
fn decorrelate_scalar_subqueries(expr: &Expr) -> Expr {
    match expr {
        Expr::SubQuery {
            subquery_type,
            query,
            test_expr,
        } => Expr::SubQuery {
            subquery_type: subquery_type.clone(),
            query: Box::new(decorrelate(query)),
            test_expr: test_expr.clone(),
        },
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: *op,
            left: Box::new(decorrelate_scalar_subqueries(left)),
            right: Box::new(decorrelate_scalar_subqueries(right)),
        },
        Expr::UnaryOp { op, operand } => Expr::UnaryOp {
            op: *op,
            operand: Box::new(decorrelate_scalar_subqueries(operand)),
        },
        Expr::Function { name, args } => Expr::Function {
            name: name.clone(),
            args: args.iter().map(decorrelate_scalar_subqueries).collect(),
        },
        _ => expr.clone(),
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

// ─── Correlated scalar aggregate subquery decorrelation ───────────────────────
//
// Handles the pattern:
//   Filter(x op (SELECT agg_expr FROM T WHERE corr_preds AND local_preds), R)
// →
//   Filter(x op rewritten_agg_expr,
//     LeftJoin(R, Aggregate(group_by=corr_inner_cols, aggs, Filter(local, T)),
//              on corr_preds))

/// Attempt to decorrelate a correlated scalar aggregate subquery.
///
/// Detects subqueries of the form:
///   `Project([expr_with_aggregates], Filter(pred, Scan(table)))`
/// where `pred` contains equality predicates referencing outer columns.
///
/// Returns `None` if the subquery is not correlated or doesn't match
/// the supported pattern (falls back to `CrossJoin` in caller).
fn try_decorrelate_correlated_scalar(
    op: BinOp,
    other_side: &Expr,
    subquery: &RelExpr,
    input: RelExpr,
) -> Option<RelExpr> {
    // Match: Project { columns, input: Filter { predicate, input: inner } }
    let (proj_columns, filter_pred, inner_rel) = match subquery {
        RelExpr::Project {
            columns,
            input: filter_box,
        } => match filter_box.as_ref() {
            RelExpr::Filter { predicate, input } => {
                Some((columns, predicate, input.as_ref()))
            }
            _ => None,
        },
        _ => None,
    }?;

    // Build inner scope from the subquery's inner relation
    let inner_scope = correlation_analysis::build_scope(inner_rel);
    if inner_scope.tables.is_empty() {
        return None;
    }

    // Split the filter predicate into correlation and local predicates
    let conjuncts = flatten_and(filter_pred);
    let (corr_preds, local_preds) =
        correlation_analysis::classify_predicates(&conjuncts, &inner_scope);

    // Must have at least one correlation predicate to qualify
    if corr_preds.is_empty() {
        return None;
    }

    // Extract inner-side columns from correlation predicates for GROUP BY,
    // and build the join condition from correlation equalities.
    let mut group_by_exprs: Vec<Expr> = Vec::new();
    let mut join_conditions: Vec<Expr> = Vec::new();

    for pred in &corr_preds {
        if let Expr::BinOp {
            op: BinOp::Eq,
            left,
            right,
        } = pred
        {
            let (inner_col, _outer_col) =
                correlation_analysis::classify_eq_sides(left, right, &inner_scope)?;
            group_by_exprs.push(Expr::Column(inner_col.clone()));
            // Keep the original equality as the join condition
            join_conditions.push(pred.clone());
        } else {
            // Non-equality correlation predicates are not supported
            return None;
        }
    }

    // The projection must contain aggregate functions to decorrelate
    if proj_columns.is_empty() {
        return None;
    }
    let proj_expr = &proj_columns[0].expr;

    // Replace aggregate function calls with column references, collecting
    // the AggregateExpr nodes for the Aggregate operator.
    let mut agg_counter = 0usize;
    let (rewritten_expr, aggregates) =
        replace_aggregates_in_expr(proj_expr, &mut agg_counter);

    // Must have found at least one aggregate
    if aggregates.is_empty() {
        return None;
    }

    // Build local filter predicate (AND together local predicates)
    let local_filter = and_together(&local_preds);

    // Build inner: Filter(local_preds, inner_rel) or just inner_rel
    let filtered_inner = if let Some(pred) = local_filter {
        RelExpr::Filter {
            predicate: pred,
            input: Box::new(inner_rel.clone()),
        }
    } else {
        inner_rel.clone()
    };

    // Build: Aggregate(group_by, aggs, filtered_inner)
    let agg_node = RelExpr::Aggregate {
        group_by: group_by_exprs,
        aggregates,
        input: Box::new(filtered_inner),
    };

    // Build join condition (AND together all correlation equalities)
    let join_cond = and_together(&join_conditions)
        .unwrap_or(Expr::Const(Const::Bool(true)));

    // Build: LeftJoin(input, agg_node, on join_cond)
    let left_join = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: join_cond,
        left: Box::new(input),
        right: Box::new(agg_node),
    };

    // Build comparison predicate: other_side op rewritten_expr
    let comparison = Expr::BinOp {
        op,
        left: Box::new(other_side.clone()),
        right: Box::new(rewritten_expr),
    };

    Some(RelExpr::Filter {
        predicate: comparison,
        input: Box::new(left_join),
    })
}

/// Flatten an AND-tree into a vector of conjuncts.
fn flatten_and(expr: &Expr) -> Vec<Expr> {
    match expr {
        Expr::BinOp {
            op: BinOp::And,
            left,
            right,
        } => {
            let mut result = flatten_and(left);
            result.extend(flatten_and(right));
            result
        }
        other => vec![other.clone()],
    }
}

/// AND together a list of predicates. Returns None for empty list.
fn and_together(preds: &[Expr]) -> Option<Expr> {
    preds.iter().cloned().reduce(|acc, p| Expr::BinOp {
        op: BinOp::And,
        left: Box::new(acc),
        right: Box::new(p),
    })
}

/// Replace aggregate function calls (SUM, COUNT, AVG, MIN, MAX) in an
/// expression with column references to generated alias names, collecting
/// the corresponding `AggregateExpr` entries.
///
/// Returns the rewritten expression and the list of aggregates found.
fn replace_aggregates_in_expr(
    expr: &Expr,
    counter: &mut usize,
) -> (Expr, Vec<AggregateExpr>) {
    let mut aggregates = Vec::new();
    let rewritten = rewrite_expr_aggregates(expr, counter, &mut aggregates);
    (rewritten, aggregates)
}

fn rewrite_expr_aggregates(
    expr: &Expr,
    counter: &mut usize,
    aggregates: &mut Vec<AggregateExpr>,
) -> Expr {
    match expr {
        Expr::Function { name, args } => {
            if let Some(agg_fn) = parse_aggregate_function(name) {
                let alias = format!("__agg_{counter}");
                *counter += 1;
                let arg = args.first().cloned();
                aggregates.push(AggregateExpr {
                    function: agg_fn,
                    arg,
                    distinct: false,
                    alias: Some(alias.clone()),
                });
                return Expr::Column(ColumnRef::new(alias));
            }
            // Not an aggregate function; recurse into args
            Expr::Function {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|a| rewrite_expr_aggregates(a, counter, aggregates))
                    .collect(),
            }
        }
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: *op,
            left: Box::new(rewrite_expr_aggregates(left, counter, aggregates)),
            right: Box::new(rewrite_expr_aggregates(right, counter, aggregates)),
        },
        Expr::UnaryOp { op, operand } => Expr::UnaryOp {
            op: *op,
            operand: Box::new(rewrite_expr_aggregates(
                operand, counter, aggregates,
            )),
        },
        Expr::Cast { expr, target_type } => Expr::Cast {
            expr: Box::new(rewrite_expr_aggregates(expr, counter, aggregates)),
            target_type: target_type.clone(),
        },
        // Leaf expressions pass through unchanged
        _ => expr.clone(),
    }
}

/// Parse an aggregate function name into the enum variant, if recognized.
fn parse_aggregate_function(name: &str) -> Option<AggregateFunction> {
    match name.to_uppercase().as_str() {
        "SUM" => Some(AggregateFunction::Sum),
        "COUNT" => Some(AggregateFunction::Count),
        "AVG" => Some(AggregateFunction::Avg),
        "MIN" => Some(AggregateFunction::Min),
        "MAX" => Some(AggregateFunction::Max),
        _ => None,
    }
}

// ─── Project-column subquery decorrelation ────────────────────────────────────
//
// Handles scalar subqueries appearing in SELECT-list columns:
//   SELECT (SELECT agg FROM T WHERE corr = outer.col), other_cols FROM R
// →
//   Project([__sq_col_0, other_cols],
//     LeftJoin(R, decorrelated_subquery, on corr_condition))

/// Decorrelate subqueries found in projection columns.
///
/// For each column containing a scalar subquery, the subquery is extracted
/// Replace the first subquery found in an expression with a column reference,
/// joining the subquery's result into the input relation.
///
/// Returns the rewritten expression and the new input (with join added).
#[expect(
    clippy::too_many_lines,
    reason = "expression-tree walk with subquery rewriting; per-variant logic is clearer inline"
)]
fn replace_subquery_in_expr(
    expr: &Expr,
    input: RelExpr,
    counter: &mut usize,
) -> (Expr, RelExpr) {
    match expr {
        Expr::SubQuery {
            subquery_type: SubQueryType::Scalar,
            query,
            ..
        } => {
            let alias = format!("__sq_col_{counter}");
            *counter += 1;

            let decorrelated_sq = decorrelate(query);

            // Try correlated decorrelation: check if the subquery has a
            // filter with correlation predicates
            if let Some((joined_input, col_ref)) =
                try_correlated_project_subquery(&decorrelated_sq, &input, &alias)
            {
                return (col_ref, joined_input);
            }

            // Uncorrelated: CrossJoin with the subquery, reference first output col
            let sq_col = first_output_column(&decorrelated_sq);
            let col_ref = if alias.is_empty() {
                sq_col
            } else {
                Expr::Column(ColumnRef::new(&alias))
            };

            // Wrap in a single-row projection with the alias
            let aliased_sq = RelExpr::Project {
                columns: vec![ProjectionColumn {
                    expr: first_output_column(&decorrelated_sq),
                    alias: Some(alias),
                }],
                input: Box::new(decorrelated_sq),
            };

            let cross = RelExpr::Join {
                join_type: JoinType::Cross,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(input),
                right: Box::new(aliased_sq),
            };

            (col_ref, cross)
        }
        // EXISTS/IN/ANY/ALL in a project column: wrap in a CASE WHEN
        Expr::SubQuery {
            subquery_type,
            query,
            test_expr,
        } => {
            let alias = format!("__sq_col_{counter}");
            *counter += 1;

            // Convert to a semi-join existence check by wrapping in
            // a left join and using CASE WHEN joined_col IS NOT NULL
            let decorrelated_sq = decorrelate(query);
            let marker_col = format!("__exists_{}", counter.wrapping_sub(1));

            // Build a left join that produces a marker column
            let marker_sq = add_existence_marker(&decorrelated_sq, &marker_col);

            let condition = match subquery_type {
                SubQueryType::Exists => {
                    let (inner_q, corr_cond) =
                        extract_correlation_predicate(&decorrelated_sq);
                    let marked = add_existence_marker(&inner_q, &marker_col);
                    let join = RelExpr::Join {
                        join_type: JoinType::LeftOuter,
                        condition: corr_cond,
                        left: Box::new(input),
                        right: Box::new(marked),
                    };
                    // CASE WHEN marker IS NOT NULL THEN TRUE ELSE FALSE
                    let case_expr = Expr::Case {
                        operand: None,
                        when_clauses: vec![(
                            Expr::UnaryOp {
                                op: UnaryOp::IsNotNull,
                                operand: Box::new(Expr::Column(ColumnRef::new(
                                    &marker_col,
                                ))),
                            },
                            Expr::Const(Const::Bool(true)),
                        )],
                        else_result: Some(Box::new(Expr::Const(Const::Bool(false)))),
                    };
                    return (case_expr, join);
                }
                SubQueryType::In => {
                    build_in_condition(test_expr.as_deref(), &decorrelated_sq)
                }
                _ => Expr::Const(Const::Bool(true)),
            };

            // Fallback: LeftJoin with condition, use CASE on marker
            let join = RelExpr::Join {
                join_type: JoinType::LeftOuter,
                condition,
                left: Box::new(input),
                right: Box::new(marker_sq),
            };
            let case_expr = Expr::Case {
                operand: None,
                when_clauses: vec![(
                    Expr::UnaryOp {
                        op: UnaryOp::IsNotNull,
                        operand: Box::new(Expr::Column(ColumnRef::new(&marker_col))),
                    },
                    Expr::Const(Const::Bool(true)),
                )],
                else_result: Some(Box::new(Expr::Const(Const::Bool(false)))),
            };
            let _ = alias;
            (case_expr, join)
        }
        // Recurse into binary ops
        Expr::BinOp { op, left, right } => {
            if contains_subquery(left) {
                let (new_left, new_input) =
                    replace_subquery_in_expr(left, input, counter);
                let (new_right, final_input) = if contains_subquery(right) {
                    replace_subquery_in_expr(right, new_input, counter)
                } else {
                    (*right.clone(), new_input)
                };
                (
                    Expr::BinOp {
                        op: *op,
                        left: Box::new(new_left),
                        right: Box::new(new_right),
                    },
                    final_input,
                )
            } else if contains_subquery(right) {
                let (new_right, new_input) =
                    replace_subquery_in_expr(right, input, counter);
                (
                    Expr::BinOp {
                        op: *op,
                        left: left.clone(),
                        right: Box::new(new_right),
                    },
                    new_input,
                )
            } else {
                (expr.clone(), input)
            }
        }
        Expr::Function { name, args } => {
            let mut current = input;
            let mut new_args = Vec::with_capacity(args.len());
            for arg in args {
                if contains_subquery(arg) {
                    let (new_arg, new_input) =
                        replace_subquery_in_expr(arg, current, counter);
                    current = new_input;
                    new_args.push(new_arg);
                } else {
                    new_args.push(arg.clone());
                }
            }
            (
                Expr::Function {
                    name: name.clone(),
                    args: new_args,
                },
                current,
            )
        }
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            let mut current = input;
            let new_operand = if let Some(op) = operand {
                if contains_subquery(op) {
                    let (new_op, new_input) =
                        replace_subquery_in_expr(op, current, counter);
                    current = new_input;
                    Some(Box::new(new_op))
                } else {
                    Some(op.clone())
                }
            } else {
                None
            };
            let mut new_whens = Vec::with_capacity(when_clauses.len());
            for (cond, result) in when_clauses {
                let (new_cond, c1) = if contains_subquery(cond) {
                    replace_subquery_in_expr(cond, current, counter)
                } else {
                    (cond.clone(), current)
                };
                let (new_result, c2) = if contains_subquery(result) {
                    replace_subquery_in_expr(result, c1, counter)
                } else {
                    (result.clone(), c1)
                };
                current = c2;
                new_whens.push((new_cond, new_result));
            }
            let new_else = if let Some(e) = else_result {
                if contains_subquery(e) {
                    let (new_e, new_input) =
                        replace_subquery_in_expr(e, current, counter);
                    current = new_input;
                    Some(Box::new(new_e))
                } else {
                    Some(e.clone())
                }
            } else {
                None
            };
            (
                Expr::Case {
                    operand: new_operand,
                    when_clauses: new_whens,
                    else_result: new_else,
                },
                current,
            )
        }
        Expr::Cast { expr: inner, target_type } => {
            if contains_subquery(inner) {
                let (new_inner, new_input) =
                    replace_subquery_in_expr(inner, input, counter);
                (
                    Expr::Cast {
                        expr: Box::new(new_inner),
                        target_type: target_type.clone(),
                    },
                    new_input,
                )
            } else {
                (expr.clone(), input)
            }
        }
        Expr::UnaryOp { op, operand } => {
            if contains_subquery(operand) {
                let (new_operand, new_input) =
                    replace_subquery_in_expr(operand, input, counter);
                (
                    Expr::UnaryOp {
                        op: *op,
                        operand: Box::new(new_operand),
                    },
                    new_input,
                )
            } else {
                (expr.clone(), input)
            }
        }
        _ => (expr.clone(), input),
    }
}

/// Try to decorrelate a correlated scalar subquery in a projection column.
///
/// If the subquery matches:
///   `Project([agg_expr], Filter(corr_pred AND local, Scan(T)))`
/// Converts to `LeftJoin` with aggregate, returns the joined input and col ref.
fn try_correlated_project_subquery(
    subquery: &RelExpr,
    input: &RelExpr,
    alias: &str,
) -> Option<(RelExpr, Expr)> {
    // Match: Project { columns, input: Filter { predicate, input: inner } }
    let (proj_columns, filter_pred, inner_rel) = match subquery {
        RelExpr::Project {
            columns,
            input: filter_box,
        } => match filter_box.as_ref() {
            RelExpr::Filter { predicate, input } => {
                Some((columns, predicate, input.as_ref()))
            }
            _ => None,
        },
        _ => None,
    }?;

    let inner_scope = correlation_analysis::build_scope(inner_rel);
    if inner_scope.tables.is_empty() {
        return None;
    }

    let conjuncts = flatten_and(filter_pred);
    let (corr_preds, local_preds) =
        correlation_analysis::classify_predicates(&conjuncts, &inner_scope);

    if corr_preds.is_empty() {
        return None;
    }

    // Build group-by and join conditions from correlation predicates
    let mut group_by_exprs: Vec<Expr> = Vec::new();
    let mut join_conditions: Vec<Expr> = Vec::new();

    for pred in &corr_preds {
        if let Expr::BinOp {
            op: BinOp::Eq,
            left,
            right,
        } = pred
        {
            let (inner_col, _outer_col) =
                correlation_analysis::classify_eq_sides(left, right, &inner_scope)?;
            group_by_exprs.push(Expr::Column(inner_col.clone()));
            join_conditions.push(pred.clone());
        } else {
            return None;
        }
    }

    if proj_columns.is_empty() {
        return None;
    }
    let proj_expr = &proj_columns[0].expr;

    let mut agg_counter = 0usize;
    let (rewritten_expr, aggregates) =
        replace_aggregates_in_expr(proj_expr, &mut agg_counter);

    // If no aggregates found, still proceed — the expression might be a
    // simple column reference from a correlated subquery
    let local_filter = and_together(&local_preds);
    let filtered_inner = if let Some(pred) = local_filter {
        RelExpr::Filter {
            predicate: pred,
            input: Box::new(inner_rel.clone()),
        }
    } else {
        inner_rel.clone()
    };

    let right_side = if aggregates.is_empty() {
        // No aggregate: just use the filtered inner with a project
        RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: proj_expr.clone(),
                alias: Some(alias.to_owned()),
            }],
            input: Box::new(filtered_inner),
        }
    } else {
        // With aggregates: wrap in Aggregate node
        let agg_node = RelExpr::Aggregate {
            group_by: group_by_exprs,
            aggregates,
            input: Box::new(filtered_inner),
        };
        // Project the rewritten expression with the alias
        RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: rewritten_expr.clone(),
                alias: Some(alias.to_owned()),
            }],
            input: Box::new(agg_node),
        }
    };

    let join_cond = and_together(&join_conditions)
        .unwrap_or(Expr::Const(Const::Bool(true)));

    let left_join = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: join_cond,
        left: Box::new(input.clone()),
        right: Box::new(right_side),
    };

    let col_ref = Expr::Column(ColumnRef::new(alias));
    Some((left_join, col_ref))
}

/// Add an existence marker column to a relation (for EXISTS decorrelation
/// in project columns). Wraps in a Project that adds a constant TRUE column.
fn add_existence_marker(rel: &RelExpr, marker_name: &str) -> RelExpr {
    RelExpr::Project {
        columns: vec![
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("*")),
                alias: None,
            },
            ProjectionColumn {
                expr: Expr::Const(Const::Bool(true)),
                alias: Some(marker_name.to_owned()),
            },
        ],
        input: Box::new(rel.clone()),
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test panics are diagnostics, not production failure modes"
)]
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
    fn not_in_subquery_is_not_decorrelated() {
        // NOT IN must NOT become a plain anti-join: SQL NULL semantics differ.
        // Decorrelation declines, leaving the predicate intact so the plan
        // builder defers to PostgreSQL (correct NULL handling).
        let subquery = RelExpr::scan("returns");
        let predicate = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::SubQuery {
                subquery_type: SubQueryType::In,
                query: Box::new(subquery),
                test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
            }),
        };
        let input = RelExpr::scan("orders").filter(predicate.clone());

        let result = decorrelate(&input);
        // Unchanged: still a Filter carrying the NOT IN sub-query (no anti-join).
        match &result {
            RelExpr::Filter { predicate: p, .. } => {
                assert_eq!(*p, predicate, "NOT IN predicate should be preserved");
            }
            other => panic!("Expected Filter preserving NOT IN, got: {other:?}"),
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
    fn scalar_subquery_comparison_preserved_for_subplan() {
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
        // Uncorrelated scalar comparison subqueries are no longer rewritten
        // into a CrossJoin; they are preserved in the predicate and lowered by
        // the plan builder's SubPlan/InitPlan path (matches PostgreSQL).
        match &result {
            RelExpr::Filter { predicate, input } => {
                assert!(
                    matches!(input.as_ref(), RelExpr::Scan { .. }),
                    "expected Filter directly over Scan (no join introduced), got: {input:?}"
                );
                assert!(
                    contains_subquery(predicate),
                    "scalar subquery should be preserved in the predicate: {predicate:?}"
                );
            }
            other => panic!("Expected Filter over Scan, got: {other:?}"),
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

    // -- D1-D3 regression suite ----------------------------------

    /// D1: `> ANY (SELECT ...)` lowers to a `SemiJoin`. Pre-D1 the audit
    /// memory recorded that quantified comparisons stayed embedded as
    /// `__gt_any` function calls and were never decorrelated; the
    /// parser now emits proper `SubQueryType::Any` so this test pins
    /// the expected shape.
    #[test]
    fn quantified_any_decorrelates_to_semi_join() {
        let subquery = RelExpr::scan("orders");
        let predicate = Expr::SubQuery {
            subquery_type: SubQueryType::Any,
            query: Box::new(subquery),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("price")))),
        };
        let input = RelExpr::scan("products").filter(predicate);

        let result = decorrelate(&input);
        assert!(
            !tree_contains_subquery(&result),
            "ANY subquery should be eliminated by decorrelation: {result:?}"
        );
        let has_semi = matches!(
            &result,
            RelExpr::Join {
                join_type: JoinType::Semi,
                ..
            }
        );
        assert!(has_semi, "expected SemiJoin, got {result:?}");
    }

    /// D1: `< ALL (SELECT ...)` lowers to an `AntiJoin` (negated semi).
    #[test]
    fn quantified_all_decorrelates_to_anti_join() {
        let subquery = RelExpr::scan("blacklist");
        let predicate = Expr::SubQuery {
            subquery_type: SubQueryType::All,
            query: Box::new(subquery),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
        };
        let input = RelExpr::scan("users").filter(predicate);

        let result = decorrelate(&input);
        assert!(
            !tree_contains_subquery(&result),
            "ALL subquery should be eliminated by decorrelation: {result:?}"
        );
        let has_anti = matches!(
            &result,
            RelExpr::Join {
                join_type: JoinType::Anti,
                ..
            }
        );
        assert!(has_anti, "expected AntiJoin, got {result:?}");
    }

    /// Walk a plan tree; true if any JOIN node's condition contains a subquery.
    fn join_condition_contains_subquery(rel: &RelExpr) -> bool {
        if let RelExpr::Join { condition, .. } = rel {
            if contains_subquery(condition) {
                return true;
            }
        }
        rel.children()
            .into_iter()
            .any(join_condition_contains_subquery)
    }

    /// D2: a subquery embedded in a JOIN's ON-condition must be hoisted out of
    /// the join condition. It is lifted into a `CrossJoin` + Filter; the scalar
    /// subquery itself is then preserved in the filter predicate for the plan
    /// builder's SubPlan path, but no JOIN condition may still contain it.
    #[test]
    fn subquery_inside_join_condition_is_decorrelated() {
        let inner_sq = Expr::SubQuery {
            subquery_type: SubQueryType::Scalar,
            query: Box::new(RelExpr::scan("threshold_table")),
            test_expr: None,
        };
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "amount"))),
                right: Box::new(inner_sq),
            },
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };

        let result = decorrelate(&join);
        assert!(
            !join_condition_contains_subquery(&result),
            "subquery must be hoisted out of every JOIN condition: {result:?}"
        );
    }

    /// D3: a subquery inside a CTE body must recurse through
    /// `decorrelate(body)` and itself be lowered. Pre-D3 the audit
    /// memory said CTE bodies weren't visited; the recursion at the
    /// `RelExpr::CTE` arm of `decorrelate` covers that.
    #[test]
    fn subquery_inside_cte_body_is_decorrelated() {
        // WITH active AS (SELECT * FROM users) SELECT * FROM active
        //   WHERE id IN (SELECT user_id FROM orders)
        let body_predicate = Expr::SubQuery {
            subquery_type: SubQueryType::In,
            query: Box::new(RelExpr::scan("orders")),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
        };
        let body = RelExpr::scan("active").filter(body_predicate);
        let cte = RelExpr::CTE {
            name: "active".to_string(),
            definition: Box::new(RelExpr::scan("users")),
            body: Box::new(body),
        };

        let result = decorrelate(&cte);
        assert!(
            !tree_contains_subquery(&result),
            "CTE body subquery should be eliminated: {result:?}"
        );
    }

    /// D3: a subquery inside a CTE's *definition* must also be
    /// decorrelated. Together with the body case, this covers both
    /// halves of the audit's CTE finding.
    #[test]
    fn subquery_inside_cte_definition_is_decorrelated() {
        // WITH x AS (SELECT * FROM t WHERE id IN (SELECT id FROM s)) SELECT * FROM x
        let def_predicate = Expr::SubQuery {
            subquery_type: SubQueryType::In,
            query: Box::new(RelExpr::scan("s")),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
        };
        let definition = RelExpr::scan("t").filter(def_predicate);
        let cte = RelExpr::CTE {
            name: "x".to_string(),
            definition: Box::new(definition),
            body: Box::new(RelExpr::scan("x")),
        };

        let result = decorrelate(&cte);
        assert!(
            !tree_contains_subquery(&result),
            "CTE definition subquery should be eliminated: {result:?}"
        );
    }
}
