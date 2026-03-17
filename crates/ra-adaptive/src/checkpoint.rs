//! Execution checkpoints for safe plan transitions.
//!
//! A checkpoint captures the intermediate state at an operator
//! boundary so that a plan switch can occur without losing or
//! re-processing data. This follows the Spark AQE model where
//! shuffle boundaries are natural checkpoint points.
//!
//! Checkpoints are lightweight: they record what rows have been
//! produced (as a count and optional materialized output), the
//! cursor position, and enough metadata to resume from the new
//! plan.

use serde::{Deserialize, Serialize};

use crate::plan_switch::Adaptation;
use crate::runtime_stats::NodeId;

/// The state of execution at a checkpoint boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointState {
    /// Execution has not started at this node.
    NotStarted,
    /// The operator is actively producing rows.
    InProgress {
        /// Number of rows already emitted.
        rows_emitted: u64,
    },
    /// The operator has finished producing all rows.
    Completed {
        /// Total rows emitted.
        total_rows: u64,
    },
}

/// A checkpoint recorded at a specific operator in the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The operator where this checkpoint was taken.
    pub node_id: NodeId,
    /// Sequential checkpoint number for ordering.
    pub sequence: u64,
    /// The execution state at this point.
    pub state: CheckpointState,
    /// Whether the checkpoint's output has been materialized
    /// (i.e., buffered rows are available for replay).
    pub materialized: bool,
}

impl Checkpoint {
    /// Create a checkpoint for an operator that hasn't started.
    #[must_use]
    pub fn not_started(node_id: NodeId, sequence: u64) -> Self {
        Self {
            node_id,
            sequence,
            state: CheckpointState::NotStarted,
            materialized: false,
        }
    }

    /// Create a checkpoint for an in-progress operator.
    #[must_use]
    pub fn in_progress(
        node_id: NodeId,
        sequence: u64,
        rows_emitted: u64,
    ) -> Self {
        Self {
            node_id,
            sequence,
            state: CheckpointState::InProgress { rows_emitted },
            materialized: false,
        }
    }

    /// Create a checkpoint for a completed operator.
    #[must_use]
    pub fn completed(
        node_id: NodeId,
        sequence: u64,
        total_rows: u64,
    ) -> Self {
        Self {
            node_id,
            sequence,
            state: CheckpointState::Completed { total_rows },
            materialized: false,
        }
    }

    /// Whether the operator has finished producing rows.
    #[must_use]
    pub fn is_completed(&self) -> bool {
        matches!(self.state, CheckpointState::Completed { .. })
    }

    /// Number of rows emitted at this checkpoint.
    #[must_use]
    pub fn rows_emitted(&self) -> u64 {
        match &self.state {
            CheckpointState::NotStarted => 0,
            CheckpointState::InProgress { rows_emitted } => {
                *rows_emitted
            }
            CheckpointState::Completed { total_rows } => {
                *total_rows
            }
        }
    }
}

/// Manages checkpoints across all operators in a plan and
/// determines safe transition points for plan switching.
#[derive(Debug)]
pub struct CheckpointManager {
    checkpoints: Vec<Checkpoint>,
    next_sequence: u64,
}

impl CheckpointManager {
    /// Create an empty checkpoint manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            checkpoints: Vec::new(),
            next_sequence: 0,
        }
    }

    /// Record a checkpoint for the given operator state.
    pub fn record(
        &mut self,
        node_id: NodeId,
        state: CheckpointState,
    ) -> &Checkpoint {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        let cp = Checkpoint {
            node_id,
            sequence: seq,
            state,
            materialized: false,
        };
        self.checkpoints.push(cp);
        // Safe: we just pushed an element.
        &self.checkpoints[self.checkpoints.len() - 1]
    }

    /// Get the latest checkpoint for a specific operator.
    #[must_use]
    pub fn latest_for(&self, node_id: NodeId) -> Option<&Checkpoint> {
        self.checkpoints
            .iter()
            .rev()
            .find(|cp| cp.node_id == node_id)
    }

    /// Determine whether it is safe to apply the given adaptation.
    ///
    /// A plan switch is safe when:
    /// - The target operator is in-progress (not completed), so
    ///   switching strategies can still help.
    /// - The operator has a checkpoint (so intermediate state is
    ///   recoverable).
    #[must_use]
    pub fn is_safe_transition(
        &self,
        adaptation: &Adaptation,
    ) -> bool {
        let target_node = match adaptation {
            Adaptation::SwitchJoinStrategy { node_id, .. }
            | Adaptation::SwapJoinInputs { node_id }
            | Adaptation::SpillToDisk { node_id } => *node_id,
            Adaptation::ReoptimizePlan { .. } => {
                // Full reoptimization is safe if all operators
                // are either not-started or have checkpoints.
                return self.all_have_checkpoints();
            }
        };
        self.latest_for(target_node)
            .is_some_and(|cp| !cp.is_completed())
    }

    /// Whether every operator that has started also has a
    /// checkpoint.
    #[must_use]
    fn all_have_checkpoints(&self) -> bool {
        // For now, consider it safe if we have at least one
        // checkpoint (the execution has been instrumented).
        !self.checkpoints.is_empty()
    }

    /// Total number of checkpoints recorded.
    #[must_use]
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    /// All checkpoints in sequence order.
    #[must_use]
    pub fn all_checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    /// Mark the latest checkpoint for an operator as materialized.
    pub fn mark_materialized(
        &mut self,
        node_id: NodeId,
    ) -> bool {
        if let Some(cp) = self
            .checkpoints
            .iter_mut()
            .rev()
            .find(|cp| cp.node_id == node_id)
        {
            cp.materialized = true;
            true
        } else {
            false
        }
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_switch::{Adaptation, JoinStrategy};

    #[test]
    fn checkpoint_not_started() {
        let cp = Checkpoint::not_started(1, 0);
        assert_eq!(cp.rows_emitted(), 0);
        assert!(!cp.is_completed());
    }

    #[test]
    fn checkpoint_in_progress() {
        let cp = Checkpoint::in_progress(1, 0, 500);
        assert_eq!(cp.rows_emitted(), 500);
        assert!(!cp.is_completed());
    }

    #[test]
    fn checkpoint_completed() {
        let cp = Checkpoint::completed(1, 0, 1000);
        assert_eq!(cp.rows_emitted(), 1000);
        assert!(cp.is_completed());
    }

    #[test]
    fn manager_record_and_latest() {
        let mut mgr = CheckpointManager::new();
        mgr.record(1, CheckpointState::NotStarted);
        mgr.record(
            1,
            CheckpointState::InProgress { rows_emitted: 100 },
        );
        mgr.record(2, CheckpointState::NotStarted);

        let latest = mgr
            .latest_for(1)
            .expect("should find checkpoint for node 1");
        assert_eq!(latest.rows_emitted(), 100);
        assert_eq!(latest.sequence, 1);

        let latest2 = mgr
            .latest_for(2)
            .expect("should find checkpoint for node 2");
        assert_eq!(latest2.rows_emitted(), 0);
    }

    #[test]
    fn manager_latest_for_missing() {
        let mgr = CheckpointManager::new();
        assert!(mgr.latest_for(99).is_none());
    }

    #[test]
    fn safe_transition_in_progress() {
        let mut mgr = CheckpointManager::new();
        mgr.record(
            1,
            CheckpointState::InProgress { rows_emitted: 50 },
        );

        let adaptation = Adaptation::SwitchJoinStrategy {
            node_id: 1,
            from: JoinStrategy::NestedLoop,
            to: JoinStrategy::HashJoin,
        };
        assert!(mgr.is_safe_transition(&adaptation));
    }

    #[test]
    fn unsafe_transition_completed() {
        let mut mgr = CheckpointManager::new();
        mgr.record(
            1,
            CheckpointState::Completed { total_rows: 1000 },
        );

        let adaptation = Adaptation::SwitchJoinStrategy {
            node_id: 1,
            from: JoinStrategy::NestedLoop,
            to: JoinStrategy::HashJoin,
        };
        assert!(!mgr.is_safe_transition(&adaptation));
    }

    #[test]
    fn unsafe_transition_no_checkpoint() {
        let mgr = CheckpointManager::new();
        let adaptation = Adaptation::SwapJoinInputs { node_id: 1 };
        assert!(!mgr.is_safe_transition(&adaptation));
    }

    #[test]
    fn reoptimize_safe_with_checkpoints() {
        let mut mgr = CheckpointManager::new();
        mgr.record(
            1,
            CheckpointState::InProgress { rows_emitted: 10 },
        );
        let adaptation = Adaptation::ReoptimizePlan {
            new_plan: ra_core::algebra::RelExpr::scan("t"),
        };
        assert!(mgr.is_safe_transition(&adaptation));
    }

    #[test]
    fn reoptimize_unsafe_no_checkpoints() {
        let mgr = CheckpointManager::new();
        let adaptation = Adaptation::ReoptimizePlan {
            new_plan: ra_core::algebra::RelExpr::scan("t"),
        };
        assert!(!mgr.is_safe_transition(&adaptation));
    }

    #[test]
    fn mark_materialized() {
        let mut mgr = CheckpointManager::new();
        mgr.record(
            1,
            CheckpointState::InProgress { rows_emitted: 50 },
        );
        assert!(mgr.mark_materialized(1));
        let cp = mgr.latest_for(1).expect("should exist");
        assert!(cp.materialized);
    }

    #[test]
    fn mark_materialized_missing() {
        let mut mgr = CheckpointManager::new();
        assert!(!mgr.mark_materialized(99));
    }

    #[test]
    fn checkpoint_count() {
        let mut mgr = CheckpointManager::new();
        assert_eq!(mgr.checkpoint_count(), 0);
        mgr.record(1, CheckpointState::NotStarted);
        mgr.record(2, CheckpointState::NotStarted);
        assert_eq!(mgr.checkpoint_count(), 2);
    }

    #[test]
    fn all_checkpoints_ordering() {
        let mut mgr = CheckpointManager::new();
        mgr.record(1, CheckpointState::NotStarted);
        mgr.record(
            2,
            CheckpointState::InProgress { rows_emitted: 10 },
        );
        let all = mgr.all_checkpoints();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].sequence, 0);
        assert_eq!(all[1].sequence, 1);
    }

    #[test]
    fn checkpoint_serialize_roundtrip() {
        let cp = Checkpoint::in_progress(42, 7, 12345);
        let json = serde_json::to_string(&cp)
            .expect("serialization should succeed");
        let deserialized: Checkpoint = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(cp.node_id, deserialized.node_id);
        assert_eq!(cp.sequence, deserialized.sequence);
        assert_eq!(cp.state, deserialized.state);
    }
}
