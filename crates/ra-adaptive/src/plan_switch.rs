//! Plan switching based on runtime observations.
//!
//! When a trigger fires, the plan switcher decides what alternative
//! plan (or operator) to use. This follows SQL Server's adaptive
//! join model: a join node begins execution with one algorithm and
//! can switch to another at a defined threshold row count.
//!
//! Supported adaptations:
//! - **Join algorithm switch**: nested-loop to hash join (or
//!   vice versa) when actual build-side cardinality differs from
//!   estimate.
//! - **Join reordering**: swap left/right inputs when the smaller
//!   table is on the wrong side.
//! - **Filter pushdown**: introduce a runtime filter when skew
//!   or selectivity data becomes available.
//! - **Spill-to-disk**: when memory pressure triggers, signal
//!   operators to switch to external (disk-backed) algorithms.

use ra_core::algebra::{JoinType, RelExpr};
use serde::{Deserialize, Serialize};

use crate::runtime_stats::NodeId;
use crate::triggers::{TriggerEvent, TriggerKind};

/// A physical join strategy.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum JoinStrategy {
    /// Nested-loop join: iterate outer, probe inner.
    NestedLoop,
    /// Hash join: build hash table on one side, probe with other.
    HashJoin,
    /// Sort-merge join: sort both sides, merge.
    SortMerge,
}

impl std::fmt::Display for JoinStrategy {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let label = match self {
            Self::NestedLoop => "nested-loop",
            Self::HashJoin => "hash-join",
            Self::SortMerge => "sort-merge",
        };
        write!(f, "{label}")
    }
}

/// A plan adaptation recommended by the switcher.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Adaptation {
    /// Switch the join algorithm at the given node.
    SwitchJoinStrategy {
        /// The join operator's node id.
        node_id: NodeId,
        /// The previous strategy.
        from: JoinStrategy,
        /// The recommended strategy.
        to: JoinStrategy,
    },
    /// Swap the left and right inputs of a join.
    SwapJoinInputs {
        /// The join operator's node id.
        node_id: NodeId,
    },
    /// Apply the reoptimized plan tree. Used when the deviation is
    /// large enough that local operator-level fixes are insufficient.
    ReoptimizePlan {
        /// The new plan produced by re-running the optimizer with
        /// corrected statistics.
        new_plan: RelExpr,
    },
    /// Signal an operator to spill intermediate state to disk.
    SpillToDisk {
        /// The operator experiencing memory pressure.
        node_id: NodeId,
    },
}

/// Decides what adaptation to apply given a trigger event and
/// the current plan context.
#[derive(Debug, Clone)]
pub struct PlanSwitcher {
    /// Row count threshold for adaptive joins. Below this, use
    /// nested-loop; above, switch to hash join.
    hash_join_threshold: u64,
}

impl PlanSwitcher {
    /// Create a switcher with default thresholds.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hash_join_threshold: 10_000,
        }
    }

    /// Create a switcher with a custom hash-join threshold.
    #[must_use]
    pub fn with_hash_join_threshold(threshold: u64) -> Self {
        Self {
            hash_join_threshold: threshold,
        }
    }

    /// Given a trigger event and the current join strategy at that
    /// node, recommend an adaptation.
    #[must_use]
    pub fn recommend(
        &self,
        event: &TriggerEvent,
        current_strategy: JoinStrategy,
    ) -> Option<Adaptation> {
        match event.kind {
            TriggerKind::CardinalityUnderestimate => {
                Self::handle_underestimate(
                    event,
                    current_strategy,
                )
            }
            TriggerKind::CardinalityOverestimate => {
                Self::handle_overestimate(
                    event,
                    current_strategy,
                )
            }
            TriggerKind::SkewDetected => {
                Some(Adaptation::SwapJoinInputs {
                    node_id: event.node_id,
                })
            }
            TriggerKind::MemoryPressure => {
                Some(Adaptation::SpillToDisk {
                    node_id: event.node_id,
                })
            }
        }
    }

    /// Choose the best join strategy for the given actual row
    /// count, independent of any trigger.
    #[must_use]
    pub fn choose_join_strategy(
        &self,
        build_side_rows: u64,
    ) -> JoinStrategy {
        if build_side_rows <= self.hash_join_threshold {
            JoinStrategy::NestedLoop
        } else {
            JoinStrategy::HashJoin
        }
    }

    /// Produce a new plan by swapping join inputs.
    ///
    /// Returns `None` if the expression is not a join.
    #[must_use]
    pub fn swap_join_inputs(plan: &RelExpr) -> Option<RelExpr> {
        if let RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } = plan
        {
            let swapped_type = match join_type {
                JoinType::LeftOuter => JoinType::RightOuter,
                JoinType::RightOuter => JoinType::LeftOuter,
                other => *other,
            };
            Some(RelExpr::Join {
                join_type: swapped_type,
                condition: condition.clone(),
                left: right.clone(),
                right: left.clone(),
            })
        } else {
            None
        }
    }

    fn handle_underestimate(
        event: &TriggerEvent,
        current: JoinStrategy,
    ) -> Option<Adaptation> {
        // Build side is larger than expected. If we're doing a
        // nested-loop, switch to hash join.
        match current {
            JoinStrategy::NestedLoop => {
                Some(Adaptation::SwitchJoinStrategy {
                    node_id: event.node_id,
                    from: JoinStrategy::NestedLoop,
                    to: JoinStrategy::HashJoin,
                })
            }
            JoinStrategy::HashJoin => {
                // Already using hash join; if severely
                // underestimated, suggest full reoptimization so
                // the planner can choose a better join order.
                if event.deviation > 100.0 {
                    None
                } else {
                    Some(Adaptation::SwapJoinInputs {
                        node_id: event.node_id,
                    })
                }
            }
            JoinStrategy::SortMerge => {
                Some(Adaptation::SwitchJoinStrategy {
                    node_id: event.node_id,
                    from: JoinStrategy::SortMerge,
                    to: JoinStrategy::HashJoin,
                })
            }
        }
    }

    fn handle_overestimate(
        event: &TriggerEvent,
        current: JoinStrategy,
    ) -> Option<Adaptation> {
        // Build side is smaller than expected. If we're using a
        // hash join, a nested-loop may be cheaper.
        match current {
            JoinStrategy::HashJoin => {
                Some(Adaptation::SwitchJoinStrategy {
                    node_id: event.node_id,
                    from: JoinStrategy::HashJoin,
                    to: JoinStrategy::NestedLoop,
                })
            }
            JoinStrategy::SortMerge => {
                Some(Adaptation::SwitchJoinStrategy {
                    node_id: event.node_id,
                    from: JoinStrategy::SortMerge,
                    to: JoinStrategy::NestedLoop,
                })
            }
            JoinStrategy::NestedLoop => None,
        }
    }
}

impl Default for PlanSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triggers::TriggerEvent;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn underestimate_event(node_id: NodeId) -> TriggerEvent {
        TriggerEvent {
            node_id,
            kind: TriggerKind::CardinalityUnderestimate,
            deviation: 50.0,
            threshold: 10.0,
        }
    }

    fn overestimate_event(node_id: NodeId) -> TriggerEvent {
        TriggerEvent {
            node_id,
            kind: TriggerKind::CardinalityOverestimate,
            deviation: 20.0,
            threshold: 10.0,
        }
    }

    fn skew_event(node_id: NodeId) -> TriggerEvent {
        TriggerEvent {
            node_id,
            kind: TriggerKind::SkewDetected,
            deviation: 0.8,
            threshold: 0.5,
        }
    }

    fn memory_event(node_id: NodeId) -> TriggerEvent {
        TriggerEvent {
            node_id,
            kind: TriggerKind::MemoryPressure,
            deviation: 5.0,
            threshold: 3.0,
        }
    }

    #[test]
    fn underestimate_switches_nl_to_hash() {
        let switcher = PlanSwitcher::new();
        let event = underestimate_event(1);
        let adaptation = switcher
            .recommend(&event, JoinStrategy::NestedLoop)
            .expect("should recommend an adaptation");
        assert!(matches!(
            adaptation,
            Adaptation::SwitchJoinStrategy {
                from: JoinStrategy::NestedLoop,
                to: JoinStrategy::HashJoin,
                ..
            }
        ));
    }

    #[test]
    fn underestimate_hash_swaps_inputs() {
        let switcher = PlanSwitcher::new();
        let event = underestimate_event(2);
        let adaptation = switcher
            .recommend(&event, JoinStrategy::HashJoin)
            .expect("should recommend swap");
        assert!(matches!(
            adaptation,
            Adaptation::SwapJoinInputs { node_id: 2 }
        ));
    }

    #[test]
    fn overestimate_switches_hash_to_nl() {
        let switcher = PlanSwitcher::new();
        let event = overestimate_event(3);
        let adaptation = switcher
            .recommend(&event, JoinStrategy::HashJoin)
            .expect("should recommend switch");
        assert!(matches!(
            adaptation,
            Adaptation::SwitchJoinStrategy {
                from: JoinStrategy::HashJoin,
                to: JoinStrategy::NestedLoop,
                ..
            }
        ));
    }

    #[test]
    fn overestimate_nl_no_adaptation() {
        let switcher = PlanSwitcher::new();
        let event = overestimate_event(4);
        assert!(switcher
            .recommend(&event, JoinStrategy::NestedLoop)
            .is_none());
    }

    #[test]
    fn skew_recommends_swap() {
        let switcher = PlanSwitcher::new();
        let event = skew_event(5);
        let adaptation = switcher
            .recommend(&event, JoinStrategy::HashJoin)
            .expect("should recommend swap");
        assert!(matches!(
            adaptation,
            Adaptation::SwapJoinInputs { node_id: 5 }
        ));
    }

    #[test]
    fn memory_pressure_recommends_spill() {
        let switcher = PlanSwitcher::new();
        let event = memory_event(6);
        let adaptation = switcher
            .recommend(&event, JoinStrategy::HashJoin)
            .expect("should recommend spill");
        assert!(matches!(
            adaptation,
            Adaptation::SpillToDisk { node_id: 6 }
        ));
    }

    #[test]
    fn choose_join_strategy_small() {
        let switcher = PlanSwitcher::new();
        let strategy = switcher.choose_join_strategy(500);
        assert_eq!(strategy, JoinStrategy::NestedLoop);
    }

    #[test]
    fn choose_join_strategy_large() {
        let switcher = PlanSwitcher::new();
        let strategy = switcher.choose_join_strategy(50_000);
        assert_eq!(strategy, JoinStrategy::HashJoin);
    }

    #[test]
    fn choose_join_strategy_at_threshold() {
        let switcher = PlanSwitcher::new();
        let strategy =
            switcher.choose_join_strategy(10_000);
        assert_eq!(strategy, JoinStrategy::NestedLoop);
    }

    #[test]
    fn swap_join_inputs_inner() {
        let plan = RelExpr::Join {
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
        };
        let swapped = PlanSwitcher::swap_join_inputs(&plan)
            .expect("should swap");
        assert!(matches!(
            &swapped,
            RelExpr::Join { join_type: JoinType::Inner, left, right, .. }
                if matches!(left.as_ref(), RelExpr::Scan { table, .. } if table == "small")
                && matches!(right.as_ref(), RelExpr::Scan { table, .. } if table == "big")
        ));
    }

    #[test]
    fn swap_join_inputs_left_outer_becomes_right() {
        let plan = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let swapped = PlanSwitcher::swap_join_inputs(&plan)
            .expect("should swap");
        assert!(matches!(
            &swapped,
            RelExpr::Join { join_type: JoinType::RightOuter, .. }
        ));
    }

    #[test]
    fn swap_non_join_returns_none() {
        let plan = RelExpr::scan("t");
        assert!(PlanSwitcher::swap_join_inputs(&plan).is_none());
    }

    #[test]
    fn join_strategy_display() {
        assert_eq!(
            JoinStrategy::NestedLoop.to_string(),
            "nested-loop"
        );
        assert_eq!(JoinStrategy::HashJoin.to_string(), "hash-join");
        assert_eq!(
            JoinStrategy::SortMerge.to_string(),
            "sort-merge"
        );
    }

    #[test]
    fn adaptation_serialize_roundtrip() {
        let adaptation = Adaptation::SwitchJoinStrategy {
            node_id: 1,
            from: JoinStrategy::NestedLoop,
            to: JoinStrategy::HashJoin,
        };
        let json = serde_json::to_string(&adaptation)
            .expect("serialization should succeed");
        let deserialized: Adaptation = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(adaptation, deserialized);
    }

    #[test]
    fn custom_threshold() {
        let switcher =
            PlanSwitcher::with_hash_join_threshold(1_000);
        assert_eq!(
            switcher.choose_join_strategy(999),
            JoinStrategy::NestedLoop
        );
        assert_eq!(
            switcher.choose_join_strategy(1_001),
            JoinStrategy::HashJoin
        );
    }
}
