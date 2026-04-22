//! Progressive re-optimization logic.
//!
//! Monitors execution cardinalities at stitch points and triggers
//! re-optimization when actual values diverge significantly from
//! estimates. Based on the Plan Stitch technique (RFC 0052).
//!
//! The [`BackgroundReoptimizer`] spawns a worker thread that
//! re-optimizes queries in the background while the initial "quick"
//! plan is executing. When a better plan is found, it is sent back
//! through an MPSC channel so the executor can atomically switch.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use tracing::{debug, warn};

/// Default divergence threshold: re-optimize when actual/estimated
/// exceeds this ratio (or its reciprocal).
pub const DIVERGENCE_THRESHOLD: f64 = 2.0;

/// Switch to alternative plan only when it saves at least this
/// fraction of the remaining cost (0.8 = 20% savings required).
pub const SWITCH_THRESHOLD: f64 = 0.8;

/// Per-row cost to copy buffered tuples during state transfer.
const COPY_COST_PER_ROW: f64 = 0.01;

/// Per-row cost to build a hash table entry during state transfer.
const HASH_BUILD_COST_PER_ROW: f64 = 0.05;

/// Per-row cost to sort entries during state transfer.
const SORT_COST_PER_ROW: f64 = 0.1;

/// Locations in the plan tree where monitoring and stitching occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StitchPointKind {
    /// After the build side of a join completes.
    JoinBuildComplete,
    /// Before aggregation input is consumed.
    AggregateInput,
    /// During sort input scanning.
    SortInput,
    /// At a subquery boundary.
    SubqueryBoundary,
}

/// Information about a cardinality divergence detected at a stitch
/// point.
#[derive(Debug, Clone)]
pub struct DivergenceInfo {
    /// Which operator reported the divergence.
    pub operator_name: String,
    /// Estimated cardinality from the optimizer.
    pub estimated_cardinality: u64,
    /// Actual cardinality observed at runtime.
    pub actual_cardinality: u64,
    /// Ratio of actual to estimated (>1 means underestimate).
    pub divergence_factor: f64,
    /// What kind of stitch point this is.
    pub stitch_kind: StitchPointKind,
}

/// Runtime statistics collected during execution, keyed by operator
/// name.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatistics {
    /// Actual row counts observed per operator.
    pub actual_row_counts: HashMap<String, u64>,
    /// Updated table cardinalities based on runtime observation.
    pub corrected_table_stats: HashMap<String, Statistics>,
}

/// Configuration for progressive re-optimization behavior.
#[derive(Debug, Clone)]
pub struct ReoptConfig {
    /// Divergence threshold: trigger re-optimization when
    /// actual/estimated exceeds this ratio or its reciprocal.
    pub divergence_threshold: f64,
    /// Only switch plans when the alternative saves at least this
    /// fraction of remaining cost.
    pub switch_threshold: f64,
    /// Maximum number of re-optimizations allowed per query.
    pub max_reoptimizations: usize,
}

impl Default for ReoptConfig {
    fn default() -> Self {
        Self {
            divergence_threshold: DIVERGENCE_THRESHOLD,
            switch_threshold: SWITCH_THRESHOLD,
            max_reoptimizations: 3,
        }
    }
}

/// Check whether the actual cardinality diverges enough from the
/// estimate to warrant re-optimization.
#[must_use]
pub fn should_reoptimize(estimated: u64, actual: u64, threshold: f64) -> bool {
    if estimated == 0 && actual == 0 {
        return false;
    }
    if estimated == 0 {
        return true;
    }
    let ratio = actual as f64 / estimated as f64;
    ratio > threshold || ratio < (1.0 / threshold)
}

/// Compute the divergence factor between actual and estimated
/// cardinalities. Returns `actual / estimated`, clamped to avoid
/// division by zero.
#[must_use]
pub fn divergence_factor(estimated: u64, actual: u64) -> f64 {
    if estimated == 0 {
        if actual == 0 {
            return 1.0;
        }
        return f64::MAX;
    }
    actual as f64 / estimated as f64
}

/// Estimate the cost of transferring state between operators during
/// a plan switch.
#[must_use]
pub fn estimate_stitch_cost(buffered_rows: u64, transfer_kind: StitchTransferKind) -> f64 {
    let per_row = match transfer_kind {
        StitchTransferKind::Copy => COPY_COST_PER_ROW,
        StitchTransferKind::HashBuild => HASH_BUILD_COST_PER_ROW,
        StitchTransferKind::Sort => SORT_COST_PER_ROW,
        StitchTransferKind::Discard => 0.0,
    };
    buffered_rows as f64 * per_row
}

/// Kinds of state transfer when switching between operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StitchTransferKind {
    /// Copy rows without transformation.
    Copy,
    /// Build a hash table from buffered rows.
    HashBuild,
    /// Sort buffered rows.
    Sort,
    /// Discard the existing state (e.g., hash table not needed).
    Discard,
}

/// Decide which transfer strategy is needed when switching from one
/// join implementation to another.
#[must_use]
pub fn join_transfer_kind(from: JoinImplKind, to: JoinImplKind) -> StitchTransferKind {
    match (from, to) {
        (JoinImplKind::Hash, JoinImplKind::Merge) => StitchTransferKind::Sort,
        (JoinImplKind::Hash, JoinImplKind::NestedLoop) => StitchTransferKind::Discard,
        (JoinImplKind::NestedLoop, JoinImplKind::Hash) => StitchTransferKind::HashBuild,
        (JoinImplKind::NestedLoop, JoinImplKind::Merge) => StitchTransferKind::Sort,
        (JoinImplKind::Merge, JoinImplKind::Hash) => StitchTransferKind::HashBuild,
        (JoinImplKind::Merge, JoinImplKind::NestedLoop) => StitchTransferKind::Discard,
        // Same implementation: simple copy of cursor state.
        _ => StitchTransferKind::Copy,
    }
}

/// Abstract join implementation categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinImplKind {
    /// Hash join.
    Hash,
    /// Merge (sort-merge) join.
    Merge,
    /// Nested loop join.
    NestedLoop,
}

/// Evaluate whether switching to a new plan is worthwhile given
/// the remaining cost of the current plan and the total cost of
/// the alternative (including state transfer overhead).
///
/// Returns `true` if the alternative saves at least
/// `(1 - switch_threshold)` of the remaining current cost.
#[must_use]
pub fn is_switch_worthwhile(
    remaining_current_cost: f64,
    alternative_cost: f64,
    stitch_overhead: f64,
    switch_threshold: f64,
) -> bool {
    let total_alternative = alternative_cost + stitch_overhead;
    total_alternative < remaining_current_cost * switch_threshold
}

/// Estimate the remaining cost of the current plan, given that
/// `progress_fraction` of the work has been completed.
#[must_use]
pub fn estimate_remaining_cost(total_estimated_cost: f64, progress_fraction: f64) -> f64 {
    let clamped = progress_fraction.clamp(0.0, 1.0);
    total_estimated_cost * (1.0 - clamped)
}

/// Insert stitch points into a plan tree at join and aggregate
/// boundaries. Returns a new plan with `StitchMonitor` wrappers
/// and a mapping from monitor IDs to estimated cardinalities.
///
/// The returned plan is semantically equivalent to the input but
/// decorated with monitoring metadata.
pub fn insert_stitch_points(plan: &RelExpr) -> (RelExpr, Vec<StitchPointMeta>) {
    let mut metas = Vec::new();
    let annotated = insert_stitch_points_rec(plan, &mut metas);
    (annotated, metas)
}

/// Metadata for a stitch point inserted into the plan.
#[derive(Debug, Clone)]
pub struct StitchPointMeta {
    /// Unique identifier for this stitch point.
    pub id: usize,
    /// Kind of stitch point.
    pub kind: StitchPointKind,
    /// Estimated cardinality at this point in the plan.
    pub estimated_cardinality: u64,
    /// Descriptive label for the operator.
    pub operator_label: String,
}

fn insert_stitch_points_rec(plan: &RelExpr, metas: &mut Vec<StitchPointMeta>) -> RelExpr {
    match plan {
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let left_annotated = insert_stitch_points_rec(left, metas);
            let right_annotated = insert_stitch_points_rec(right, metas);

            let id = metas.len();
            metas.push(StitchPointMeta {
                id,
                kind: StitchPointKind::JoinBuildComplete,
                estimated_cardinality: 0,
                operator_label: format!("join_{id}"),
            });

            RelExpr::Join {
                join_type: *join_type,
                condition: condition.clone(),
                left: Box::new(left_annotated),
                right: Box::new(right_annotated),
            }
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            let input_annotated = insert_stitch_points_rec(input, metas);

            let id = metas.len();
            metas.push(StitchPointMeta {
                id,
                kind: StitchPointKind::AggregateInput,
                estimated_cardinality: 0,
                operator_label: format!("agg_{id}"),
            });

            RelExpr::Aggregate {
                group_by: group_by.clone(),
                aggregates: aggregates.clone(),
                input: Box::new(input_annotated),
            }
        }
        RelExpr::Sort { keys, input } => {
            let input_annotated = insert_stitch_points_rec(input, metas);

            let id = metas.len();
            metas.push(StitchPointMeta {
                id,
                kind: StitchPointKind::SortInput,
                estimated_cardinality: 0,
                operator_label: format!("sort_{id}"),
            });

            RelExpr::Sort {
                keys: keys.clone(),
                input: Box::new(input_annotated),
            }
        }
        RelExpr::Filter { predicate, input } => {
            let input_annotated = insert_stitch_points_rec(input, metas);
            RelExpr::Filter {
                predicate: predicate.clone(),
                input: Box::new(input_annotated),
            }
        }
        RelExpr::Project { columns, input } => {
            let input_annotated = insert_stitch_points_rec(input, metas);
            RelExpr::Project {
                columns: columns.clone(),
                input: Box::new(input_annotated),
            }
        }
        other => other.clone(),
    }
}

/// Full re-optimization decision context. Gathers all the
/// information needed to decide whether to switch plans.
#[derive(Debug, Clone)]
pub struct ReoptDecision {
    /// Whether to re-optimize.
    pub should_switch: bool,
    /// Divergence factor that triggered the evaluation.
    pub divergence_factor: f64,
    /// Remaining cost of the current plan.
    pub remaining_current_cost: f64,
    /// Cost of the alternative plan (including stitch overhead).
    pub alternative_total_cost: f64,
    /// Cost savings as a fraction (0.0 to 1.0).
    pub savings_fraction: f64,
}

/// Evaluate whether re-optimization is warranted given a divergence
/// observation and cost estimates.
#[must_use]
pub fn evaluate_reopt_decision(
    estimated: u64,
    actual: u64,
    remaining_current_cost: f64,
    alternative_plan_cost: f64,
    stitch_overhead: f64,
    config: &ReoptConfig,
) -> ReoptDecision {
    let factor = divergence_factor(estimated, actual);
    let triggers = should_reoptimize(estimated, actual, config.divergence_threshold);

    if !triggers {
        return ReoptDecision {
            should_switch: false,
            divergence_factor: factor,
            remaining_current_cost,
            alternative_total_cost: alternative_plan_cost + stitch_overhead,
            savings_fraction: 0.0,
        };
    }

    let total_alt = alternative_plan_cost + stitch_overhead;
    let savings = if remaining_current_cost > 0.0 {
        1.0 - (total_alt / remaining_current_cost)
    } else {
        0.0
    };

    let switch = is_switch_worthwhile(
        remaining_current_cost,
        alternative_plan_cost,
        stitch_overhead,
        config.switch_threshold,
    );

    ReoptDecision {
        should_switch: switch,
        divergence_factor: factor,
        remaining_current_cost,
        alternative_total_cost: total_alt,
        savings_fraction: savings.max(0.0),
    }
}

// -----------------------------------------------------------------
// Background re-optimization
// -----------------------------------------------------------------

/// A function that re-optimizes a plan given corrected statistics.
/// The optimizer is abstracted behind this trait so the background
/// thread does not depend on a concrete `Optimizer` type.
pub trait ReoptimizeFn: Send + 'static {
    /// Re-optimize `plan` using the corrected `stats` and return
    /// the improved plan.
    fn reoptimize(
        &self,
        plan: &RelExpr,
        stats: &HashMap<String, Statistics>,
    ) -> Result<RelExpr, ReoptError>;
}

/// Errors from background re-optimization.
#[derive(Debug, Clone)]
pub enum ReoptError {
    /// The optimizer failed internally.
    OptimizerFailed(String),
    /// The background thread was cancelled.
    Cancelled,
}

impl std::fmt::Display for ReoptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OptimizerFailed(msg) => write!(f, "optimizer failed: {msg}"),
            Self::Cancelled => write!(f, "reoptimization cancelled"),
        }
    }
}

impl std::error::Error for ReoptError {}

/// Result sent from the background thread when re-optimization
/// completes.
#[derive(Debug, Clone)]
pub struct ReoptResult {
    /// The improved plan (or `None` if no improvement was found).
    pub improved_plan: Option<RelExpr>,
    /// Number of reoptimization attempts made.
    pub attempts: usize,
    /// Whether the background thread completed normally.
    pub completed: bool,
}

/// Handle to a running background re-optimization thread.
pub struct BackgroundReoptimizer {
    /// Receive improved plans from the background thread.
    receiver: mpsc::Receiver<ReoptResult>,
    /// Signal the background thread to stop.
    cancel: Arc<Mutex<bool>>,
    /// Join handle for the worker thread.
    handle: Option<thread::JoinHandle<()>>,
}

impl BackgroundReoptimizer {
    /// Spawn a background re-optimization thread.
    ///
    /// `quick_plan` is the initial plan returned to the executor.
    /// `corrected_stats` are runtime-observed statistics.
    /// `optimizer_fn` performs the actual re-optimization.
    /// `config` controls divergence thresholds and max attempts.
    pub fn spawn(
        quick_plan: RelExpr,
        corrected_stats: HashMap<String, Statistics>,
        optimizer_fn: Box<dyn ReoptimizeFn>,
        config: ReoptConfig,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(Mutex::new(false));
        let cancel_flag = Arc::clone(&cancel);

        let handle = thread::spawn(move || {
            background_worker(
                quick_plan,
                corrected_stats,
                optimizer_fn,
                config,
                tx,
                cancel_flag,
            );
        });

        Self {
            receiver: rx,
            cancel,
            handle: Some(handle),
        }
    }

    /// Try to receive a completed re-optimization result without
    /// blocking. Returns `None` if the background thread hasn't
    /// finished yet.
    #[must_use]
    pub fn try_recv(&self) -> Option<ReoptResult> {
        self.receiver.try_recv().ok()
    }

    /// Block until the background thread produces a result.
    /// Returns `None` if the channel was disconnected.
    #[must_use]
    pub fn recv(&self) -> Option<ReoptResult> {
        self.receiver.recv().ok()
    }

    /// Signal the background thread to stop and wait for it to
    /// finish.
    pub fn cancel_and_join(&mut self) {
        if let Ok(mut flag) = self.cancel.lock() {
            *flag = true;
        }
        if let Some(handle) = self.handle.take() {
            // Intentionally ignore join errors from panicked threads.
            let _result = handle.join();
        }
    }

    /// Check whether the background thread is still running.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .map_or(true, thread::JoinHandle::is_finished)
    }
}

impl Drop for BackgroundReoptimizer {
    fn drop(&mut self) {
        self.cancel_and_join();
    }
}

/// The background worker loop. Runs up to `max_reoptimizations`
/// rounds, sending any improved plan back through `tx`.
fn background_worker(
    plan: RelExpr,
    stats: HashMap<String, Statistics>,
    optimizer_fn: Box<dyn ReoptimizeFn>,
    config: ReoptConfig,
    tx: mpsc::Sender<ReoptResult>,
    cancel: Arc<Mutex<bool>>,
) {
    let mut best_plan = plan;
    let mut attempts = 0_usize;
    let mut improved = false;

    for _ in 0..config.max_reoptimizations {
        if cancel.lock().map_or(false, |f| *f) {
            debug!("background reoptimization cancelled");
            let _send = tx.send(ReoptResult {
                improved_plan: None,
                attempts,
                completed: false,
            });
            return;
        }

        attempts += 1;
        match optimizer_fn.reoptimize(&best_plan, &stats) {
            Ok(new_plan) => {
                if new_plan != best_plan {
                    debug!(
                        "background reopt attempt {attempts}: \
                         found improved plan"
                    );
                    best_plan = new_plan;
                    improved = true;
                } else {
                    debug!(
                        "background reopt attempt {attempts}: \
                         no improvement, stopping"
                    );
                    break;
                }
            }
            Err(e) => {
                warn!("background reopt attempt {attempts} failed: {e}");
                break;
            }
        }
    }

    let result = ReoptResult {
        improved_plan: if improved { Some(best_plan) } else { None },
        attempts,
        completed: true,
    };
    let _send = tx.send(result);
}

/// Convenience: produce a quick plan immediately and start
/// background re-optimization. Returns `(quick_plan, handle)`.
///
/// The quick plan is the input plan unchanged (representing the
/// initial fast path). The handle allows polling for an improved
/// plan from the background thread.
pub fn progressive_optimize(
    plan: RelExpr,
    corrected_stats: HashMap<String, Statistics>,
    optimizer_fn: Box<dyn ReoptimizeFn>,
    config: ReoptConfig,
) -> (RelExpr, BackgroundReoptimizer) {
    let quick_plan = plan.clone();
    let handle = BackgroundReoptimizer::spawn(plan, corrected_stats, optimizer_fn, config);
    (quick_plan, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    #[test]
    fn should_reoptimize_both_zero() {
        assert!(!should_reoptimize(0, 0, 2.0));
    }

    #[test]
    fn should_reoptimize_estimated_zero_actual_nonzero() {
        assert!(should_reoptimize(0, 100, 2.0));
    }

    #[test]
    fn should_reoptimize_within_threshold() {
        assert!(!should_reoptimize(100, 150, 2.0));
    }

    #[test]
    fn should_reoptimize_exceeds_threshold_overestimate() {
        assert!(should_reoptimize(100, 300, 2.0));
    }

    #[test]
    fn should_reoptimize_exceeds_threshold_underestimate() {
        assert!(should_reoptimize(300, 100, 2.0));
    }

    #[test]
    fn should_reoptimize_exact_threshold_boundary() {
        assert!(!should_reoptimize(100, 200, 2.0));
    }

    #[test]
    fn should_reoptimize_just_above_threshold() {
        assert!(should_reoptimize(100, 201, 2.0));
    }

    #[test]
    fn divergence_factor_both_zero() {
        let f = divergence_factor(0, 0);
        assert!((f - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn divergence_factor_estimated_zero() {
        let f = divergence_factor(0, 100);
        assert_eq!(f, f64::MAX);
    }

    #[test]
    fn divergence_factor_normal() {
        let f = divergence_factor(100, 300);
        assert!((f - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn divergence_factor_underestimate() {
        let f = divergence_factor(200, 50);
        assert!((f - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_stitch_cost_copy() {
        let cost = estimate_stitch_cost(1000, StitchTransferKind::Copy);
        assert!((cost - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_stitch_cost_hash_build() {
        let cost = estimate_stitch_cost(1000, StitchTransferKind::HashBuild);
        assert!((cost - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_stitch_cost_sort() {
        let cost = estimate_stitch_cost(1000, StitchTransferKind::Sort);
        assert!((cost - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_stitch_cost_discard() {
        let cost = estimate_stitch_cost(1000, StitchTransferKind::Discard);
        assert!(cost.abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_stitch_cost_zero_rows() {
        let cost = estimate_stitch_cost(0, StitchTransferKind::Sort);
        assert!(cost.abs() < f64::EPSILON);
    }

    #[test]
    fn join_transfer_kind_hash_to_merge() {
        assert_eq!(
            join_transfer_kind(JoinImplKind::Hash, JoinImplKind::Merge),
            StitchTransferKind::Sort,
        );
    }

    #[test]
    fn join_transfer_kind_hash_to_nested_loop() {
        assert_eq!(
            join_transfer_kind(JoinImplKind::Hash, JoinImplKind::NestedLoop),
            StitchTransferKind::Discard,
        );
    }

    #[test]
    fn join_transfer_kind_nested_loop_to_hash() {
        assert_eq!(
            join_transfer_kind(JoinImplKind::NestedLoop, JoinImplKind::Hash),
            StitchTransferKind::HashBuild,
        );
    }

    #[test]
    fn join_transfer_kind_same_impl() {
        assert_eq!(
            join_transfer_kind(JoinImplKind::Hash, JoinImplKind::Hash),
            StitchTransferKind::Copy,
        );
        assert_eq!(
            join_transfer_kind(JoinImplKind::Merge, JoinImplKind::Merge),
            StitchTransferKind::Copy,
        );
    }

    #[test]
    fn join_transfer_kind_merge_to_hash() {
        assert_eq!(
            join_transfer_kind(JoinImplKind::Merge, JoinImplKind::Hash),
            StitchTransferKind::HashBuild,
        );
    }

    #[test]
    fn join_transfer_kind_merge_to_nested_loop() {
        assert_eq!(
            join_transfer_kind(JoinImplKind::Merge, JoinImplKind::NestedLoop),
            StitchTransferKind::Discard,
        );
    }

    #[test]
    fn is_switch_worthwhile_yes() {
        assert!(is_switch_worthwhile(100.0, 50.0, 10.0, 0.8));
    }

    #[test]
    fn is_switch_worthwhile_no_too_expensive() {
        assert!(!is_switch_worthwhile(100.0, 90.0, 10.0, 0.8));
    }

    #[test]
    fn is_switch_worthwhile_zero_remaining() {
        assert!(!is_switch_worthwhile(0.0, 10.0, 5.0, 0.8));
    }

    #[test]
    fn estimate_remaining_cost_full() {
        let r = estimate_remaining_cost(100.0, 0.0);
        assert!((r - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_remaining_cost_half_done() {
        let r = estimate_remaining_cost(100.0, 0.5);
        assert!((r - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_remaining_cost_complete() {
        let r = estimate_remaining_cost(100.0, 1.0);
        assert!(r.abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_remaining_cost_clamps_below_zero() {
        let r = estimate_remaining_cost(100.0, -0.5);
        assert!((r - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_remaining_cost_clamps_above_one() {
        let r = estimate_remaining_cost(100.0, 1.5);
        assert!(r.abs() < f64::EPSILON);
    }

    #[test]
    fn reopt_config_default() {
        let cfg = ReoptConfig::default();
        assert!((cfg.divergence_threshold - DIVERGENCE_THRESHOLD).abs() < f64::EPSILON);
        assert!((cfg.switch_threshold - SWITCH_THRESHOLD).abs() < f64::EPSILON);
        assert_eq!(cfg.max_reoptimizations, 3);
    }

    #[test]
    fn evaluate_reopt_decision_no_divergence() {
        let cfg = ReoptConfig::default();
        let decision = evaluate_reopt_decision(100, 150, 50.0, 30.0, 5.0, &cfg);
        assert!(!decision.should_switch);
        assert!(decision.savings_fraction.abs() < f64::EPSILON);
    }

    #[test]
    fn evaluate_reopt_decision_divergence_and_switch() {
        let cfg = ReoptConfig::default();
        let decision = evaluate_reopt_decision(100, 500, 100.0, 30.0, 5.0, &cfg);
        assert!(decision.should_switch);
        assert!(decision.savings_fraction > 0.0);
    }

    #[test]
    fn evaluate_reopt_decision_divergence_but_no_savings() {
        let cfg = ReoptConfig::default();
        let decision = evaluate_reopt_decision(100, 500, 100.0, 95.0, 10.0, &cfg);
        assert!(!decision.should_switch);
    }

    #[test]
    fn evaluate_reopt_decision_zero_remaining_cost() {
        let cfg = ReoptConfig::default();
        let decision = evaluate_reopt_decision(100, 500, 0.0, 10.0, 5.0, &cfg);
        assert!(!decision.should_switch);
        assert!(decision.savings_fraction.abs() < f64::EPSILON);
    }

    fn make_join_plan() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        }
    }

    #[test]
    fn insert_stitch_points_on_join() {
        let plan = make_join_plan();
        let (_annotated, metas) = insert_stitch_points(&plan);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].kind, StitchPointKind::JoinBuildComplete);
    }

    #[test]
    fn insert_stitch_points_on_aggregate() {
        let plan = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        let (_annotated, metas) = insert_stitch_points(&plan);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].kind, StitchPointKind::AggregateInput);
    }

    #[test]
    fn insert_stitch_points_on_sort() {
        let plan = RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        let (_annotated, metas) = insert_stitch_points(&plan);
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].kind, StitchPointKind::SortInput);
    }

    #[test]
    fn insert_stitch_points_nested() {
        let plan = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(make_join_plan()),
        };
        let (_annotated, metas) = insert_stitch_points(&plan);
        assert_eq!(metas.len(), 2);
    }

    #[test]
    fn insert_stitch_points_filter_passes_through() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(ra_core::expr::Const::Bool(true)),
            input: Box::new(make_join_plan()),
        };
        let (_annotated, metas) = insert_stitch_points(&plan);
        assert_eq!(metas.len(), 1);
    }

    #[test]
    fn insert_stitch_points_leaf_node() {
        let plan = RelExpr::scan("t");
        let (annotated, metas) = insert_stitch_points(&plan);
        assert!(metas.is_empty());
        assert_eq!(annotated, plan);
    }

    #[test]
    fn runtime_statistics_default() {
        let stats = RuntimeStatistics::default();
        assert!(stats.actual_row_counts.is_empty());
        assert!(stats.corrected_table_stats.is_empty());
    }

    #[test]
    fn reopt_error_display() {
        let e = ReoptError::OptimizerFailed("bad plan".to_string());
        assert_eq!(e.to_string(), "optimizer failed: bad plan");

        let e2 = ReoptError::Cancelled;
        assert_eq!(e2.to_string(), "reoptimization cancelled");
    }

    struct NoopOptimizer;
    impl ReoptimizeFn for NoopOptimizer {
        fn reoptimize(
            &self,
            plan: &RelExpr,
            _stats: &HashMap<String, Statistics>,
        ) -> Result<RelExpr, ReoptError> {
            Ok(plan.clone())
        }
    }

    struct ImprovingOptimizer;
    impl ReoptimizeFn for ImprovingOptimizer {
        fn reoptimize(
            &self,
            _plan: &RelExpr,
            _stats: &HashMap<String, Statistics>,
        ) -> Result<RelExpr, ReoptError> {
            Ok(RelExpr::scan("optimized"))
        }
    }

    struct FailingOptimizer;
    impl ReoptimizeFn for FailingOptimizer {
        fn reoptimize(
            &self,
            _plan: &RelExpr,
            _stats: &HashMap<String, Statistics>,
        ) -> Result<RelExpr, ReoptError> {
            Err(ReoptError::OptimizerFailed("fail".into()))
        }
    }

    #[test]
    fn background_reoptimizer_no_improvement() {
        let plan = RelExpr::scan("t");
        let handle = BackgroundReoptimizer::spawn(
            plan,
            HashMap::new(),
            Box::new(NoopOptimizer),
            ReoptConfig::default(),
        );
        let result = handle.recv();
        assert!(result.is_some());
        let r = result.expect("recv returned None");
        assert!(r.improved_plan.is_none());
        assert!(r.completed);
        assert_eq!(r.attempts, 1);
    }

    #[test]
    fn background_reoptimizer_with_improvement() {
        let plan = RelExpr::scan("t");
        let handle = BackgroundReoptimizer::spawn(
            plan,
            HashMap::new(),
            Box::new(ImprovingOptimizer),
            ReoptConfig {
                max_reoptimizations: 1,
                ..ReoptConfig::default()
            },
        );
        let result = handle.recv();
        assert!(result.is_some());
        let r = result.expect("recv returned None");
        assert!(r.improved_plan.is_some());
        assert_eq!(
            r.improved_plan.expect("no improved plan"),
            RelExpr::scan("optimized")
        );
        assert!(r.completed);
    }

    #[test]
    fn background_reoptimizer_failure() {
        let plan = RelExpr::scan("t");
        let handle = BackgroundReoptimizer::spawn(
            plan,
            HashMap::new(),
            Box::new(FailingOptimizer),
            ReoptConfig::default(),
        );
        let result = handle.recv();
        assert!(result.is_some());
        let r = result.expect("recv returned None");
        assert!(r.improved_plan.is_none());
        assert!(r.completed);
        assert_eq!(r.attempts, 1);
    }

    #[test]
    fn background_reoptimizer_cancel() {
        let plan = RelExpr::scan("t");
        let mut handle = BackgroundReoptimizer::spawn(
            plan,
            HashMap::new(),
            Box::new(NoopOptimizer),
            ReoptConfig::default(),
        );
        handle.cancel_and_join();
        assert!(handle.is_finished());
    }

    #[test]
    fn progressive_optimize_returns_quick_plan() {
        let plan = RelExpr::scan("t");
        let (quick, handle) = progressive_optimize(
            plan.clone(),
            HashMap::new(),
            Box::new(NoopOptimizer),
            ReoptConfig::default(),
        );
        assert_eq!(quick, plan);
        let result = handle.recv();
        assert!(result.is_some());
    }
}
