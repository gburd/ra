//! Query pattern fingerprinting for Bayesian pruning.
//!
//! Extracts structural features from `RelExpr` plan trees and
//! produces a coarse, hashable fingerprint. Two plans with the same
//! fingerprint are expected to have similar improvement probability
//! during optimization search.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::Expr;

/// Discretized structural summary of a plan subtree.
///
/// Fields are bucketed so the total fingerprint space stays small
/// (4 x 4 x 3 x 2 x 2 x 2 = 384 possible values). This ensures
/// each bucket accumulates observations quickly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlanFingerprint {
    /// Number of base tables (bucketed: 0-1, 2-3, 4-6, 7+).
    pub table_bucket: u8,
    /// Number of join operators (bucketed: 0, 1-2, 3-5, 6+).
    pub join_bucket: u8,
    /// Predicate complexity score (bucketed: low=0, medium=1, high=2).
    pub predicate_complexity: u8,
    /// Whether the subtree contains a cross join.
    pub has_cross_join: bool,
    /// Whether the subtree contains a correlated subquery.
    pub has_correlated_subquery: bool,
    /// Whether aggregation appears below a join.
    pub has_early_aggregation: bool,
}

impl PlanFingerprint {
    /// Build a fingerprint from a plan subtree.
    #[must_use]
    pub fn from_plan(plan: &RelExpr) -> Self {
        let tables = count_tables(plan);
        let joins = count_joins(plan);
        let pred = predicate_complexity(plan);

        Self {
            table_bucket: match tables {
                0..=1 => 0,
                2..=3 => 1,
                4..=6 => 2,
                _ => 3,
            },
            join_bucket: match joins {
                0 => 0,
                1..=2 => 1,
                3..=5 => 2,
                _ => 3,
            },
            predicate_complexity: match pred {
                0..=2 => 0,
                3..=6 => 1,
                _ => 2,
            },
            has_cross_join: contains_cross_join(plan),
            has_correlated_subquery: contains_correlated(plan),
            has_early_aggregation: has_agg_below_join(plan),
        }
    }
}

/// Count the number of base table scans in a plan tree.
#[must_use]
pub fn count_tables(plan: &RelExpr) -> usize {
    match plan {
        RelExpr::Scan { .. } | RelExpr::IndexScan { .. } => 1,
        RelExpr::Join { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => count_tables(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::CTE {
            definition, body, ..
        } => count_tables(definition) + count_tables(body),
        _ => 0,
    }
}

/// Count the number of join operators in a plan tree.
#[must_use]
pub fn count_joins(plan: &RelExpr) -> usize {
    match plan {
        RelExpr::Join { left, right, .. } => 1 + count_joins(left) + count_joins(right),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => count_joins(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => count_joins(left) + count_joins(right),
        _ => 0,
    }
}

/// Compute a predicate complexity score by counting nodes in all
/// predicate expressions across the plan tree.
#[must_use]
pub fn predicate_complexity(plan: &RelExpr) -> usize {
    match plan {
        RelExpr::Filter { predicate, input } => {
            expr_complexity(predicate) + predicate_complexity(input)
        }
        RelExpr::Join {
            condition,
            left,
            right,
            ..
        } => expr_complexity(condition) + predicate_complexity(left) + predicate_complexity(right),
        RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => predicate_complexity(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            predicate_complexity(left) + predicate_complexity(right)
        }
        _ => 0,
    }
}

/// Count the number of nodes in a scalar expression tree.
fn expr_complexity(expr: &Expr) -> usize {
    match expr {
        Expr::Column(_) | Expr::Const(_) => 1,
        Expr::BinOp { left, right, .. } => 1 + expr_complexity(left) + expr_complexity(right),
        Expr::UnaryOp { operand, .. } => 1 + expr_complexity(operand),
        Expr::Function { args, .. } => 1 + args.iter().map(expr_complexity).sum::<usize>(),
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            let base = operand.as_ref().map_or(0, |e| expr_complexity(e));
            let whens: usize = when_clauses
                .iter()
                .map(|(w, t)| expr_complexity(w) + expr_complexity(t))
                .sum();
            let else_cost = else_result.as_ref().map_or(0, |e| expr_complexity(e));
            1 + base + whens + else_cost
        }
        Expr::Cast { expr, .. } => 1 + expr_complexity(expr),
        Expr::Array(elems) => 1 + elems.iter().map(expr_complexity).sum::<usize>(),
        Expr::ArrayIndex(arr, idx) => 1 + expr_complexity(arr) + expr_complexity(idx),
        Expr::ArraySlice { array, start, end } => {
            1 + expr_complexity(array)
                + start.as_ref().map_or(0, |e| expr_complexity(e))
                + end.as_ref().map_or(0, |e| expr_complexity(e))
        }
        _ => 1,
    }
}

/// Check whether a plan tree contains any cross join.
#[must_use]
pub fn contains_cross_join(plan: &RelExpr) -> bool {
    match plan {
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            *join_type == JoinType::Cross || contains_cross_join(left) || contains_cross_join(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => contains_cross_join(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            contains_cross_join(left) || contains_cross_join(right)
        }
        _ => false,
    }
}

/// Check whether a plan tree contains a correlated subquery pattern.
///
/// Detects `RecursiveCTE` nodes or `CTE` nodes with body references
/// that indicate correlation. This is a conservative approximation
/// since the `Expr` type does not have explicit subquery variants.
#[must_use]
pub fn contains_correlated(plan: &RelExpr) -> bool {
    match plan {
        RelExpr::RecursiveCTE { .. } => true,
        RelExpr::CTE {
            definition, body, ..
        } => contains_correlated(definition) || contains_correlated(body),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => contains_correlated(input),
        RelExpr::Join { left, right, .. } => {
            contains_correlated(left) || contains_correlated(right)
        }
        _ => false,
    }
}

/// Check whether aggregation appears below a join in the plan tree.
///
/// Early aggregation (aggregate pushed below join) is a structural
/// pattern that has distinctive optimization characteristics.
#[must_use]
pub fn has_agg_below_join(plan: &RelExpr) -> bool {
    has_agg_below_join_rec(plan, false)
}

fn has_agg_below_join_rec(plan: &RelExpr, inside_join: bool) -> bool {
    match plan {
        RelExpr::Aggregate { input, .. } => {
            if inside_join {
                return true;
            }
            has_agg_below_join_rec(input, inside_join)
        }
        RelExpr::Join { left, right, .. } => {
            has_agg_below_join_rec(left, true) || has_agg_below_join_rec(right, true)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => has_agg_below_join_rec(input, inside_join),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            has_agg_below_join_rec(left, inside_join) || has_agg_below_join_rec(right, inside_join)
        }
        _ => false,
    }
}
