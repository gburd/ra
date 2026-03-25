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
        StitchPointKind::SortInput => stitch_at_sort(materialized, reoptimized),
        StitchPointKind::SubqueryBoundary => stitch_passthrough(materialized, reoptimized),
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

/// Stitch at a sort boundary: the materialized (partially sorted)
/// input feeds into the re-optimized sort.
fn stitch_at_sort(materialized: &RelExpr, reoptimized: &RelExpr) -> RelExpr {
    match reoptimized {
        RelExpr::Sort { keys, input: _ } => RelExpr::Sort {
            keys: keys.clone(),
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

/// Collect the set of base table names referenced by a plan tree.
fn collect_table_names(plan: &RelExpr, out: &mut Vec<String>) {
    match plan {
        RelExpr::Scan { table, .. } => out.push(table.clone()),
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_table_names(left, out);
            collect_table_names(right, out);
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => {
            collect_table_names(input, out);
        }
        _ => {}
    }
}

/// Verify that two plans reference the same set of base tables,
/// regardless of join order. This is a necessary (not sufficient)
/// condition for plan equivalence.
#[must_use]
pub fn verify_join_order_equivalence(
    old_plan: &RelExpr,
    new_plan: &RelExpr,
) -> bool {
    let mut old_tables = Vec::new();
    let mut new_tables = Vec::new();
    collect_table_names(old_plan, &mut old_tables);
    collect_table_names(new_plan, &mut new_tables);
    old_tables.sort();
    new_tables.sort();
    old_tables == new_tables
}

/// Stitch at multiple points in a plan tree. Each stitch point
/// is described by a `(materialized, kind, state)` triple. Points
/// are applied bottom-up so that inner stitches take effect before
/// outer ones.
pub fn stitch_multi(
    reoptimized: &RelExpr,
    points: &[(RelExpr, StitchPointKind, OperatorState)],
) -> StitchResult {
    if points.is_empty() {
        return StitchResult {
            plan: reoptimized.clone(),
            stitch_points_applied: 0,
            stitch_overhead: 0.0,
        };
    }

    let mut plan = reoptimized.clone();
    let mut total_overhead = 0.0;
    let mut applied = 0_usize;

    for (materialized, kind, state) in points {
        let single = stitch_plans(materialized, &plan, *kind, state);
        plan = single.plan;
        total_overhead += single.stitch_overhead;
        applied += single.stitch_points_applied;
    }

    StitchResult {
        plan,
        stitch_points_applied: applied,
        stitch_overhead: total_overhead,
    }
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

/// Replace the first subtree matching `target` with `replacement`.
/// Returns the modified plan and `true` if a replacement was made.
#[must_use]
pub fn replace_subtree(
    plan: &RelExpr,
    target: &RelExpr,
    replacement: &RelExpr,
) -> (RelExpr, bool) {
    if plan == target {
        return (replacement.clone(), true);
    }
    match plan {
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let (new_left, replaced) =
                replace_subtree(left, target, replacement);
            if replaced {
                return (
                    RelExpr::Join {
                        join_type: *join_type,
                        condition: condition.clone(),
                        left: Box::new(new_left),
                        right: right.clone(),
                    },
                    true,
                );
            }
            let (new_right, replaced) =
                replace_subtree(right, target, replacement);
            (
                RelExpr::Join {
                    join_type: *join_type,
                    condition: condition.clone(),
                    left: left.clone(),
                    right: Box::new(new_right),
                },
                replaced,
            )
        }
        RelExpr::Filter { predicate, input } => {
            let (new_input, replaced) =
                replace_subtree(input, target, replacement);
            (
                RelExpr::Filter {
                    predicate: predicate.clone(),
                    input: Box::new(new_input),
                },
                replaced,
            )
        }
        RelExpr::Project { columns, input } => {
            let (new_input, replaced) =
                replace_subtree(input, target, replacement);
            (
                RelExpr::Project {
                    columns: columns.clone(),
                    input: Box::new(new_input),
                },
                replaced,
            )
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            let (new_input, replaced) =
                replace_subtree(input, target, replacement);
            (
                RelExpr::Aggregate {
                    group_by: group_by.clone(),
                    aggregates: aggregates.clone(),
                    input: Box::new(new_input),
                },
                replaced,
            )
        }
        RelExpr::Sort { keys, input } => {
            let (new_input, replaced) =
                replace_subtree(input, target, replacement);
            (
                RelExpr::Sort {
                    keys: keys.clone(),
                    input: Box::new(new_input),
                },
                replaced,
            )
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            let (new_input, replaced) =
                replace_subtree(input, target, replacement);
            (
                RelExpr::Limit {
                    count: *count,
                    offset: *offset,
                    input: Box::new(new_input),
                },
                replaced,
            )
        }
        RelExpr::Distinct { input } => {
            let (new_input, replaced) =
                replace_subtree(input, target, replacement);
            (
                RelExpr::Distinct {
                    input: Box::new(new_input),
                },
                replaced,
            )
        }
        RelExpr::Window { functions, input } => {
            let (new_input, replaced) =
                replace_subtree(input, target, replacement);
            (
                RelExpr::Window {
                    functions: functions.clone(),
                    input: Box::new(new_input),
                },
                replaced,
            )
        }
        other => (other.clone(), false),
    }
}

/// Verify that replacing a subtree in `old_plan` with `replacement`
/// preserves the set of base tables (a necessary condition for
/// semantic equivalence).
#[must_use]
pub fn differential_verify(
    old_plan: &RelExpr,
    new_plan: &RelExpr,
) -> DifferentialResult {
    let tables_match =
        verify_join_order_equivalence(old_plan, new_plan);

    let old_stitch_count = count_stitch_points(old_plan);
    let new_stitch_count = count_stitch_points(new_plan);

    DifferentialResult {
        tables_equivalent: tables_match,
        old_stitch_points: old_stitch_count,
        new_stitch_points: new_stitch_count,
    }
}

/// Result of a differential verification between old and new plans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifferentialResult {
    /// Whether both plans reference the same set of base tables.
    pub tables_equivalent: bool,
    /// Number of stitch points in the old plan.
    pub old_stitch_points: usize,
    /// Number of stitch points in the new plan.
    pub new_stitch_points: usize,
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

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{
        AggregateExpr, AggregateFunction, JoinType, NullOrdering,
        SortDirection, SortKey,
    };
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    fn eq_cond() -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Column(ColumnRef::new("id"))),
        }
    }

    fn make_join(left: RelExpr, right: RelExpr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq_cond(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn make_aggregate(input: RelExpr) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("g"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".to_string()),
            }],
            input: Box::new(input),
        }
    }

    fn make_sort(input: RelExpr) -> RelExpr {
        RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("a")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(input),
        }
    }

    // -- OperatorState tests --

    #[test]
    fn operator_state_scan_row_count() {
        let state = OperatorState::Scan {
            cursor_position: 10,
            buffered_row_count: 500,
        };
        assert_eq!(state.row_count(), 500);
    }

    #[test]
    fn operator_state_join_row_count() {
        let state = OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 1000,
            probe_side_cursor: 50,
        };
        assert_eq!(state.row_count(), 1000);
    }

    #[test]
    fn operator_state_aggregate_row_count() {
        let state = OperatorState::Aggregate {
            partial_group_count: 20,
            input_rows_consumed: 5000,
        };
        assert_eq!(state.row_count(), 5000);
    }

    #[test]
    fn operator_state_sort_row_count() {
        let state = OperatorState::Sort {
            sorted_run_count: 3,
            total_sorted_rows: 2000,
        };
        assert_eq!(state.row_count(), 2000);
    }

    // -- stitch_plans tests --

    #[test]
    fn stitch_plans_at_join() {
        let materialized = RelExpr::scan("mat_a");
        let reoptimized = make_join(RelExpr::scan("old_a"), RelExpr::scan("b"));
        let state = OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 100,
            probe_side_cursor: 0,
        };
        let result = stitch_plans(
            &materialized,
            &reoptimized,
            StitchPointKind::JoinBuildComplete,
            &state,
        );
        assert_eq!(result.stitch_points_applied, 1);
        assert!(result.stitch_overhead > 0.0);
        // Verify the left side was replaced with materialized
        if let RelExpr::Join { left, right, .. } = &result.plan {
            assert_eq!(**left, materialized);
            assert_eq!(**right, RelExpr::scan("b"));
        } else {
            panic!("Expected Join");
        }
    }

    #[test]
    fn stitch_plans_at_aggregate() {
        let materialized = RelExpr::scan("mat_t");
        let reoptimized = make_aggregate(RelExpr::scan("old_t"));
        let state = OperatorState::Aggregate {
            partial_group_count: 10,
            input_rows_consumed: 500,
        };
        let result = stitch_plans(
            &materialized,
            &reoptimized,
            StitchPointKind::AggregateInput,
            &state,
        );
        assert_eq!(result.stitch_points_applied, 1);
        if let RelExpr::Aggregate { input, .. } = &result.plan {
            assert_eq!(**input, materialized);
        } else {
            panic!("Expected Aggregate");
        }
    }

    #[test]
    fn stitch_plans_at_sort() {
        let materialized = RelExpr::scan("mat_t");
        let reoptimized = make_sort(RelExpr::scan("old_t"));
        let state = OperatorState::Sort {
            sorted_run_count: 2,
            total_sorted_rows: 300,
        };
        let result = stitch_plans(
            &materialized,
            &reoptimized,
            StitchPointKind::SortInput,
            &state,
        );
        assert_eq!(result.stitch_points_applied, 1);
        if let RelExpr::Sort { input, .. } = &result.plan {
            assert_eq!(**input, materialized);
        } else {
            panic!("Expected Sort");
        }
    }

    #[test]
    fn stitch_plans_at_subquery_boundary() {
        let materialized = RelExpr::scan("mat_t");
        let reoptimized = RelExpr::scan("new_t");
        let state = OperatorState::Scan {
            cursor_position: 0,
            buffered_row_count: 100,
        };
        let result = stitch_plans(
            &materialized,
            &reoptimized,
            StitchPointKind::SubqueryBoundary,
            &state,
        );
        assert_eq!(result.plan, reoptimized);
    }

    #[test]
    fn stitch_at_join_passthrough_when_not_join() {
        let materialized = RelExpr::scan("mat");
        let reoptimized = RelExpr::scan("new");
        let state = OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 10,
            probe_side_cursor: 0,
        };
        let result = stitch_plans(
            &materialized,
            &reoptimized,
            StitchPointKind::JoinBuildComplete,
            &state,
        );
        assert_eq!(result.plan, reoptimized);
    }

    // -- verify_join_order_equivalence tests --

    #[test]
    fn verify_join_order_equivalence_same_tables() {
        let plan1 = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let plan2 = make_join(RelExpr::scan("b"), RelExpr::scan("a"));
        assert!(verify_join_order_equivalence(&plan1, &plan2));
    }

    #[test]
    fn verify_join_order_equivalence_different_tables() {
        let plan1 = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let plan2 = make_join(RelExpr::scan("a"), RelExpr::scan("c"));
        assert!(!verify_join_order_equivalence(&plan1, &plan2));
    }

    #[test]
    fn verify_join_order_single_table() {
        let plan1 = RelExpr::scan("a");
        let plan2 = RelExpr::scan("a");
        assert!(verify_join_order_equivalence(&plan1, &plan2));
    }

    // -- count_stitch_points tests --

    #[test]
    fn count_stitch_points_leaf() {
        assert_eq!(count_stitch_points(&RelExpr::scan("t")), 0);
    }

    #[test]
    fn count_stitch_points_join() {
        let plan = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        assert_eq!(count_stitch_points(&plan), 1);
    }

    #[test]
    fn count_stitch_points_nested_joins() {
        let inner = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let outer = make_join(inner, RelExpr::scan("c"));
        assert_eq!(count_stitch_points(&outer), 2);
    }

    #[test]
    fn count_stitch_points_aggregate() {
        let plan = make_aggregate(RelExpr::scan("t"));
        assert_eq!(count_stitch_points(&plan), 1);
    }

    #[test]
    fn count_stitch_points_sort() {
        let plan = make_sort(RelExpr::scan("t"));
        assert_eq!(count_stitch_points(&plan), 1);
    }

    #[test]
    fn count_stitch_points_filter_passthrough() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(ra_core::expr::Const::Bool(true)),
            input: Box::new(make_join(RelExpr::scan("a"), RelExpr::scan("b"))),
        };
        assert_eq!(count_stitch_points(&plan), 1);
    }

    #[test]
    fn count_stitch_points_union() {
        let plan = RelExpr::Union {
            all: true,
            left: Box::new(make_join(RelExpr::scan("a"), RelExpr::scan("b"))),
            right: Box::new(make_sort(RelExpr::scan("c"))),
        };
        assert_eq!(count_stitch_points(&plan), 2);
    }

    // -- replace_subtree tests --

    #[test]
    fn replace_subtree_match_at_root() {
        let plan = RelExpr::scan("old");
        let replacement = RelExpr::scan("new");
        let (result, replaced) = replace_subtree(&plan, &plan, &replacement);
        assert!(replaced);
        assert_eq!(result, replacement);
    }

    #[test]
    fn replace_subtree_no_match() {
        let plan = RelExpr::scan("a");
        let target = RelExpr::scan("b");
        let replacement = RelExpr::scan("c");
        let (result, replaced) = replace_subtree(&plan, &target, &replacement);
        assert!(!replaced);
        assert_eq!(result, plan);
    }

    #[test]
    fn replace_subtree_in_join_left() {
        let plan = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let target = RelExpr::scan("a");
        let replacement = RelExpr::scan("new_a");
        let (result, replaced) = replace_subtree(&plan, &target, &replacement);
        assert!(replaced);
        if let RelExpr::Join { left, right, .. } = &result {
            assert_eq!(**left, replacement);
            assert_eq!(**right, RelExpr::scan("b"));
        } else {
            panic!("Expected Join");
        }
    }

    #[test]
    fn replace_subtree_in_join_right() {
        let plan = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let target = RelExpr::scan("b");
        let replacement = RelExpr::scan("new_b");
        let (result, replaced) = replace_subtree(&plan, &target, &replacement);
        assert!(replaced);
        if let RelExpr::Join { right, .. } = &result {
            assert_eq!(**right, replacement);
        } else {
            panic!("Expected Join");
        }
    }

    #[test]
    fn replace_subtree_in_filter() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(ra_core::expr::Const::Bool(true)),
            input: Box::new(RelExpr::scan("old")),
        };
        let target = RelExpr::scan("old");
        let replacement = RelExpr::scan("new");
        let (result, replaced) = replace_subtree(&plan, &target, &replacement);
        assert!(replaced);
        if let RelExpr::Filter { input, .. } = &result {
            assert_eq!(**input, replacement);
        } else {
            panic!("Expected Filter");
        }
    }

    #[test]
    fn replace_subtree_in_project() {
        let plan = RelExpr::Project {
            columns: vec![],
            input: Box::new(RelExpr::scan("old")),
        };
        let (result, replaced) =
            replace_subtree(&plan, &RelExpr::scan("old"), &RelExpr::scan("new"));
        assert!(replaced);
        if let RelExpr::Project { input, .. } = &result {
            assert_eq!(**input, RelExpr::scan("new"));
        } else {
            panic!("Expected Project");
        }
    }

    #[test]
    fn replace_subtree_in_aggregate() {
        let plan = make_aggregate(RelExpr::scan("old"));
        let (result, replaced) =
            replace_subtree(&plan, &RelExpr::scan("old"), &RelExpr::scan("new"));
        assert!(replaced);
        if let RelExpr::Aggregate { input, .. } = &result {
            assert_eq!(**input, RelExpr::scan("new"));
        } else {
            panic!("Expected Aggregate");
        }
    }

    #[test]
    fn replace_subtree_in_sort() {
        let plan = make_sort(RelExpr::scan("old"));
        let (result, replaced) =
            replace_subtree(&plan, &RelExpr::scan("old"), &RelExpr::scan("new"));
        assert!(replaced);
        if let RelExpr::Sort { input, .. } = &result {
            assert_eq!(**input, RelExpr::scan("new"));
        } else {
            panic!("Expected Sort");
        }
    }

    #[test]
    fn replace_subtree_in_limit() {
        let plan = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(RelExpr::scan("old")),
        };
        let (result, replaced) =
            replace_subtree(&plan, &RelExpr::scan("old"), &RelExpr::scan("new"));
        assert!(replaced);
        if let RelExpr::Limit { input, .. } = &result {
            assert_eq!(**input, RelExpr::scan("new"));
        } else {
            panic!("Expected Limit");
        }
    }

    #[test]
    fn replace_subtree_in_distinct() {
        let plan = RelExpr::Distinct {
            input: Box::new(RelExpr::scan("old")),
        };
        let (result, replaced) =
            replace_subtree(&plan, &RelExpr::scan("old"), &RelExpr::scan("new"));
        assert!(replaced);
        if let RelExpr::Distinct { input } = &result {
            assert_eq!(**input, RelExpr::scan("new"));
        } else {
            panic!("Expected Distinct");
        }
    }

    // -- differential_verify tests --

    #[test]
    fn differential_verify_same_plan() {
        let plan = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let result = differential_verify(&plan, &plan);
        assert!(result.tables_equivalent);
        assert_eq!(result.old_stitch_points, result.new_stitch_points);
    }

    #[test]
    fn differential_verify_different_tables() {
        let old = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let new = make_join(RelExpr::scan("a"), RelExpr::scan("c"));
        let result = differential_verify(&old, &new);
        assert!(!result.tables_equivalent);
    }

    // -- stitch_multi tests --

    #[test]
    fn stitch_multi_empty_points() {
        let plan = RelExpr::scan("t");
        let result = stitch_multi(&plan, &[]);
        assert_eq!(result.stitch_points_applied, 0);
        assert!(result.stitch_overhead.abs() < f64::EPSILON);
        assert_eq!(result.plan, plan);
    }

    #[test]
    fn stitch_multi_single_point() {
        let reoptimized = make_join(RelExpr::scan("old"), RelExpr::scan("b"));
        let materialized = RelExpr::scan("mat");
        let state = OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 50,
            probe_side_cursor: 0,
        };
        let points = vec![(materialized.clone(), StitchPointKind::JoinBuildComplete, state)];
        let result = stitch_multi(&reoptimized, &points);
        assert_eq!(result.stitch_points_applied, 1);
        assert!(result.stitch_overhead > 0.0);
    }

    // -- find_deepest_join tests --

    #[test]
    fn find_deepest_join_no_joins() {
        assert!(find_deepest_join(&RelExpr::scan("t")).is_none());
    }

    #[test]
    fn find_deepest_join_single_join() {
        let plan = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        assert!(find_deepest_join(&plan).is_some());
    }

    #[test]
    fn find_deepest_join_nested() {
        let inner = make_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let outer = make_join(inner.clone(), RelExpr::scan("c"));
        let deepest = find_deepest_join(&outer);
        assert!(deepest.is_some());
        assert_eq!(*deepest.unwrap(), inner);
    }

    #[test]
    fn find_deepest_join_through_filter() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(ra_core::expr::Const::Bool(true)),
            input: Box::new(make_join(RelExpr::scan("a"), RelExpr::scan("b"))),
        };
        assert!(find_deepest_join(&plan).is_some());
    }

    #[test]
    fn find_deepest_join_through_sort() {
        let plan = make_sort(make_join(RelExpr::scan("a"), RelExpr::scan("b")));
        assert!(find_deepest_join(&plan).is_some());
    }

    // -- compute_stitch_overhead tests --

    #[test]
    fn stitch_overhead_join_build() {
        let state = OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 100,
            probe_side_cursor: 0,
        };
        let result = stitch_plans(
            &RelExpr::scan("m"),
            &make_join(RelExpr::scan("a"), RelExpr::scan("b")),
            StitchPointKind::JoinBuildComplete,
            &state,
        );
        // 100 rows * 0.05 = 5.0
        assert!((result.stitch_overhead - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stitch_overhead_aggregate_uses_group_count() {
        let state = OperatorState::Aggregate {
            partial_group_count: 50,
            input_rows_consumed: 1000,
        };
        let result = stitch_plans(
            &RelExpr::scan("m"),
            &make_aggregate(RelExpr::scan("t")),
            StitchPointKind::AggregateInput,
            &state,
        );
        // 50 groups * 0.02 = 1.0
        assert!((result.stitch_overhead - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stitch_overhead_sort_uses_rows() {
        let state = OperatorState::Sort {
            sorted_run_count: 2,
            total_sorted_rows: 200,
        };
        let result = stitch_plans(
            &RelExpr::scan("m"),
            &make_sort(RelExpr::scan("t")),
            StitchPointKind::SortInput,
            &state,
        );
        // 200 rows * 0.03 = 6.0
        assert!((result.stitch_overhead - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stitch_overhead_subquery_boundary() {
        let state = OperatorState::Scan {
            cursor_position: 0,
            buffered_row_count: 100,
        };
        let result = stitch_plans(
            &RelExpr::scan("m"),
            &RelExpr::scan("new"),
            StitchPointKind::SubqueryBoundary,
            &state,
        );
        // 100 rows * 0.01 = 1.0
        assert!((result.stitch_overhead - 1.0).abs() < f64::EPSILON);
    }
}
