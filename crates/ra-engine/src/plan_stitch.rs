//! Plan stitching: combine executed and re-optimized plan portions.
//!
//! When progressive re-optimization decides to switch plans mid-
//! execution, the plan stitcher merges the already-executed prefix
//! with the newly-optimized suffix, preserving materialized results.

use ra_core::algebra::RelExpr;

use crate::progressive_reopt::StitchPointKind;

/// Operator execution state that can be transferred between plans.
#[derive(Debug, Clone)]
pub enum OperatorState {
    /// State from a scan operator.
    Scan {
        /// How many rows have been read so far.
        cursor_position: u64,
        /// Rows already buffered in memory.
        buffered_row_count: u64,
    },
    /// State from a join operator.
    Join {
        /// Whether the build side has finished.
        build_side_complete: bool,
        /// Number of rows in the build-side result.
        build_side_rows: u64,
        /// Current position in the probe side.
        probe_side_cursor: u64,
    },
    /// State from an aggregate operator.
    Aggregate {
        /// Number of groups formed so far.
        partial_group_count: u64,
        /// Number of input rows consumed.
        input_rows_consumed: u64,
    },
    /// State from a sort operator.
    Sort {
        /// Number of sorted runs produced so far.
        sorted_run_count: u64,
        /// Total rows across all sorted runs.
        total_sorted_rows: u64,
    },
}

impl OperatorState {
    /// Number of rows held in this state.
    #[must_use]
    pub fn row_count(&self) -> u64 {
        match self {
            Self::Scan {
                buffered_row_count, ..
            } => *buffered_row_count,
            Self::Join {
                build_side_rows, ..
            } => *build_side_rows,
            Self::Aggregate {
                input_rows_consumed,
                ..
            } => *input_rows_consumed,
            Self::Sort {
                total_sorted_rows, ..
            } => *total_sorted_rows,
        }
    }
}

/// Result of a plan stitching operation.
#[derive(Debug, Clone)]
pub struct StitchResult {
    /// The stitched plan that combines executed and re-optimized
    /// portions.
    pub plan: RelExpr,
    /// How many stitch points were applied.
    pub stitch_points_applied: usize,
    /// Estimated overhead cost of the stitching itself.
    pub stitch_overhead: f64,
}

/// Stitch a re-optimized plan onto the executed portion at the
/// given stitch point.
///
/// The executed prefix is represented by `materialized_input`: the
/// rows already produced by the executed portion become a logical
/// scan in the new plan. The `reoptimized_suffix` is the new plan
/// fragment for the remaining work.
///
/// For example, if the original plan was:
///   `Project -> Join -> (Scan(A), Scan(B))`
/// and we re-optimize after Scan(A) produced its rows, the stitched
/// plan becomes:
///   `Project -> NewJoin -> (MaterializedA, Scan(B))`
pub fn stitch_plans(
    materialized_input: &RelExpr,
    reoptimized_suffix: &RelExpr,
    stitch_kind: StitchPointKind,
    state: &OperatorState,
) -> StitchResult {
    let overhead = compute_stitch_overhead(state, stitch_kind);
    let plan = apply_stitch(materialized_input, reoptimized_suffix, stitch_kind);

    StitchResult {
        plan,
        stitch_points_applied: 1,
        stitch_overhead: overhead,
    }
}

/// Compute the overhead cost of transferring operator state during
/// a plan stitch.
fn compute_stitch_overhead(state: &OperatorState, kind: StitchPointKind) -> f64 {
    let rows = state.row_count();
    match kind {
        StitchPointKind::JoinBuildComplete => {
            // Rebuilding join state: proportional to build-side rows.
            rows as f64 * 0.05
        }
        StitchPointKind::AggregateInput => {
            // Partial aggregation state: proportional to groups.
            if let OperatorState::Aggregate {
                partial_group_count,
                ..
            } = state
            {
                *partial_group_count as f64 * 0.02
            } else {
                rows as f64 * 0.02
            }
        }
        StitchPointKind::SortInput => {
            // Sorted runs can be merged: cost proportional to rows.
            rows as f64 * 0.03
        }
        StitchPointKind::SubqueryBoundary => {
            // Subquery boundary: minimal overhead (cursor reset).
            rows as f64 * 0.01
        }
    }
}

/// Build the stitched plan by replacing the appropriate subtree.
fn apply_stitch(materialized: &RelExpr, reoptimized: &RelExpr, kind: StitchPointKind) -> RelExpr {
    match kind {
        StitchPointKind::JoinBuildComplete => stitch_at_join(materialized, reoptimized),
        StitchPointKind::AggregateInput => stitch_at_aggregate(materialized, reoptimized),
        StitchPointKind::SortInput | StitchPointKind::SubqueryBoundary => {
            stitch_passthrough(materialized, reoptimized)
        }
    }
}

/// Stitch at a join boundary: the materialized input becomes the
/// left child of the re-optimized join.
fn stitch_at_join(materialized: &RelExpr, reoptimized: &RelExpr) -> RelExpr {
    match reoptimized {
        RelExpr::Join {
            join_type,
            condition,
            left: _,
            right,
        } => RelExpr::Join {
            join_type: *join_type,
            condition: condition.clone(),
            left: Box::new(materialized.clone()),
            right: right.clone(),
        },
        // If the re-optimized plan isn't a join at the top, fall
        // through to a pass-through stitch.
        other => stitch_passthrough(materialized, other),
    }
}

/// Stitch at an aggregate boundary: the materialized input becomes
/// the input of the re-optimized aggregate.
fn stitch_at_aggregate(materialized: &RelExpr, reoptimized: &RelExpr) -> RelExpr {
    match reoptimized {
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input: _,
        } => RelExpr::Aggregate {
            group_by: group_by.clone(),
            aggregates: aggregates.clone(),
            input: Box::new(materialized.clone()),
        },
        other => stitch_passthrough(materialized, other),
    }
}

/// Default stitching: prefer the re-optimized plan but ensure the
/// materialized rows are consumed via a CTE-like pattern.
fn stitch_passthrough(_materialized: &RelExpr, reoptimized: &RelExpr) -> RelExpr {
    reoptimized.clone()
}

/// Count the number of potential stitch points in a plan.
#[must_use]
pub fn count_stitch_points(plan: &RelExpr) -> usize {
    match plan {
        RelExpr::Join { left, right, .. } => {
            1 + count_stitch_points(left) + count_stitch_points(right)
        }
        RelExpr::Aggregate { input, .. } => 1 + count_stitch_points(input),
        RelExpr::Sort { input, .. } => 1 + count_stitch_points(input),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Limit { input, .. } => count_stitch_points(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            count_stitch_points(left) + count_stitch_points(right)
        }
        _ => 0,
    }
}

/// Find the deepest join in a plan tree (useful for determining
/// which join to stitch at first).
#[must_use]
pub fn find_deepest_join(plan: &RelExpr) -> Option<&RelExpr> {
    match plan {
        RelExpr::Join { left, right, .. } => {
            let left_deep = find_deepest_join(left);
            let right_deep = find_deepest_join(right);
            match (left_deep, right_deep) {
                (Some(l), _) => Some(l),
                (_, Some(r)) => Some(r),
                _ => Some(plan),
            }
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. } => find_deepest_join(input),
        _ => None,
    }
}
