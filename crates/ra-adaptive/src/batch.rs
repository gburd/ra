//! Batch-mode adaptive execution with feedback loops.
//!
//! Processes queries in configurable batches, collecting execution
//! feedback (actual vs estimated cardinalities) after each batch and
//! applying corrections to the statistics state. This drives
//! reoptimization when the optimizer's model drifts from reality.
//!
//! Three feedback modes control how aggressively statistics are
//! updated between batches:
//!
//! - [`FeedbackMode::ConfidenceOnly`]: Adjust confidence scores
//!   without changing row counts or histograms.
//! - [`FeedbackMode::IncrementalStats`]: Update confidence and
//!   apply incremental corrections to row counts.
//! - [`FeedbackMode::FullReanalyze`]: Replace statistics entirely
//!   with the latest snapshot (simulates a full ANALYZE).

use std::collections::HashMap;

use ra_core::algebra::RelExpr;
use ra_stats::accuracy::{StatisticsSource, StatisticsState};
use ra_stats::integration::{ManagedTableStats, StatisticsAdapter};
use ra_stats::profiles::StatisticsProfile;
use ra_stats::timeline::{ExecutionFeedback, Snapshot, TimelinePlayer};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::executor::{AdaptiveConfig, AdaptiveExecutor, ExecutionReport};
use crate::plan_switch::JoinStrategy;
use crate::runtime_stats::NodeId;

/// Errors from batch execution.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    /// Timeline playback reached end before all batches completed.
    #[error("timeline exhausted after {batches_completed} batches")]
    TimelineExhausted {
        /// Number of batches completed before exhaustion.
        batches_completed: usize,
    },
    /// A batch execution failed.
    #[error("batch {batch_index} failed: {reason}")]
    ExecutionFailed {
        /// The batch that failed.
        batch_index: usize,
        /// What went wrong.
        reason: String,
    },
    /// Configuration error.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

/// How feedback is applied to the statistics state between batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeedbackMode {
    /// Only adjust confidence scores based on estimate accuracy.
    /// Row counts and histograms are unchanged.
    ConfidenceOnly,
    /// Adjust confidence and apply incremental corrections to
    /// row count estimates based on observed actuals.
    IncrementalStats,
    /// Replace statistics entirely with the current snapshot's
    /// values (equivalent to running ANALYZE).
    FullReanalyze,
}

impl std::fmt::Display for FeedbackMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::ConfidenceOnly => "confidence-only",
            Self::IncrementalStats => "incremental-stats",
            Self::FullReanalyze => "full-reanalyze",
        };
        write!(f, "{label}")
    }
}

/// Configuration for batch execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Number of timeline snapshots per batch.
    pub batch_size: usize,
    /// How feedback is applied between batches.
    pub feedback_mode: FeedbackMode,
    /// Q-error threshold that triggers reoptimization.
    /// When the Q-error of any operator exceeds this, the plan
    /// is reconsidered.
    pub reoptimize_threshold: f64,
    /// Minimum confidence before triggering reoptimization.
    pub min_confidence: f64,
    /// Maximum number of reoptimizations across all batches.
    pub max_reoptimizations: u32,
    /// Configuration for the adaptive executor used within batches.
    pub adaptive_config: AdaptiveConfig,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 1,
            feedback_mode: FeedbackMode::IncrementalStats,
            reoptimize_threshold: 3.0,
            min_confidence: 0.4,
            max_reoptimizations: 10,
            adaptive_config: AdaptiveConfig::default(),
        }
    }
}

/// Tracks a single operator's estimated vs actual cardinalities
/// within a batch for feedback computation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperatorFeedback {
    /// Optimizer's row estimate at batch start.
    pub estimated_rows: f64,
    /// Actual rows observed during execution.
    pub actual_rows: f64,
    /// Q-error: max(est/act, act/est).
    pub q_error: f64,
    /// Whether this feedback triggered a reoptimization.
    pub triggered_reoptimization: bool,
}

/// Record of a single batch execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRecord {
    /// Sequential batch number (0-indexed).
    pub batch_index: usize,
    /// Snapshot indices consumed by this batch.
    pub snapshot_range: (usize, usize),
    /// Per-operator feedback collected in this batch.
    pub operator_feedback: HashMap<NodeId, OperatorFeedback>,
    /// Whether the plan was reoptimized after this batch.
    pub reoptimized: bool,
    /// The plan used for this batch.
    pub plan: RelExpr,
    /// Confidence level at the start of this batch.
    pub confidence_before: f64,
    /// Confidence level after applying feedback.
    pub confidence_after: f64,
    /// Average Q-error across all operators in this batch.
    pub avg_q_error: f64,
    /// Maximum Q-error across all operators.
    pub max_q_error: f64,
    /// The execution report from the adaptive executor.
    pub execution_report: ExecutionReport,
}

/// Result of the complete batch execution run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRunResult {
    /// All batch records in order.
    pub batches: Vec<BatchRecord>,
    /// Total number of reoptimizations.
    pub total_reoptimizations: u32,
    /// Final plan after all batches.
    pub final_plan: RelExpr,
    /// Configuration used.
    pub config: BatchConfig,
    /// Whether the run completed all available snapshots.
    pub completed: bool,
    /// Feedback mode used.
    pub feedback_mode: FeedbackMode,
}

impl BatchRunResult {
    /// Average Q-error across all batches.
    #[must_use]
    pub fn overall_avg_q_error(&self) -> f64 {
        if self.batches.is_empty() {
            return 1.0;
        }
        let sum: f64 = self.batches.iter().map(|b| b.avg_q_error).sum();
        sum / self.batches.len() as f64
    }

    /// Whether Q-error improved over the course of the run.
    #[must_use]
    pub fn q_error_improved(&self) -> bool {
        if self.batches.len() < 2 {
            return false;
        }
        let first = self.batches.first().map_or(1.0, |b| b.avg_q_error);
        let last = self.batches.last().map_or(1.0, |b| b.avg_q_error);
        last < first
    }

    /// Confidence trend: final minus initial confidence.
    #[must_use]
    pub fn confidence_delta(&self) -> f64 {
        let first = self.batches.first().map_or(1.0, |b| b.confidence_before);
        let last = self.batches.last().map_or(1.0, |b| b.confidence_after);
        last - first
    }
}

/// Orchestrates batch execution with feedback loops.
///
/// Processes a statistics timeline in batches, using an
/// [`AdaptiveExecutor`] within each batch. After each batch,
/// collects execution feedback and applies it to the statistics
/// state according to the configured [`FeedbackMode`].
pub struct BatchExecutor {
    config: BatchConfig,
    adapter: StatisticsAdapter,
    current_plan: RelExpr,
    batch_history: Vec<BatchRecord>,
    reoptimization_count: u32,
    table_names: Vec<String>,
}

impl std::fmt::Debug for BatchExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchExecutor")
            .field("config", &self.config)
            .field("batches_completed", &self.batch_history.len())
            .field("reoptimization_count", &self.reoptimization_count)
            .field("table_names", &self.table_names)
            .finish_non_exhaustive()
    }
}

impl BatchExecutor {
    /// Create a new batch executor.
    ///
    /// # Errors
    ///
    /// Returns `BatchError::InvalidConfig` if the batch size is 0.
    pub fn new(
        plan: RelExpr,
        config: BatchConfig,
        profile: StatisticsProfile,
    ) -> Result<Self, BatchError> {
        if config.batch_size == 0 {
            return Err(BatchError::InvalidConfig("batch_size must be > 0".into()));
        }
        Ok(Self {
            config,
            adapter: StatisticsAdapter::new(profile),
            current_plan: plan,
            batch_history: Vec::new(),
            reoptimization_count: 0,
            table_names: Vec::new(),
        })
    }

    /// Create with default configuration.
    #[must_use]
    pub fn with_defaults(plan: RelExpr) -> Self {
        Self {
            config: BatchConfig::default(),
            adapter: StatisticsAdapter::new(StatisticsProfile::standard()),
            current_plan: plan,
            batch_history: Vec::new(),
            reoptimization_count: 0,
            table_names: Vec::new(),
        }
    }

    /// Run batch execution over a timeline player.
    ///
    /// Steps through the timeline's snapshots in batches of
    /// `config.batch_size`, executing the plan and collecting
    /// feedback after each batch.
    ///
    /// # Errors
    ///
    /// Returns `BatchError` if execution fails.
    pub fn run(&mut self, player: &mut TimelinePlayer) -> Result<BatchRunResult, BatchError> {
        player.seek_start();
        let total_snapshots = player.snapshot_count();
        let mut snapshot_idx = 0;
        let mut batch_index = 0;

        while snapshot_idx < total_snapshots {
            let batch_end = (snapshot_idx + self.config.batch_size).min(total_snapshots);

            let record = self.execute_batch(player, batch_index, snapshot_idx, batch_end)?;

            self.batch_history.push(record);
            snapshot_idx = batch_end;
            batch_index += 1;
        }

        let completed = snapshot_idx >= total_snapshots;

        Ok(BatchRunResult {
            batches: self.batch_history.clone(),
            total_reoptimizations: self.reoptimization_count,
            final_plan: self.current_plan.clone(),
            config: self.config.clone(),
            completed,
            feedback_mode: self.config.feedback_mode,
        })
    }

    /// Execute a single batch covering snapshots [start, end).
    fn execute_batch(
        &mut self,
        player: &mut TimelinePlayer,
        batch_index: usize,
        snap_start: usize,
        snap_end: usize,
    ) -> Result<BatchRecord, BatchError> {
        // Load statistics from the last snapshot in this batch
        let target_snap = snap_end.saturating_sub(1);
        player
            .seek(target_snap)
            .map_err(|e| BatchError::ExecutionFailed {
                batch_index,
                reason: format!("seek failed: {e}"),
            })?;

        let managed_stats =
            player
                .current_managed_stats()
                .ok_or_else(|| BatchError::ExecutionFailed {
                    batch_index,
                    reason: "no stats at snapshot".into(),
                })?;

        // Update adapter with snapshot statistics
        for (table, stats) in &managed_stats {
            self.adapter.add_table(table.clone(), stats.clone());
            if !self.table_names.contains(table) {
                self.table_names.push(table.clone());
            }
        }

        // Compute confidence before this batch
        let confidence_before = self.average_confidence();

        // Set up adaptive executor for this batch
        let mut executor = AdaptiveExecutor::with_config(
            self.current_plan.clone(),
            self.config.adaptive_config.clone(),
        );

        let estimates = self.register_operators(&managed_stats, &mut executor);

        // Collect execution feedback from timeline and
        // synthetic sources
        let mut operator_feedback: HashMap<NodeId, OperatorFeedback> = HashMap::new();
        Self::collect_timeline_feedback(
            player,
            &managed_stats,
            &estimates,
            &mut executor,
            &mut operator_feedback,
        );
        Self::collect_synthetic_feedback(player, &estimates, &mut executor, &mut operator_feedback);

        let report = executor.report();

        // Compute Q-error summary
        let (avg_q, max_q) = compute_q_error_summary(&operator_feedback);

        // Apply feedback to statistics state
        self.apply_feedback(&managed_stats, &operator_feedback);

        let confidence_after = self.average_confidence();

        // Decide whether to reoptimize
        let should_reoptimize = self.should_reoptimize(max_q, confidence_after);

        let reoptimized = if should_reoptimize {
            self.reoptimize(&report);
            // Mark which operator triggered
            for fb in operator_feedback.values_mut() {
                if fb.q_error > self.config.reoptimize_threshold {
                    fb.triggered_reoptimization = true;
                }
            }
            true
        } else {
            false
        };

        info!(
            batch_index,
            snap_start,
            snap_end,
            avg_q_error = avg_q,
            max_q_error = max_q,
            reoptimized,
            "batch completed"
        );

        Ok(BatchRecord {
            batch_index,
            snapshot_range: (snap_start, snap_end),
            operator_feedback,
            reoptimized,
            plan: self.current_plan.clone(),
            confidence_before,
            confidence_after,
            avg_q_error: avg_q,
            max_q_error: max_q,
            execution_report: report,
        })
    }

    /// Register operators with the executor and return estimates.
    fn register_operators(
        &self,
        managed_stats: &HashMap<String, ManagedTableStats>,
        executor: &mut AdaptiveExecutor,
    ) -> HashMap<NodeId, f64> {
        let mut estimates = HashMap::new();
        let is_join = Self::is_join_node(&self.current_plan);

        for (i, (table_name, stats)) in managed_stats.iter().enumerate() {
            let node_id: NodeId = (i + 1) as NodeId;
            let estimated_rows = stats.table.row_count as f64;
            estimates.insert(node_id, estimated_rows);

            if is_join {
                executor.register_join(node_id, estimated_rows, JoinStrategy::HashJoin);
            } else {
                executor.register_operator(node_id, estimated_rows);
            }

            let core_stats = self.adapter.to_core_statistics(stats);
            executor.add_table_stats(table_name, core_stats);
        }
        estimates
    }

    /// Collect feedback from timeline entries and report to executor.
    fn collect_timeline_feedback(
        player: &TimelinePlayer,
        managed_stats: &HashMap<String, ManagedTableStats>,
        estimates: &HashMap<NodeId, f64>,
        executor: &mut AdaptiveExecutor,
        operator_feedback: &mut HashMap<NodeId, OperatorFeedback>,
    ) {
        let feedback_entries = player.feedback_at_current();
        for fb in &feedback_entries {
            let fb_node_id = Self::match_feedback_to_node(fb, managed_stats);
            let estimated = estimates
                .get(&fb_node_id)
                .copied()
                .unwrap_or(fb.estimated_rows);

            let actual = fb.actual_rows.max(0.0) as u64;
            executor.report_rows(fb_node_id, actual);
            executor.report_completed(fb_node_id, actual);

            operator_feedback.insert(
                fb_node_id,
                OperatorFeedback {
                    estimated_rows: estimated,
                    actual_rows: fb.actual_rows,
                    q_error: fb.q_error(),
                    triggered_reoptimization: false,
                },
            );
        }
    }

    /// Generate synthetic feedback for nodes without timeline entries.
    fn collect_synthetic_feedback(
        player: &TimelinePlayer,
        estimates: &HashMap<NodeId, f64>,
        executor: &mut AdaptiveExecutor,
        operator_feedback: &mut HashMap<NodeId, OperatorFeedback>,
    ) {
        for (&nid, &est) in estimates {
            if operator_feedback.contains_key(&nid) {
                continue;
            }
            let Some(snap) = player.current_snapshot() else {
                continue;
            };
            let actual = Self::find_actual_for_node(nid, snap, estimates);
            let q_err = compute_q_error(est, actual);
            operator_feedback.insert(
                nid,
                OperatorFeedback {
                    estimated_rows: est,
                    actual_rows: actual,
                    q_error: q_err,
                    triggered_reoptimization: false,
                },
            );
            let actual_u64 = actual.max(0.0) as u64;
            executor.report_completed(nid, actual_u64);
        }
    }

    /// Apply feedback to statistics state based on the mode.
    fn apply_feedback(
        &mut self,
        managed_stats: &HashMap<String, ManagedTableStats>,
        operator_feedback: &HashMap<NodeId, OperatorFeedback>,
    ) {
        match self.config.feedback_mode {
            FeedbackMode::ConfidenceOnly => {
                self.apply_confidence_feedback(operator_feedback);
            }
            FeedbackMode::IncrementalStats => {
                self.apply_incremental_feedback(managed_stats, operator_feedback);
            }
            FeedbackMode::FullReanalyze => {
                self.apply_full_reanalyze(managed_stats);
            }
        }
    }

    /// Adjust confidence based on estimate accuracy.
    fn apply_confidence_feedback(&mut self, operator_feedback: &HashMap<NodeId, OperatorFeedback>) {
        let avg_q = if operator_feedback.is_empty() {
            1.0
        } else {
            let sum: f64 = operator_feedback.values().map(|f| f.q_error).sum();
            sum / operator_feedback.len() as f64
        };

        // Decrease confidence proportionally to Q-error
        let decay = confidence_decay_from_q_error(avg_q);

        // Apply to all tables
        let table_names: Vec<String> = self.adapter_table_names().to_vec();
        for name in &table_names {
            if let Some(stats) = self.adapter.get_table_stats(name) {
                let mut updated = stats.clone();
                updated.state.confidence = (updated.state.confidence * decay).max(0.05);
                self.adapter.add_table(name.clone(), updated);

                debug!(
                    table = name.as_str(),
                    new_confidence = self
                        .adapter
                        .get_table_stats(name)
                        .map_or(0.0, |s| s.state.confidence),
                    "confidence updated"
                );
            }
        }
    }

    /// Adjust confidence and apply incremental row count corrections.
    fn apply_incremental_feedback(
        &mut self,
        managed_stats: &HashMap<String, ManagedTableStats>,
        operator_feedback: &HashMap<NodeId, OperatorFeedback>,
    ) {
        // First apply confidence decay
        self.apply_confidence_feedback(operator_feedback);

        // Then adjust row counts based on feedback
        for fb in operator_feedback.values() {
            if fb.estimated_rows.abs() < f64::EPSILON {
                continue;
            }
            let correction = fb.actual_rows / fb.estimated_rows;

            // Apply correction to matching tables
            for (name, stats) in managed_stats {
                let table_rows = stats.table.row_count as f64;
                if (table_rows - fb.estimated_rows).abs() / table_rows.max(1.0) < 0.5 {
                    if let Some(current) = self.adapter.get_table_stats(name) {
                        let mut updated = current.clone();
                        // Blend: 70% old + 30% corrected
                        let corrected = table_rows * correction;
                        let blended = table_rows * 0.7 + corrected * 0.3;
                        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "legacy allow")]
                        let new_rows = blended.max(1.0) as u64;
                        updated.table.row_count = new_rows;
                        // Record the modification for staleness
                        let delta = (new_rows as i64 - stats.table.row_count as i64).unsigned_abs();
                        updated.state.record_modifications(delta);

                        debug!(
                            table = name.as_str(),
                            old_rows = stats.table.row_count,
                            new_rows,
                            correction,
                            "incremental stats update"
                        );
                        self.adapter.add_table(name.clone(), updated);
                    }
                }
            }
        }
    }

    /// Replace statistics entirely with fresh snapshot data.
    fn apply_full_reanalyze(&mut self, managed_stats: &HashMap<String, ManagedTableStats>) {
        for (name, stats) in managed_stats {
            let fresh = ManagedTableStats {
                table: stats.table.clone(),
                columns: stats.columns.clone(),
                state: StatisticsState::new(StatisticsSource::ExactCount, stats.table.row_count),
            };
            self.adapter.add_table(name.clone(), fresh);
            debug!(
                table = name.as_str(),
                rows = stats.table.row_count,
                "full reanalyze applied"
            );
        }
    }

    /// Whether reoptimization should occur.
    fn should_reoptimize(&self, max_q_error: f64, confidence: f64) -> bool {
        if self.reoptimization_count >= self.config.max_reoptimizations {
            return false;
        }
        max_q_error > self.config.reoptimize_threshold || confidence < self.config.min_confidence
    }

    /// Perform reoptimization by recording the decision.
    fn reoptimize(&mut self, _report: &ExecutionReport) {
        self.reoptimization_count += 1;
        info!(
            count = self.reoptimization_count,
            "reoptimization triggered"
        );
        // In a real system, we'd re-run the optimizer here.
        // For the demo, we keep the current plan but note the
        // reoptimization decision.
    }

    /// Average confidence across all registered tables.
    fn average_confidence(&self) -> f64 {
        let names = self.adapter_table_names();
        if names.is_empty() {
            return 1.0;
        }
        let sum: f64 = names
            .iter()
            .filter_map(|n| self.adapter.get_table_stats(n).map(|s| s.state.confidence))
            .sum();
        sum / names.len() as f64
    }

    /// Get all table names registered with the adapter.
    fn adapter_table_names(&self) -> &[String] {
        &self.table_names
    }

    /// Match a feedback entry to a node ID based on table name.
    fn match_feedback_to_node(
        fb: &ExecutionFeedback,
        managed_stats: &HashMap<String, ManagedTableStats>,
    ) -> NodeId {
        // Map feedback to node by matching table name in operator
        if let Some(operator) = &fb.operator {
            for (i, name) in managed_stats.keys().enumerate() {
                let nid: NodeId = (i + 1) as NodeId;
                if operator.contains(name.as_str()) {
                    return nid;
                }
            }
        }
        1 // Default to first node
    }

    /// Find the actual row count for a node from the snapshot.
    fn find_actual_for_node(
        node_id: NodeId,
        snapshot: &Snapshot,
        estimates: &HashMap<NodeId, f64>,
    ) -> f64 {
        // Map node IDs back to table order
        let idx = (node_id as usize).saturating_sub(1);
        if idx < snapshot.tables.len() {
            snapshot.tables[idx].row_count as f64
        } else {
            estimates.get(&node_id).copied().unwrap_or(1000.0)
        }
    }

    /// Whether the given plan node is a join.
    fn is_join_node(plan: &RelExpr) -> bool {
        matches!(plan, RelExpr::Join { .. })
    }

    /// Current plan.
    #[must_use]
    pub fn current_plan(&self) -> &RelExpr {
        &self.current_plan
    }

    /// Batch history.
    #[must_use]
    pub fn batch_history(&self) -> &[BatchRecord] {
        &self.batch_history
    }

    /// Number of reoptimizations so far.
    #[must_use]
    pub fn reoptimization_count(&self) -> u32 {
        self.reoptimization_count
    }

    /// The configuration.
    #[must_use]
    pub fn config(&self) -> &BatchConfig {
        &self.config
    }

    /// Access the statistics adapter.
    #[must_use]
    pub fn adapter(&self) -> &StatisticsAdapter {
        &self.adapter
    }

    /// Reset the executor for a new run with the same config.
    pub fn reset(&mut self, plan: RelExpr) {
        self.current_plan = plan;
        self.batch_history.clear();
        self.reoptimization_count = 0;
        self.table_names.clear();
    }
}

/// Compute Q-error: max(est/act, act/est), minimum 1.0.
fn compute_q_error(estimated: f64, actual: f64) -> f64 {
    let est = estimated.max(1.0);
    let act = actual.max(1.0);
    (est / act).max(act / est)
}

/// Compute average and max Q-error from operator feedback.
fn compute_q_error_summary(feedback: &HashMap<NodeId, OperatorFeedback>) -> (f64, f64) {
    if feedback.is_empty() {
        return (1.0, 1.0);
    }
    let sum: f64 = feedback.values().map(|f| f.q_error).sum();
    let max = feedback
        .values()
        .map(|f| f.q_error)
        .reduce(f64::max)
        .unwrap_or(1.0);
    (sum / feedback.len() as f64, max)
}

/// Compute confidence decay factor from average Q-error.
/// Perfect estimates (Q=1.0) produce no decay; large errors
/// produce strong decay.
fn confidence_decay_from_q_error(avg_q_error: f64) -> f64 {
    // Q-error 1.0 -> decay factor 1.0 (no change)
    // Q-error 2.0 -> decay factor 0.9
    // Q-error 5.0 -> decay factor 0.7
    // Q-error 10.0 -> decay factor 0.5
    let q = avg_q_error.max(1.0);
    (1.0 / q.ln().max(0.1)).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Expr};
    use ra_stats::timeline::{
        ColumnSnapshot, Snapshot, TableSnapshot, Timeline, TimelineMetadata, TimelinePlayer,
    };

    // -- Test helpers --

    fn simple_plan() -> RelExpr {
        RelExpr::scan("orders")
    }

    fn join_plan() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("o_id"))),
                right: Box::new(Expr::Column(ColumnRef::new("l_orderkey"))),
            },
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("lineitem")),
        }
    }

    fn make_timeline(snapshots: Vec<Snapshot>) -> Timeline {
        Timeline {
            metadata: TimelineMetadata {
                name: "test".to_string(),
                description: "test timeline".to_string(),
                database: None,
                schema: None,
                scale_factor: None,
                duration_seconds: None,
            },
            snapshots,
            events: vec![],
            feedback: vec![],
        }
    }

    fn make_timeline_with_feedback(
        snapshots: Vec<Snapshot>,
        feedback: Vec<ExecutionFeedback>,
    ) -> Timeline {
        Timeline {
            metadata: TimelineMetadata {
                name: "test-fb".to_string(),
                description: "test with feedback".to_string(),
                database: None,
                schema: None,
                scale_factor: None,
                duration_seconds: None,
            },
            snapshots,
            events: vec![],
            feedback,
        }
    }

    fn make_snapshot(time: u64, table_name: &str, rows: u64) -> Snapshot {
        Snapshot {
            time_offset: time,
            label: None,
            tables: vec![TableSnapshot {
                name: table_name.to_string(),
                row_count: rows,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![],
            }],
        }
    }

    fn make_snapshot_with_columns(
        time: u64,
        table_name: &str,
        rows: u64,
        columns: Vec<ColumnSnapshot>,
    ) -> Snapshot {
        Snapshot {
            time_offset: time,
            label: None,
            tables: vec![TableSnapshot {
                name: table_name.to_string(),
                row_count: rows,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns,
            }],
        }
    }

    fn make_two_table_snapshot(
        time: u64,
        table1: &str,
        rows1: u64,
        table2: &str,
        rows2: u64,
    ) -> Snapshot {
        Snapshot {
            time_offset: time,
            label: None,
            tables: vec![
                TableSnapshot {
                    name: table1.to_string(),
                    row_count: rows1,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
                TableSnapshot {
                    name: table2.to_string(),
                    row_count: rows2,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
            ],
        }
    }

    fn default_player(snapshots: Vec<Snapshot>) -> TimelinePlayer {
        let tl = make_timeline(snapshots);
        TimelinePlayer::new(tl).expect("player")
    }

    // ---- FeedbackMode ----

    #[test]
    fn feedback_mode_display() {
        assert_eq!(FeedbackMode::ConfidenceOnly.to_string(), "confidence-only");
        assert_eq!(
            FeedbackMode::IncrementalStats.to_string(),
            "incremental-stats"
        );
        assert_eq!(FeedbackMode::FullReanalyze.to_string(), "full-reanalyze");
    }

    #[test]
    fn feedback_mode_equality() {
        assert_eq!(FeedbackMode::ConfidenceOnly, FeedbackMode::ConfidenceOnly,);
        assert_ne!(FeedbackMode::ConfidenceOnly, FeedbackMode::IncrementalStats,);
    }

    #[test]
    fn feedback_mode_serialize_roundtrip() {
        let mode = FeedbackMode::IncrementalStats;
        let json = serde_json::to_string(&mode).expect("serialize");
        let deserialized: FeedbackMode = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(mode, deserialized);
    }

    // ---- BatchConfig ----

    #[test]
    fn batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.batch_size, 1);
        assert_eq!(config.feedback_mode, FeedbackMode::IncrementalStats,);
        assert!((config.reoptimize_threshold - 3.0).abs() < f64::EPSILON);
        assert!((config.min_confidence - 0.4).abs() < f64::EPSILON);
        assert_eq!(config.max_reoptimizations, 10);
    }

    #[test]
    fn batch_config_serialize_roundtrip() {
        let config = BatchConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: BatchConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config.batch_size, deserialized.batch_size,);
        assert_eq!(config.feedback_mode, deserialized.feedback_mode,);
    }

    // ---- BatchExecutor creation ----

    #[test]
    fn executor_creation() {
        let exec = BatchExecutor::new(
            simple_plan(),
            BatchConfig::default(),
            StatisticsProfile::standard(),
        )
        .expect("create");
        assert_eq!(exec.reoptimization_count(), 0);
        assert!(exec.batch_history().is_empty());
    }

    #[test]
    fn executor_zero_batch_size_error() {
        let config = BatchConfig {
            batch_size: 0,
            ..BatchConfig::default()
        };
        let result = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard());
        assert!(result.is_err());
    }

    #[test]
    fn executor_with_defaults() {
        let exec = BatchExecutor::with_defaults(simple_plan());
        assert_eq!(exec.reoptimization_count(), 0);
        assert_eq!(exec.config().feedback_mode, FeedbackMode::IncrementalStats,);
    }

    #[test]
    fn executor_debug_format() {
        let exec = BatchExecutor::with_defaults(simple_plan());
        let debug = format!("{exec:?}");
        assert!(debug.contains("BatchExecutor"));
    }

    // ---- compute_q_error ----

    #[test]
    fn q_error_perfect() {
        assert!((compute_q_error(1000.0, 1000.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_overestimate() {
        assert!((compute_q_error(2000.0, 1000.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_underestimate() {
        assert!((compute_q_error(500.0, 1000.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_zero_clamped() {
        let q = compute_q_error(0.0, 0.0);
        assert!((q - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_symmetric() {
        let q1 = compute_q_error(100.0, 500.0);
        let q2 = compute_q_error(500.0, 100.0);
        assert!((q1 - q2).abs() < f64::EPSILON);
    }

    // ---- confidence_decay_from_q_error ----

    #[test]
    fn confidence_decay_perfect() {
        let decay = confidence_decay_from_q_error(1.0);
        assert!(decay <= 1.0);
        assert!(decay > 0.0);
    }

    #[test]
    fn confidence_decay_high_error_lower() {
        let decay_low = confidence_decay_from_q_error(2.0);
        let decay_high = confidence_decay_from_q_error(10.0);
        assert!(decay_high < decay_low);
    }

    #[test]
    fn confidence_decay_always_positive() {
        for q in [1.0, 2.0, 5.0, 10.0, 100.0, 1000.0] {
            let decay = confidence_decay_from_q_error(q);
            assert!(decay > 0.0, "decay should be positive for q={q}");
            assert!(decay <= 1.0, "decay should be <= 1.0 for q={q}");
        }
    }

    // ---- compute_q_error_summary ----

    #[test]
    fn q_error_summary_empty() {
        let (avg, max) = compute_q_error_summary(&HashMap::new());
        assert!((avg - 1.0).abs() < f64::EPSILON);
        assert!((max - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_summary_single() {
        let mut fb = HashMap::new();
        fb.insert(
            1,
            OperatorFeedback {
                estimated_rows: 1000.0,
                actual_rows: 500.0,
                q_error: 2.0,
                triggered_reoptimization: false,
            },
        );
        let (avg, max) = compute_q_error_summary(&fb);
        assert!((avg - 2.0).abs() < f64::EPSILON);
        assert!((max - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_summary_multiple() {
        let mut fb = HashMap::new();
        fb.insert(
            1,
            OperatorFeedback {
                estimated_rows: 1000.0,
                actual_rows: 1000.0,
                q_error: 1.0,
                triggered_reoptimization: false,
            },
        );
        fb.insert(
            2,
            OperatorFeedback {
                estimated_rows: 1000.0,
                actual_rows: 100.0,
                q_error: 10.0,
                triggered_reoptimization: false,
            },
        );
        let (avg, max) = compute_q_error_summary(&fb);
        assert!((avg - 5.5).abs() < f64::EPSILON);
        assert!((max - 10.0).abs() < f64::EPSILON);
    }

    // ---- OperatorFeedback ----

    #[test]
    fn operator_feedback_serialize_roundtrip() {
        let fb = OperatorFeedback {
            estimated_rows: 1000.0,
            actual_rows: 1500.0,
            q_error: 1.5,
            triggered_reoptimization: false,
        };
        let json = serde_json::to_string(&fb).expect("serialize");
        let deserialized: OperatorFeedback = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(fb, deserialized);
    }

    // ---- BatchRecord ----

    #[test]
    fn batch_record_serialize_roundtrip() {
        let record = BatchRecord {
            batch_index: 0,
            snapshot_range: (0, 1),
            operator_feedback: HashMap::new(),
            reoptimized: false,
            plan: simple_plan(),
            confidence_before: 1.0,
            confidence_after: 0.9,
            avg_q_error: 1.5,
            max_q_error: 2.0,
            execution_report: ExecutionReport {
                final_plan: simple_plan(),
                was_adapted: false,
                adaptation_count: 0,
                adaptations: vec![],
                final_stats: HashMap::new(),
            },
        };
        let json = serde_json::to_string(&record).expect("serialize");
        let deserialized: BatchRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(record.batch_index, deserialized.batch_index,);
    }

    // ---- Batch execution: basic run ----

    #[test]
    fn run_single_snapshot() {
        let snapshots = vec![make_snapshot(0, "orders", 1000)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 1);
        assert_eq!(result.batches[0].batch_index, 0);
        assert_eq!(result.batches[0].snapshot_range, (0, 1));
    }

    #[test]
    fn run_multiple_snapshots_batch_size_1() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
            make_snapshot(120, "orders", 2000),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 3);
    }

    #[test]
    fn run_multiple_snapshots_batch_size_2() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
            make_snapshot(120, "orders", 2000),
            make_snapshot(180, "orders", 2500),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 2,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 2);
        assert_eq!(result.batches[0].snapshot_range, (0, 2));
        assert_eq!(result.batches[1].snapshot_range, (2, 4));
    }

    #[test]
    fn run_odd_snapshots_batch_size_2() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
            make_snapshot(120, "orders", 2000),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 2,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 2);
    }

    // ---- Feedback modes ----

    #[test]
    fn confidence_only_mode_runs() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            feedback_mode: FeedbackMode::ConfidenceOnly,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.feedback_mode, FeedbackMode::ConfidenceOnly,);
    }

    #[test]
    fn incremental_stats_mode_runs() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            feedback_mode: FeedbackMode::IncrementalStats,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
    }

    #[test]
    fn full_reanalyze_mode_runs() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            feedback_mode: FeedbackMode::FullReanalyze,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.feedback_mode, FeedbackMode::FullReanalyze,);
    }

    // ---- Reoptimization triggering ----

    #[test]
    fn reoptimization_triggered_by_high_q_error() {
        // Large discrepancy between snapshots should trigger
        // reoptimization
        let snapshots = vec![
            make_snapshot(0, "orders", 100),
            make_snapshot(60, "orders", 10_000),
        ];
        let fb = vec![ExecutionFeedback {
            time_offset: 60,
            query: "SELECT * FROM orders".to_string(),
            operator: Some("SeqScan on orders".to_string()),
            estimated_rows: 100.0,
            actual_rows: 10_000.0,
            estimated_cost: None,
            actual_time_ms: None,
        }];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let config = BatchConfig {
            reoptimize_threshold: 2.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.total_reoptimizations > 0);
    }

    #[test]
    fn no_reoptimization_when_accurate() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1050),
        ];
        let mut player = default_player(snapshots);

        let config = BatchConfig {
            reoptimize_threshold: 5.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert_eq!(result.total_reoptimizations, 0);
    }

    #[test]
    fn max_reoptimizations_respected() {
        let snapshots = vec![
            make_snapshot(0, "orders", 100),
            make_snapshot(60, "orders", 100_000),
            make_snapshot(120, "orders", 100),
            make_snapshot(180, "orders", 100_000),
            make_snapshot(240, "orders", 100),
        ];
        let mut player = default_player(snapshots);

        let config = BatchConfig {
            max_reoptimizations: 1,
            reoptimize_threshold: 2.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.total_reoptimizations <= 1);
    }

    // ---- BatchRunResult ----

    #[test]
    fn overall_avg_q_error_empty() {
        let result = BatchRunResult {
            batches: vec![],
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::ConfidenceOnly,
        };
        assert!((result.overall_avg_q_error() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_improved_true() {
        let batches = vec![
            BatchRecord {
                batch_index: 0,
                snapshot_range: (0, 1),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 1.0,
                confidence_after: 0.9,
                avg_q_error: 5.0,
                max_q_error: 5.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
            BatchRecord {
                batch_index: 1,
                snapshot_range: (1, 2),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 0.9,
                confidence_after: 0.8,
                avg_q_error: 2.0,
                max_q_error: 2.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
        ];
        let result = BatchRunResult {
            batches,
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::IncrementalStats,
        };
        assert!(result.q_error_improved());
    }

    #[test]
    fn q_error_improved_false_single_batch() {
        let batches = vec![BatchRecord {
            batch_index: 0,
            snapshot_range: (0, 1),
            operator_feedback: HashMap::new(),
            reoptimized: false,
            plan: simple_plan(),
            confidence_before: 1.0,
            confidence_after: 0.9,
            avg_q_error: 2.0,
            max_q_error: 2.0,
            execution_report: ExecutionReport {
                final_plan: simple_plan(),
                was_adapted: false,
                adaptation_count: 0,
                adaptations: vec![],
                final_stats: HashMap::new(),
            },
        }];
        let result = BatchRunResult {
            batches,
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::IncrementalStats,
        };
        assert!(!result.q_error_improved());
    }

    #[test]
    fn confidence_delta_positive() {
        let batches = vec![
            BatchRecord {
                batch_index: 0,
                snapshot_range: (0, 1),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 0.5,
                confidence_after: 0.6,
                avg_q_error: 1.5,
                max_q_error: 1.5,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
            BatchRecord {
                batch_index: 1,
                snapshot_range: (1, 2),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 0.6,
                confidence_after: 0.8,
                avg_q_error: 1.2,
                max_q_error: 1.2,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
        ];
        let result = BatchRunResult {
            batches,
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::FullReanalyze,
        };
        assert!(result.confidence_delta() > 0.0);
    }

    // ---- Two-table scenarios ----

    #[test]
    fn two_table_batch_run() {
        let snapshots = vec![
            make_two_table_snapshot(0, "orders", 1000, "lineitem", 6000),
            make_two_table_snapshot(60, "orders", 1100, "lineitem", 6600),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(join_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 2);
    }

    // ---- Reset ----

    #[test]
    fn reset_clears_state() {
        let snapshots = vec![make_snapshot(0, "orders", 1000)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        exec.run(&mut player).expect("should run");
        assert!(!exec.batch_history().is_empty());

        exec.reset(simple_plan());
        assert!(exec.batch_history().is_empty());
        assert_eq!(exec.reoptimization_count(), 0);
    }

    // ---- With column statistics ----

    #[test]
    fn snapshot_with_column_stats() {
        let col = ColumnSnapshot {
            name: "order_id".to_string(),
            ndv: 1000,
            null_fraction: 0.0,
            avg_width: 8.0,
            correlation: Some(0.99),
            min_value: Some("1".to_string()),
            max_value: Some("1000".to_string()),
        };
        let snapshots = vec![make_snapshot_with_columns(0, "orders", 1000, vec![col])];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
    }

    // ---- Batch with feedback entries ----

    #[test]
    fn batch_with_timeline_feedback() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let fb = vec![
            ExecutionFeedback {
                time_offset: 0,
                query: "SELECT * FROM orders".to_string(),
                operator: Some("SeqScan on orders".to_string()),
                estimated_rows: 1000.0,
                actual_rows: 1000.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
            ExecutionFeedback {
                time_offset: 60,
                query: "SELECT * FROM orders".to_string(),
                operator: Some("SeqScan on orders".to_string()),
                estimated_rows: 1000.0,
                actual_rows: 1500.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
        ];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let mut exec = BatchExecutor::with_defaults(simple_plan());
        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 2);
    }

    // ---- Error cases ----

    #[test]
    fn batch_error_display() {
        let err = BatchError::TimelineExhausted {
            batches_completed: 5,
        };
        let msg = format!("{err}");
        assert!(msg.contains('5'));
        assert!(msg.contains("exhausted"));
    }

    #[test]
    fn batch_error_execution_failed_display() {
        let err = BatchError::ExecutionFailed {
            batch_index: 3,
            reason: "test failure".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains('3'));
        assert!(msg.contains("test failure"));
    }

    #[test]
    fn batch_error_invalid_config_display() {
        let err = BatchError::InvalidConfig("bad setting".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("bad setting"));
    }

    // ---- Statistics profiles ----

    #[test]
    fn run_with_realtime_profile() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::new(
            simple_plan(),
            BatchConfig::default(),
            StatisticsProfile::real_time(),
        )
        .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
    }

    #[test]
    fn run_with_lazy_profile() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::new(
            simple_plan(),
            BatchConfig::default(),
            StatisticsProfile::lazy(),
        )
        .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
    }

    #[test]
    fn run_with_streaming_profile() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::new(
            simple_plan(),
            BatchConfig::default(),
            StatisticsProfile::streaming(),
        )
        .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
    }

    // ---- Large batch scenarios ----

    #[test]
    fn many_snapshots_run() {
        let snapshots: Vec<Snapshot> = (0..20)
            .map(|i| make_snapshot(i * 60, "orders", 1000 + i * 100))
            .collect();
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 20);
    }

    #[test]
    fn large_batch_size_covers_all() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
            make_snapshot(120, "orders", 2000),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 100,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 1);
    }

    // ---- Streaming inserts scenario ----

    #[test]
    fn streaming_inserts_scenario() {
        // Simulate TPC-H Q1 with growing lineitem table
        let snapshots: Vec<Snapshot> = (0..10)
            .map(|i| make_snapshot(i * 300, "lineitem", 6_000_000 + i * 500_000))
            .collect();
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 2,
            feedback_mode: FeedbackMode::IncrementalStats,
            reoptimize_threshold: 3.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(
            RelExpr::scan("lineitem"),
            config,
            StatisticsProfile::standard(),
        )
        .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 5);
    }

    // ---- Skewed data scenario ----

    #[test]
    fn skewed_data_scenario() {
        // Simulate discovering skewed data distribution
        let snapshots = vec![
            make_snapshot_with_columns(
                0,
                "orders",
                100_000,
                vec![ColumnSnapshot {
                    name: "status".to_string(),
                    ndv: 5,
                    null_fraction: 0.0,
                    avg_width: 4.0,
                    correlation: None,
                    min_value: None,
                    max_value: None,
                }],
            ),
            make_snapshot_with_columns(
                60,
                "orders",
                100_000,
                vec![ColumnSnapshot {
                    name: "status".to_string(),
                    ndv: 5,
                    null_fraction: 0.0,
                    avg_width: 4.0,
                    correlation: None,
                    min_value: None,
                    max_value: None,
                }],
            ),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
    }

    // ---- Index effectiveness scenario ----

    #[test]
    fn index_effectiveness_scenario() {
        // Simulate index becoming less effective as data grows
        let snapshots: Vec<Snapshot> = (0..5)
            .map(|i| {
                make_snapshot_with_columns(
                    i * 600,
                    "events",
                    50_000 + i * 50_000,
                    vec![ColumnSnapshot {
                        name: "event_type".to_string(),
                        ndv: 10 + i,
                        null_fraction: 0.0,
                        avg_width: 8.0,
                        correlation: Some(0.95 - (i as f64 * 0.1)),
                        min_value: None,
                        max_value: None,
                    }],
                )
            })
            .collect();
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            feedback_mode: FeedbackMode::FullReanalyze,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(
            RelExpr::scan("events"),
            config,
            StatisticsProfile::standard(),
        )
        .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 5);
    }

    // ---- All three feedback modes produce different behavior ----

    #[test]
    fn feedback_modes_produce_different_results() {
        let make_snapshots = || {
            vec![
                make_snapshot(0, "orders", 1000),
                make_snapshot(60, "orders", 5000),
                make_snapshot(120, "orders", 2000),
            ]
        };

        let modes = [
            FeedbackMode::ConfidenceOnly,
            FeedbackMode::IncrementalStats,
            FeedbackMode::FullReanalyze,
        ];

        let mut results = Vec::new();
        for mode in &modes {
            let mut player = default_player(make_snapshots());
            let config = BatchConfig {
                feedback_mode: *mode,
                ..BatchConfig::default()
            };
            let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
                .expect("create");

            let result = exec.run(&mut player).expect("should run");
            results.push(result);
        }

        // All should complete
        for result in &results {
            assert!(result.completed);
        }
    }

    // ---- Accessor methods ----

    #[test]
    fn current_plan_accessor() {
        let exec = BatchExecutor::with_defaults(simple_plan());
        assert_eq!(*exec.current_plan(), simple_plan());
    }

    #[test]
    fn config_accessor() {
        let config = BatchConfig {
            batch_size: 5,
            ..BatchConfig::default()
        };
        let exec = BatchExecutor::new(simple_plan(), config.clone(), StatisticsProfile::standard())
            .expect("create");
        assert_eq!(exec.config().batch_size, 5);
    }

    #[test]
    fn adapter_accessor() {
        let exec = BatchExecutor::new(
            simple_plan(),
            BatchConfig::default(),
            StatisticsProfile::analytical(),
        )
        .expect("create");
        assert_eq!(exec.adapter().profile().name, "Analytical",);
    }

    // ---- BatchRunResult overall_avg_q_error ----

    #[test]
    fn overall_avg_q_error_two_batches() {
        let batches = vec![
            BatchRecord {
                batch_index: 0,
                snapshot_range: (0, 1),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 1.0,
                confidence_after: 0.9,
                avg_q_error: 3.0,
                max_q_error: 3.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
            BatchRecord {
                batch_index: 1,
                snapshot_range: (1, 2),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 0.9,
                confidence_after: 0.8,
                avg_q_error: 1.0,
                max_q_error: 1.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
        ];
        let result = BatchRunResult {
            batches,
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::IncrementalStats,
        };
        assert!((result.overall_avg_q_error() - 2.0).abs() < f64::EPSILON);
    }

    // ---- Edge: batch size larger than snapshots ----

    #[test]
    fn batch_size_exceeds_snapshots() {
        let snapshots = vec![make_snapshot(0, "orders", 1000)];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 10,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 1);
    }

    // ---- Growing data scenario ----

    #[test]
    fn growing_data_reoptimization() {
        // Data grows 10x across snapshots with feedback
        // reflecting the stale estimate from batch 0
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 10_000),
            make_snapshot(120, "orders", 100_000),
        ];
        let fb = vec![
            ExecutionFeedback {
                time_offset: 60,
                query: "SELECT * FROM orders".to_string(),
                operator: Some("SeqScan on orders".to_string()),
                estimated_rows: 1000.0,
                actual_rows: 10_000.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
            ExecutionFeedback {
                time_offset: 120,
                query: "SELECT * FROM orders".to_string(),
                operator: Some("SeqScan on orders".to_string()),
                estimated_rows: 10_000.0,
                actual_rows: 100_000.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
        ];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let config = BatchConfig {
            reoptimize_threshold: 2.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        // 10x growth with feedback should trigger reoptimization
        assert!(result.total_reoptimizations > 0);
    }

    // ---- Shrinking data scenario ----

    #[test]
    fn shrinking_data_scenario() {
        let snapshots = vec![
            make_snapshot(0, "orders", 100_000),
            make_snapshot(60, "orders", 50_000),
            make_snapshot(120, "orders", 10_000),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 3);
    }

    // ---- Stable data scenario ----

    #[test]
    fn stable_data_no_reoptimization() {
        let snapshots = vec![
            make_snapshot(0, "orders", 10_000),
            make_snapshot(60, "orders", 10_000),
            make_snapshot(120, "orders", 10_000),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            reoptimize_threshold: 5.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert_eq!(result.total_reoptimizations, 0);
    }

    // ---- Join plan batch execution ----

    #[test]
    fn join_plan_batch_execution() {
        let snapshots = vec![
            make_two_table_snapshot(0, "orders", 1000, "lineitem", 6000),
            make_two_table_snapshot(60, "orders", 1500, "lineitem", 9000),
            make_two_table_snapshot(120, "orders", 2000, "lineitem", 12000),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(join_plan());

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 3);
    }

    // ---- Additional coverage tests ----

    #[test]
    fn batch_config_custom_values() {
        let config = BatchConfig {
            batch_size: 5,
            feedback_mode: FeedbackMode::FullReanalyze,
            reoptimize_threshold: 10.0,
            min_confidence: 0.1,
            max_reoptimizations: 3,
            adaptive_config: AdaptiveConfig::default(),
        };
        assert_eq!(config.batch_size, 5);
        assert_eq!(config.feedback_mode, FeedbackMode::FullReanalyze,);
    }

    #[test]
    fn q_error_near_one_high_precision() {
        let q = compute_q_error(1001.0, 1000.0);
        assert!(q > 1.0);
        assert!(q < 1.01);
    }

    #[test]
    fn q_error_very_large_ratio() {
        let q = compute_q_error(1_000_000.0, 1.0);
        assert!((q - 1_000_000.0).abs() < 1.0);
    }

    #[test]
    fn confidence_decay_at_q_1() {
        let decay = confidence_decay_from_q_error(1.0);
        assert!(decay > 0.0);
    }

    #[test]
    fn confidence_decay_at_q_100() {
        let decay = confidence_decay_from_q_error(100.0);
        assert!(decay > 0.0);
        assert!(decay < 0.5);
    }

    #[test]
    fn batch_record_snapshot_range() {
        let snapshots = vec![
            make_snapshot(0, "t", 100),
            make_snapshot(60, "t", 200),
            make_snapshot(120, "t", 300),
            make_snapshot(180, "t", 400),
            make_snapshot(240, "t", 500),
            make_snapshot(300, "t", 600),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 3,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert_eq!(result.batches.len(), 2);
        assert_eq!(result.batches[0].snapshot_range, (0, 3));
        assert_eq!(result.batches[1].snapshot_range, (3, 6));
    }

    #[test]
    fn overall_avg_q_error_single_batch() {
        let batches = vec![BatchRecord {
            batch_index: 0,
            snapshot_range: (0, 1),
            operator_feedback: HashMap::new(),
            reoptimized: false,
            plan: simple_plan(),
            confidence_before: 1.0,
            confidence_after: 1.0,
            avg_q_error: 3.5,
            max_q_error: 5.0,
            execution_report: ExecutionReport {
                final_plan: simple_plan(),
                was_adapted: false,
                adaptation_count: 0,
                adaptations: vec![],
                final_stats: HashMap::new(),
            },
        }];
        let result = BatchRunResult {
            batches,
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::ConfidenceOnly,
        };
        assert!((result.overall_avg_q_error() - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_delta_negative() {
        let batches = vec![
            BatchRecord {
                batch_index: 0,
                snapshot_range: (0, 1),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 1.0,
                confidence_after: 0.8,
                avg_q_error: 2.0,
                max_q_error: 2.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
            BatchRecord {
                batch_index: 1,
                snapshot_range: (1, 2),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 0.8,
                confidence_after: 0.5,
                avg_q_error: 3.0,
                max_q_error: 3.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
        ];
        let result = BatchRunResult {
            batches,
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::IncrementalStats,
        };
        assert!(result.confidence_delta() < 0.0);
    }

    #[test]
    fn confidence_delta_empty_batches() {
        let result = BatchRunResult {
            batches: vec![],
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::ConfidenceOnly,
        };
        assert!((result.confidence_delta() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_not_improved_when_worsening() {
        let batches = vec![
            BatchRecord {
                batch_index: 0,
                snapshot_range: (0, 1),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 1.0,
                confidence_after: 0.9,
                avg_q_error: 1.5,
                max_q_error: 2.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
            BatchRecord {
                batch_index: 1,
                snapshot_range: (1, 2),
                operator_feedback: HashMap::new(),
                reoptimized: false,
                plan: simple_plan(),
                confidence_before: 0.9,
                confidence_after: 0.7,
                avg_q_error: 5.0,
                max_q_error: 8.0,
                execution_report: ExecutionReport {
                    final_plan: simple_plan(),
                    was_adapted: false,
                    adaptation_count: 0,
                    adaptations: vec![],
                    final_stats: HashMap::new(),
                },
            },
        ];
        let result = BatchRunResult {
            batches,
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::IncrementalStats,
        };
        assert!(!result.q_error_improved());
    }

    #[test]
    fn reoptimization_by_low_confidence() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1050),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            min_confidence: 1.1,
            reoptimize_threshold: 100.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.total_reoptimizations > 0);
    }

    #[test]
    fn run_with_analytical_profile() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::new(
            simple_plan(),
            BatchConfig::default(),
            StatisticsProfile::analytical(),
        )
        .expect("create");

        let result = exec.run(&mut player).expect("should run");
        assert!(result.completed);
    }

    #[test]
    fn batch_size_exactly_matches_snapshots() {
        let snapshots = vec![
            make_snapshot(0, "t", 100),
            make_snapshot(60, "t", 200),
            make_snapshot(120, "t", 300),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 3,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 1);
    }

    #[test]
    fn batch_size_1_each_snapshot_separate() {
        let snapshots: Vec<Snapshot> = (0..7)
            .map(|i| make_snapshot(i * 60, "t", 1000 + i * 100))
            .collect();
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        assert_eq!(result.batches.len(), 7);
        for (i, batch) in result.batches.iter().enumerate() {
            assert_eq!(batch.batch_index, i);
            assert_eq!(batch.snapshot_range, (i, i + 1));
        }
    }

    #[test]
    fn feedback_mode_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(FeedbackMode::ConfidenceOnly);
        set.insert(FeedbackMode::IncrementalStats);
        set.insert(FeedbackMode::FullReanalyze);
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn operator_feedback_fields() {
        let fb = OperatorFeedback {
            estimated_rows: 500.0,
            actual_rows: 1000.0,
            q_error: 2.0,
            triggered_reoptimization: true,
        };
        assert!(fb.triggered_reoptimization);
        assert!((fb.estimated_rows - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn batch_run_result_fields() {
        let result = BatchRunResult {
            batches: vec![],
            total_reoptimizations: 5,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: false,
            feedback_mode: FeedbackMode::FullReanalyze,
        };
        assert_eq!(result.total_reoptimizations, 5);
        assert!(!result.completed);
        assert_eq!(result.feedback_mode, FeedbackMode::FullReanalyze,);
    }

    #[test]
    fn multiple_reoptimizations_across_batches() {
        let snapshots = vec![
            make_snapshot(0, "t", 100),
            make_snapshot(60, "t", 10_000),
            make_snapshot(120, "t", 100),
            make_snapshot(180, "t", 50_000),
        ];
        let fb = vec![
            ExecutionFeedback {
                time_offset: 60,
                query: "q".to_string(),
                operator: Some("SeqScan on t".to_string()),
                estimated_rows: 100.0,
                actual_rows: 10_000.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
            ExecutionFeedback {
                time_offset: 180,
                query: "q".to_string(),
                operator: Some("SeqScan on t".to_string()),
                estimated_rows: 100.0,
                actual_rows: 50_000.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
        ];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let config = BatchConfig {
            reoptimize_threshold: 2.0,
            max_reoptimizations: 5,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert!(result.total_reoptimizations >= 2);
    }

    #[test]
    fn feedback_with_cost_and_time() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let fb = vec![ExecutionFeedback {
            time_offset: 60,
            query: "q".to_string(),
            operator: Some("SeqScan on orders".to_string()),
            estimated_rows: 1000.0,
            actual_rows: 1500.0,
            estimated_cost: Some(500.0),
            actual_time_ms: Some(120.5),
        }];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let mut exec = BatchExecutor::with_defaults(simple_plan());
        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
    }

    #[test]
    fn reset_and_rerun() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let mut player1 = default_player(snapshots.clone());
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result1 = exec.run(&mut player1).expect("run 1");
        assert_eq!(result1.batches.len(), 2);

        exec.reset(simple_plan());
        let mut player2 = default_player(snapshots);
        let result2 = exec.run(&mut player2).expect("run 2");
        assert_eq!(result2.batches.len(), 2);
    }

    #[test]
    fn batch_index_sequential() {
        let snapshots: Vec<Snapshot> = (0..5).map(|i| make_snapshot(i * 60, "t", 1000)).collect();
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        for (i, batch) in result.batches.iter().enumerate() {
            assert_eq!(batch.batch_index, i);
        }
    }

    #[test]
    fn all_batches_have_plan() {
        let snapshots: Vec<Snapshot> = (0..4).map(|i| make_snapshot(i * 60, "t", 1000)).collect();
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        for batch in &result.batches {
            assert_eq!(batch.plan, simple_plan());
        }
    }

    #[test]
    fn confidence_before_after_present() {
        let snapshots = vec![make_snapshot(0, "t", 1000), make_snapshot(60, "t", 1500)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        for batch in &result.batches {
            assert!(batch.confidence_before >= 0.0);
            assert!(batch.confidence_before <= 1.0);
            assert!(batch.confidence_after >= 0.0);
            assert!(batch.confidence_after <= 1.0);
        }
    }

    #[test]
    fn avg_q_error_at_least_one() {
        let snapshots = vec![make_snapshot(0, "t", 1000), make_snapshot(60, "t", 1500)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        for batch in &result.batches {
            assert!(batch.avg_q_error >= 1.0);
            assert!(batch.max_q_error >= 1.0);
        }
    }

    #[test]
    fn max_q_error_gte_avg_q_error() {
        let snapshots = vec![make_snapshot(0, "t", 100), make_snapshot(60, "t", 10000)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        for batch in &result.batches {
            assert!(batch.max_q_error >= batch.avg_q_error);
        }
    }

    #[test]
    fn full_reanalyze_resets_confidence() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 5000),
            make_snapshot(120, "orders", 5000),
        ];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            feedback_mode: FeedbackMode::FullReanalyze,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        // After full reanalyze, confidence should be high
        let last = result.batches.last().expect("last");
        assert!(last.confidence_after >= 0.5);
    }

    #[test]
    fn empty_operator_feedback_in_batch() {
        let snapshots = vec![make_snapshot(0, "nonexistent", 0)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
    }

    #[test]
    fn batch_config_serialize_all_fields() {
        let config = BatchConfig {
            batch_size: 10,
            feedback_mode: FeedbackMode::FullReanalyze,
            reoptimize_threshold: 5.5,
            min_confidence: 0.2,
            max_reoptimizations: 7,
            adaptive_config: AdaptiveConfig::default(),
        };
        let json = serde_json::to_string(&config).expect("serialize");
        assert!(json.contains("batch_size"));
        assert!(json.contains("feedback_mode"));
        assert!(json.contains("reoptimize_threshold"));
    }

    #[test]
    fn feedback_mode_all_variants_serializable() {
        for mode in [
            FeedbackMode::ConfidenceOnly,
            FeedbackMode::IncrementalStats,
            FeedbackMode::FullReanalyze,
        ] {
            let json = serde_json::to_string(&mode).expect("serialize");
            let back: FeedbackMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn batch_error_debug_format() {
        let err = BatchError::TimelineExhausted {
            batches_completed: 3,
        };
        let debug = format!("{err:?}");
        assert!(debug.contains("TimelineExhausted"));
    }

    #[test]
    fn many_small_batches_performance() {
        let snapshots: Vec<Snapshot> = (0..50)
            .map(|i| make_snapshot(i * 10, "events", 10_000 + i * 500))
            .collect();
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::new(
            RelExpr::scan("events"),
            BatchConfig::default(),
            StatisticsProfile::standard(),
        )
        .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
        assert_eq!(result.batches.len(), 50);
    }

    #[test]
    fn batch_size_larger_than_total_single_batch() {
        let snapshots = vec![make_snapshot(0, "t", 100), make_snapshot(60, "t", 200)];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            batch_size: 1000,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert_eq!(result.batches.len(), 1);
        assert_eq!(result.batches[0].snapshot_range, (0, 2),);
    }

    #[test]
    fn two_table_with_feedback() {
        let snapshots = vec![
            make_two_table_snapshot(0, "orders", 1000, "lineitem", 6000),
            make_two_table_snapshot(60, "orders", 2000, "lineitem", 12000),
        ];
        let fb = vec![ExecutionFeedback {
            time_offset: 60,
            query: "SELECT * FROM orders JOIN lineitem".to_string(),
            operator: Some("HashJoin".to_string()),
            estimated_rows: 6000.0,
            actual_rows: 12000.0,
            estimated_cost: Some(10000.0),
            actual_time_ms: Some(500.0),
        }];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let mut exec = BatchExecutor::with_defaults(join_plan());
        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
    }

    #[test]
    fn batch_with_multiple_feedback_same_time() {
        let snapshots = vec![
            make_snapshot(0, "orders", 1000),
            make_snapshot(60, "orders", 1500),
        ];
        let fb = vec![
            ExecutionFeedback {
                time_offset: 60,
                query: "q1".to_string(),
                operator: Some("SeqScan on orders".to_string()),
                estimated_rows: 1000.0,
                actual_rows: 1500.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
            ExecutionFeedback {
                time_offset: 60,
                query: "q2".to_string(),
                operator: Some("IndexScan on orders".to_string()),
                estimated_rows: 500.0,
                actual_rows: 750.0,
                estimated_cost: None,
                actual_time_ms: None,
            },
        ];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let mut exec = BatchExecutor::with_defaults(simple_plan());
        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
    }

    #[test]
    fn reoptimization_count_accessor() {
        let snapshots = vec![make_snapshot(0, "t", 1000)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());
        assert_eq!(exec.reoptimization_count(), 0);
        exec.run(&mut player).expect("run");
        // Stable data should not trigger reoptimization
        assert_eq!(exec.reoptimization_count(), 0);
    }

    #[test]
    fn batch_history_grows() {
        let snapshots: Vec<Snapshot> = (0..3).map(|i| make_snapshot(i * 60, "t", 1000)).collect();
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        assert!(exec.batch_history().is_empty());
        exec.run(&mut player).expect("run");
        assert_eq!(exec.batch_history().len(), 3);
    }

    #[test]
    fn confidence_only_no_row_count_change() {
        let snapshots = vec![make_snapshot(0, "t", 1000), make_snapshot(60, "t", 1000)];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            feedback_mode: FeedbackMode::ConfidenceOnly,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
    }

    #[test]
    fn incremental_with_large_correction() {
        let snapshots = vec![make_snapshot(0, "t", 100), make_snapshot(60, "t", 10_000)];
        let fb = vec![ExecutionFeedback {
            time_offset: 60,
            query: "q".to_string(),
            operator: Some("SeqScan on t".to_string()),
            estimated_rows: 100.0,
            actual_rows: 10_000.0,
            estimated_cost: None,
            actual_time_ms: None,
        }];
        let tl = make_timeline_with_feedback(snapshots, fb);
        let mut player = TimelinePlayer::new(tl).expect("player");

        let config = BatchConfig {
            feedback_mode: FeedbackMode::IncrementalStats,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert!(result.completed);
    }

    #[test]
    fn execution_report_in_batch_record() {
        let snapshots = vec![make_snapshot(0, "t", 1000)];
        let mut player = default_player(snapshots);
        let mut exec = BatchExecutor::with_defaults(simple_plan());

        let result = exec.run(&mut player).expect("run");
        let report = &result.batches[0].execution_report;
        assert_eq!(report.final_plan, simple_plan());
    }

    #[test]
    fn final_plan_matches_initial_without_reopt() {
        let snapshots = vec![make_snapshot(0, "t", 1000), make_snapshot(60, "t", 1000)];
        let mut player = default_player(snapshots);
        let config = BatchConfig {
            reoptimize_threshold: 100.0,
            ..BatchConfig::default()
        };
        let mut exec = BatchExecutor::new(simple_plan(), config, StatisticsProfile::standard())
            .expect("create");

        let result = exec.run(&mut player).expect("run");
        assert_eq!(result.final_plan, simple_plan());
    }

    #[test]
    fn batch_run_result_serialize_roundtrip() {
        let result = BatchRunResult {
            batches: vec![],
            total_reoptimizations: 0,
            final_plan: simple_plan(),
            config: BatchConfig::default(),
            completed: true,
            feedback_mode: FeedbackMode::ConfidenceOnly,
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: BatchRunResult = serde_json::from_str(&json).expect("deserialize");
        assert!(back.completed);
        assert_eq!(back.total_reoptimizations, 0);
    }
}
