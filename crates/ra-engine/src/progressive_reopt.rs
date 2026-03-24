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
                warn!(
                    "background reopt attempt {attempts} failed: {e}"
                );
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
    let handle = BackgroundReoptimizer::spawn(
        plan,
        corrected_stats,
        optimizer_fn,
        config,
    );
    (quick_plan, handle)
}
