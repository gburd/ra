//! Adaptive query executor.
//!
//! The [`AdaptiveExecutor`] orchestrates query execution by:
//! 1. Instrumenting the plan with statistics collection points.
//! 2. Running the plan, collecting runtime statistics at each operator.
//! 3. Evaluating triggers at adaptation points.
//! 4. Applying plan switches when triggers fire and transitions
//!    are safe.
//!
//! This module ties together [`runtime_stats`], [`triggers`],
//! [`plan_switch`], and [`checkpoint`] into a cohesive execution
//! pipeline.

use std::collections::HashMap;

use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::checkpoint::{
    CheckpointManager, CheckpointState,
};
use crate::plan_switch::{
    Adaptation, JoinStrategy, PlanSwitcher,
};
use crate::runtime_stats::{NodeId, PlanStats};
use crate::triggers::{TriggerConfig, TriggerEvent, TriggerSet};

/// Errors from the adaptive executor.
#[derive(Debug, thiserror::Error)]
pub enum AdaptiveError {
    /// Reoptimization failed.
    #[error("reoptimization failed: {0}")]
    ReoptimizationFailed(String),
    /// A plan switch was attempted but is not safe.
    #[error(
        "unsafe plan transition at node {node_id}: {reason}"
    )]
    UnsafeTransition {
        /// The node where the transition was attempted.
        node_id: NodeId,
        /// Why the transition is unsafe.
        reason: String,
    },
}

/// Configuration for the adaptive executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveConfig {
    /// How often (in rows) to evaluate triggers at each operator.
    pub check_interval_rows: u64,
    /// Maximum number of adaptations per query execution.
    pub max_adaptations: u32,
    /// Trigger thresholds.
    pub trigger_config: TriggerConfig,
    /// Whether to enable adaptive execution at all. When false,
    /// the executor runs the original plan without instrumentation.
    pub enabled: bool,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            check_interval_rows: 10_000,
            max_adaptations: 5,
            trigger_config: TriggerConfig::default(),
            enabled: true,
        }
    }
}

/// Record of a single adaptation applied during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationRecord {
    /// Sequential number of this adaptation.
    pub sequence: u32,
    /// The trigger that caused this adaptation.
    pub trigger: TriggerEvent,
    /// The adaptation that was applied.
    pub adaptation: Adaptation,
    /// Rows processed when the adaptation occurred.
    pub rows_at_switch: u64,
}

/// Result of an adaptive execution, including diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    /// The final plan after all adaptations.
    pub final_plan: RelExpr,
    /// Whether the original plan was modified during execution.
    pub was_adapted: bool,
    /// Number of adaptations applied.
    pub adaptation_count: u32,
    /// Detailed record of each adaptation.
    pub adaptations: Vec<AdaptationRecord>,
    /// Per-operator runtime statistics at the end of execution.
    pub final_stats: HashMap<NodeId, OperatorSummary>,
}

/// Summary statistics for an operator at end of execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorSummary {
    /// Optimizer's row estimate.
    pub estimated_rows: f64,
    /// Actual rows observed.
    pub actual_rows: u64,
    /// Cardinality ratio (actual / estimated).
    pub cardinality_ratio: Option<f64>,
}

/// The adaptive query executor.
///
/// Wraps plan execution with runtime monitoring and reoptimization.
/// The executor does not itself evaluate relational expressions
/// (that is the job of a backend). Instead, it manages the
/// feedback loop between observation and plan adjustment.
#[derive(Debug)]
pub struct AdaptiveExecutor {
    config: AdaptiveConfig,
    triggers: TriggerSet,
    switcher: PlanSwitcher,
    checkpoint_mgr: CheckpointManager,
    plan_stats: PlanStats,
    current_plan: RelExpr,
    adaptations: Vec<AdaptationRecord>,
    adaptation_count: u32,
    /// Per-node join strategies (for joins).
    join_strategies: HashMap<NodeId, JoinStrategy>,
    /// Table statistics for reoptimization.
    table_stats: HashMap<String, Statistics>,
}

impl AdaptiveExecutor {
    /// Create an executor for the given plan with default config.
    #[must_use]
    pub fn new(plan: RelExpr) -> Self {
        Self::with_config(plan, AdaptiveConfig::default())
    }

    /// Create an executor with custom configuration.
    #[must_use]
    pub fn with_config(
        plan: RelExpr,
        config: AdaptiveConfig,
    ) -> Self {
        let triggers =
            TriggerSet::with_config(config.trigger_config.clone());
        Self {
            config,
            triggers,
            switcher: PlanSwitcher::new(),
            checkpoint_mgr: CheckpointManager::new(),
            plan_stats: PlanStats::new(),
            current_plan: plan,
            adaptations: Vec::new(),
            adaptation_count: 0,
            join_strategies: HashMap::new(),
            table_stats: HashMap::new(),
        }
    }

    /// Register an operator with its estimated row count.
    pub fn register_operator(
        &mut self,
        node_id: NodeId,
        estimated_rows: f64,
    ) {
        self.plan_stats.register(node_id, estimated_rows);
        self.checkpoint_mgr
            .record(node_id, CheckpointState::NotStarted);
    }

    /// Register a join operator with its estimated rows and
    /// initial strategy.
    pub fn register_join(
        &mut self,
        node_id: NodeId,
        estimated_rows: f64,
        strategy: JoinStrategy,
    ) {
        self.register_operator(node_id, estimated_rows);
        self.join_strategies.insert(node_id, strategy);
    }

    /// Provide table statistics for potential reoptimization.
    pub fn add_table_stats(
        &mut self,
        table: impl Into<String>,
        stats: Statistics,
    ) {
        self.table_stats.insert(table.into(), stats);
    }

    /// Report that an operator has produced `count` more rows.
    ///
    /// This is called by the execution backend as rows flow through
    /// each operator. At configured intervals, it evaluates
    /// triggers and applies adaptations.
    ///
    /// Returns any adaptations that were applied.
    pub fn report_rows(
        &mut self,
        node_id: NodeId,
        count: u64,
    ) -> Vec<Adaptation> {
        if !self.config.enabled {
            return Vec::new();
        }

        self.plan_stats.record_rows(node_id, count);

        // Update checkpoint
        let rows_emitted = self
            .plan_stats
            .get(node_id)
            .map_or(0, |s| s.actual_rows);
        self.checkpoint_mgr.record(
            node_id,
            CheckpointState::InProgress { rows_emitted },
        );

        // Check if we've hit the interval for trigger evaluation
        if rows_emitted % self.config.check_interval_rows != 0 {
            return Vec::new();
        }

        self.evaluate_and_adapt(node_id)
    }

    /// Mark an operator as completed.
    pub fn report_completed(
        &mut self,
        node_id: NodeId,
        total_rows: u64,
    ) {
        self.plan_stats.record_rows(node_id, 0);
        if let Some(stats) =
            self.plan_stats.operators.get_mut(&node_id)
        {
            stats.actual_rows = total_rows;
        }
        self.checkpoint_mgr.record(
            node_id,
            CheckpointState::Completed { total_rows },
        );
    }

    /// Force an immediate trigger evaluation for a specific
    /// operator, regardless of the check interval.
    pub fn force_evaluate(
        &mut self,
        node_id: NodeId,
    ) -> Vec<Adaptation> {
        if !self.config.enabled {
            return Vec::new();
        }
        self.evaluate_and_adapt(node_id)
    }

    /// Get the current plan (may have been modified by adaptations).
    #[must_use]
    pub fn current_plan(&self) -> &RelExpr {
        &self.current_plan
    }

    /// Get the current join strategy for a node.
    #[must_use]
    pub fn join_strategy(
        &self,
        node_id: NodeId,
    ) -> Option<JoinStrategy> {
        self.join_strategies.get(&node_id).copied()
    }

    /// Generate the final execution report.
    #[must_use]
    pub fn report(&self) -> ExecutionReport {
        let mut final_stats = HashMap::new();
        for (&node_id, stats) in &self.plan_stats.operators {
            final_stats.insert(
                node_id,
                OperatorSummary {
                    estimated_rows: stats.estimated_rows,
                    actual_rows: stats.actual_rows,
                    cardinality_ratio: stats.cardinality_ratio(),
                },
            );
        }
        ExecutionReport {
            final_plan: self.current_plan.clone(),
            was_adapted: !self.adaptations.is_empty(),
            adaptation_count: self.adaptation_count,
            adaptations: self.adaptations.clone(),
            final_stats,
        }
    }

    /// Number of adaptations applied so far.
    #[must_use]
    pub fn adaptation_count(&self) -> u32 {
        self.adaptation_count
    }

    /// Access the checkpoint manager.
    #[must_use]
    pub fn checkpoints(&self) -> &CheckpointManager {
        &self.checkpoint_mgr
    }

    fn evaluate_and_adapt(
        &mut self,
        node_id: NodeId,
    ) -> Vec<Adaptation> {
        if self.adaptation_count >= self.config.max_adaptations {
            return Vec::new();
        }

        let Some(stats) = self.plan_stats.get(node_id) else {
            return Vec::new();
        };
        let events =
            self.triggers.check_operator(node_id, stats);

        let mut applied = Vec::new();
        for event in events {
            if self.adaptation_count
                >= self.config.max_adaptations
            {
                break;
            }
            if let Some(adaptation) = self.try_adapt(&event) {
                let rows_at = self
                    .plan_stats
                    .get(node_id)
                    .map_or(0, |s| s.actual_rows);
                self.adaptations.push(AdaptationRecord {
                    sequence: self.adaptation_count,
                    trigger: event,
                    adaptation: adaptation.clone(),
                    rows_at_switch: rows_at,
                });
                self.adaptation_count += 1;
                info!(
                    node_id,
                    adaptation_count = self.adaptation_count,
                    "applied adaptation"
                );
                applied.push(adaptation);
            }
        }
        applied
    }

    fn try_adapt(
        &mut self,
        event: &TriggerEvent,
    ) -> Option<Adaptation> {
        let current_strategy = self
            .join_strategies
            .get(&event.node_id)
            .copied()
            .unwrap_or(JoinStrategy::HashJoin);

        let adaptation =
            self.switcher.recommend(event, current_strategy)?;

        if !self.checkpoint_mgr.is_safe_transition(&adaptation) {
            debug!(
                node_id = event.node_id,
                "skipping adaptation: transition not safe"
            );
            return None;
        }

        self.apply_adaptation(&adaptation);
        Some(adaptation)
    }

    fn apply_adaptation(&mut self, adaptation: &Adaptation) {
        match adaptation {
            Adaptation::SwitchJoinStrategy {
                node_id, to, ..
            } => {
                self.join_strategies.insert(*node_id, *to);
            }
            Adaptation::SwapJoinInputs { .. } => {
                if let Some(swapped) =
                    PlanSwitcher::swap_join_inputs(
                        &self.current_plan,
                    )
                {
                    self.current_plan = swapped;
                }
            }
            Adaptation::ReoptimizePlan { new_plan } => {
                self.current_plan = new_plan.clone();
            }
            Adaptation::SpillToDisk { node_id } => {
                debug!(
                    node_id,
                    "signaling spill-to-disk"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_switch::JoinStrategy;
    use crate::triggers::TriggerConfig;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    fn simple_join() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(
                    ColumnRef::new("b"),
                )),
            },
            left: Box::new(RelExpr::scan("big")),
            right: Box::new(RelExpr::scan("small")),
        }
    }

    #[test]
    fn executor_creation() {
        let plan = RelExpr::scan("t");
        let exec = AdaptiveExecutor::new(plan.clone());
        assert_eq!(*exec.current_plan(), plan);
        assert_eq!(exec.adaptation_count(), 0);
    }

    #[test]
    fn register_operator_creates_checkpoint() {
        let mut exec = AdaptiveExecutor::new(RelExpr::scan("t"));
        exec.register_operator(1, 100.0);
        assert_eq!(exec.checkpoints().checkpoint_count(), 1);
    }

    #[test]
    fn register_join_sets_strategy() {
        let mut exec = AdaptiveExecutor::new(simple_join());
        exec.register_join(1, 1000.0, JoinStrategy::NestedLoop);
        assert_eq!(
            exec.join_strategy(1),
            Some(JoinStrategy::NestedLoop)
        );
    }

    #[test]
    fn report_rows_below_interval_no_adaptation() {
        let mut exec = AdaptiveExecutor::new(simple_join());
        exec.register_join(1, 100.0, JoinStrategy::NestedLoop);
        let adaptations = exec.report_rows(1, 50);
        assert!(adaptations.is_empty());
    }

    #[test]
    fn disabled_executor_no_adaptation() {
        let config = AdaptiveConfig {
            enabled: false,
            ..AdaptiveConfig::default()
        };
        let mut exec =
            AdaptiveExecutor::with_config(simple_join(), config);
        exec.register_join(1, 1.0, JoinStrategy::NestedLoop);
        // Even with a massive cardinality misestimate, disabled
        // executor does nothing.
        let adaptations = exec.report_rows(1, 1_000_000);
        assert!(adaptations.is_empty());
    }

    #[test]
    fn max_adaptations_respected() {
        let config = AdaptiveConfig {
            max_adaptations: 1,
            check_interval_rows: 1000,
            trigger_config: TriggerConfig {
                cardinality_overcount_ratio: 2.0,
                min_rows_for_cardinality: 100,
                ..TriggerConfig::default()
            },
            enabled: true,
        };
        let mut exec =
            AdaptiveExecutor::with_config(simple_join(), config);
        exec.register_join(1, 100.0, JoinStrategy::NestedLoop);

        // First interval: should trigger adaptation
        let a1 = exec.report_rows(1, 1000);
        // Second interval: max already reached
        let a2 = exec.report_rows(1, 1000);

        let expected_count = a1.len() as u32;
        assert_eq!(exec.adaptation_count(), expected_count);
        assert!(a2.is_empty());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn report_completed_records_final_stats() {
        let mut exec = AdaptiveExecutor::new(RelExpr::scan("t"));
        exec.register_operator(1, 1000.0);
        exec.report_completed(1, 1500);

        let report = exec.report();
        let summary = report
            .final_stats
            .get(&1)
            .expect("node 1 should have stats");
        assert_eq!(summary.actual_rows, 1500);
        assert!((summary.estimated_rows - 1000.0).abs() < f64::EPSILON);
        let ratio = summary
            .cardinality_ratio
            .expect("ratio should exist");
        assert!((ratio - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn execution_report_not_adapted() {
        let mut exec = AdaptiveExecutor::new(RelExpr::scan("t"));
        exec.register_operator(1, 100.0);
        exec.report_completed(1, 100);
        let report = exec.report();
        assert!(!report.was_adapted);
        assert_eq!(report.adaptation_count, 0);
        assert!(report.adaptations.is_empty());
    }

    #[test]
    fn force_evaluate_triggers_adaptation() {
        let config = AdaptiveConfig {
            check_interval_rows: 1_000_000,
            trigger_config: TriggerConfig {
                cardinality_overcount_ratio: 2.0,
                min_rows_for_cardinality: 100,
                ..TriggerConfig::default()
            },
            enabled: true,
            ..AdaptiveConfig::default()
        };
        let mut exec =
            AdaptiveExecutor::with_config(simple_join(), config);
        exec.register_join(1, 100.0, JoinStrategy::NestedLoop);

        // Report rows but not at a check interval
        exec.report_rows(1, 50_000);
        assert_eq!(exec.adaptation_count(), 0);

        // Force evaluation
        let adaptations = exec.force_evaluate(1);
        assert!(!adaptations.is_empty());
    }

    #[test]
    fn adaptation_record_serialization() {
        let record = AdaptationRecord {
            sequence: 0,
            trigger: crate::triggers::TriggerEvent {
                node_id: 1,
                kind: crate::triggers::TriggerKind::CardinalityUnderestimate,
                deviation: 50.0,
                threshold: 10.0,
            },
            adaptation: Adaptation::SwitchJoinStrategy {
                node_id: 1,
                from: JoinStrategy::NestedLoop,
                to: JoinStrategy::HashJoin,
            },
            rows_at_switch: 10_000,
        };
        let json = serde_json::to_string(&record)
            .expect("serialization should succeed");
        let deserialized: AdaptationRecord =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(record.sequence, deserialized.sequence);
        assert_eq!(
            record.rows_at_switch,
            deserialized.rows_at_switch
        );
    }

    #[test]
    fn adaptive_execution_end_to_end() {
        // Simulate a join where the build side is much larger
        // than estimated.
        let config = AdaptiveConfig {
            check_interval_rows: 10_000,
            max_adaptations: 3,
            trigger_config: TriggerConfig {
                cardinality_overcount_ratio: 5.0,
                min_rows_for_cardinality: 1000,
                ..TriggerConfig::default()
            },
            enabled: true,
        };

        let plan = simple_join();
        let mut exec =
            AdaptiveExecutor::with_config(plan, config);

        // Register: join node estimated at 100 rows
        exec.register_join(
            1,
            100.0,
            JoinStrategy::NestedLoop,
        );

        // Simulate row flow: actual cardinality is 100x estimate
        for _ in 0..10 {
            exec.report_rows(1, 10_000);
        }

        let report = exec.report();
        assert!(report.was_adapted);

        // The strategy should have switched from NL to hash
        let strategy = exec
            .join_strategy(1)
            .expect("should have a strategy");
        assert_eq!(strategy, JoinStrategy::HashJoin);
    }

    #[test]
    fn swap_join_inputs_through_executor() {
        let config = AdaptiveConfig {
            check_interval_rows: 1000,
            trigger_config: TriggerConfig {
                skew_fraction: 0.3,
                min_rows_for_cardinality: 100,
                ..TriggerConfig::default()
            },
            enabled: true,
            ..AdaptiveConfig::default()
        };

        let plan = simple_join();
        let mut exec =
            AdaptiveExecutor::with_config(plan, config);
        exec.register_join(
            1,
            10_000.0,
            JoinStrategy::HashJoin,
        );

        // Add a skewed column sketch
        if let Some(stats) =
            exec.plan_stats.operators.get_mut(&1)
        {
            stats.column_sketches.insert(
                "key".into(),
                crate::runtime_stats::ColumnSketch {
                    approx_distinct: 50,
                    null_count: 0,
                    total_count: 10_000,
                    most_frequent: Some((
                        "hot".into(),
                        5000,
                    )),
                },
            );
        }

        // Report enough rows to hit the check interval
        exec.report_rows(1, 1000);

        // Verify the executor detected skew and recommended swap
        let report = exec.report();
        assert!(report.was_adapted);
    }
}
