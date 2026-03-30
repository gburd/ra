//! Reoptimization trigger detection.
//!
//! A trigger fires when runtime statistics diverge enough from the
//! optimizer's estimates to indicate that the current plan is likely
//! suboptimal. The design follows SQL Server's approach of checking
//! at "adaptation points" rather than on every row.
//!
//! Triggers are composable: [`TriggerSet`] evaluates a collection
//! of triggers and returns actionable [`TriggerEvent`]s.

use serde::{Deserialize, Serialize};

use crate::runtime_stats::{NodeId, OperatorStats, PlanStats};

/// A detected condition that warrants plan reoptimization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerEvent {
    /// Which operator triggered the event.
    pub node_id: NodeId,
    /// What kind of deviation was detected.
    pub kind: TriggerKind,
    /// The ratio that exceeded the threshold (for diagnostics).
    pub deviation: f64,
    /// The configured threshold that was exceeded.
    pub threshold: f64,
}

/// Categories of reoptimization triggers.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum TriggerKind {
    /// Actual cardinality far exceeds the estimate (underestimate).
    CardinalityUnderestimate,
    /// Actual cardinality is far below the estimate (overestimate).
    CardinalityOverestimate,
    /// A join input has skewed data distribution.
    SkewDetected,
    /// Memory usage exceeds what the plan budgeted.
    MemoryPressure,
}

impl std::fmt::Display for TriggerKind {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let label = match self {
            Self::CardinalityUnderestimate => {
                "cardinality underestimate"
            }
            Self::CardinalityOverestimate => {
                "cardinality overestimate"
            }
            Self::SkewDetected => "skew detected",
            Self::MemoryPressure => "memory pressure",
        };
        write!(f, "{label}")
    }
}

/// Configuration for trigger thresholds.
///
/// All ratio thresholds are multiplicative factors. For example,
/// a `cardinality_ratio` of `10.0` means the trigger fires when
/// actual rows exceed 10x the estimate (or fall below 1/10th).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Fire when `actual / estimated > cardinality_ratio` (underestimate).
    pub cardinality_overcount_ratio: f64,
    /// Fire when `estimated / actual > cardinality_ratio` (overestimate).
    pub cardinality_undercount_ratio: f64,
    /// Skew threshold: fraction of rows a single value must hold.
    pub skew_fraction: f64,
    /// Memory pressure: fire when actual memory exceeds this
    /// multiple of the budgeted amount.
    pub memory_ratio: f64,
    /// Minimum rows before cardinality triggers activate.
    /// Avoids noisy triggers on small data.
    pub min_rows_for_cardinality: u64,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            cardinality_overcount_ratio: 10.0,
            cardinality_undercount_ratio: 10.0,
            skew_fraction: 0.5,
            memory_ratio: 3.0,
            min_rows_for_cardinality: 1000,
        }
    }
}

/// Evaluates runtime stats against configured thresholds to
/// produce [`TriggerEvent`]s.
#[derive(Debug, Clone)]
pub struct TriggerSet {
    config: TriggerConfig,
}

impl TriggerSet {
    /// Create a trigger set with default thresholds.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: TriggerConfig::default(),
        }
    }

    /// Create a trigger set with custom thresholds.
    #[must_use]
    pub fn with_config(config: TriggerConfig) -> Self {
        Self { config }
    }

    /// Evaluate all operators in the plan and return any triggered
    /// events.
    #[must_use]
    pub fn evaluate(&self, plan_stats: &PlanStats) -> Vec<TriggerEvent> {
        let mut events = Vec::new();
        for (&node_id, stats) in &plan_stats.operators {
            self.check_cardinality(node_id, stats, &mut events);
            self.check_skew(node_id, stats, &mut events);
            self.check_memory(node_id, stats, &mut events);
        }
        events
    }

    /// Check a single operator for cardinality misestimates.
    #[must_use]
    pub fn check_operator(
        &self,
        node_id: NodeId,
        stats: &OperatorStats,
    ) -> Vec<TriggerEvent> {
        let mut events = Vec::new();
        self.check_cardinality(node_id, stats, &mut events);
        self.check_skew(node_id, stats, &mut events);
        self.check_memory(node_id, stats, &mut events);
        events
    }

    fn check_cardinality(
        &self,
        node_id: NodeId,
        stats: &OperatorStats,
        events: &mut Vec<TriggerEvent>,
    ) {
        if stats.actual_rows < self.config.min_rows_for_cardinality
        {
            return;
        }
        let Some(ratio) = stats.cardinality_ratio() else {
            return;
        };
        if ratio > self.config.cardinality_overcount_ratio {
            events.push(TriggerEvent {
                node_id,
                kind: TriggerKind::CardinalityUnderestimate,
                deviation: ratio,
                threshold: self.config.cardinality_overcount_ratio,
            });
        }
        if ratio > 0.0
            && (1.0 / ratio)
                > self.config.cardinality_undercount_ratio
        {
            events.push(TriggerEvent {
                node_id,
                kind: TriggerKind::CardinalityOverestimate,
                deviation: 1.0 / ratio,
                threshold: self.config.cardinality_undercount_ratio,
            });
        }
    }

    fn check_skew(
        &self,
        node_id: NodeId,
        stats: &OperatorStats,
        events: &mut Vec<TriggerEvent>,
    ) {
        for sketch in stats.column_sketches.values() {
            if sketch.is_skewed(self.config.skew_fraction) {
                let deviation = sketch
                    .most_frequent
                    .as_ref()
                    .map_or(0.0, |(_, count)| {
                        if sketch.total_count == 0 {
                            0.0
                        } else {
                            let d = *count as f64
                                / sketch.total_count as f64;
                            d
                        }
                    });
                events.push(TriggerEvent {
                    node_id,
                    kind: TriggerKind::SkewDetected,
                    deviation,
                    threshold: self.config.skew_fraction,
                });
                break;
            }
        }
    }

    fn check_memory(
        &self,
        node_id: NodeId,
        stats: &OperatorStats,
        events: &mut Vec<TriggerEvent>,
    ) {
        if stats.estimated_rows.abs() < f64::EPSILON {
            return;
        }
        // Estimate budgeted memory as proportional to estimated rows
        // (8 bytes per row as a baseline).
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let budgeted = (stats.estimated_rows * 8.0) as u64;
        if budgeted == 0 {
            return;
        }
        let ratio =
            stats.peak_memory_bytes as f64 / budgeted as f64;
        if ratio > self.config.memory_ratio {
            events.push(TriggerEvent {
                node_id,
                kind: TriggerKind::MemoryPressure,
                deviation: ratio,
                threshold: self.config.memory_ratio,
            });
        }
    }
}

impl Default for TriggerSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_stats::{ColumnSketch, OperatorStats, PlanStats};

    fn make_stats(
        actual: u64,
        estimated: f64,
    ) -> OperatorStats {
        let mut s = OperatorStats::with_estimate(estimated);
        s.record_rows(actual);
        s
    }

    #[test]
    fn trigger_cardinality_underestimate() {
        let triggers = TriggerSet::new();
        let stats = make_stats(200_000, 1_000.0);
        let events = triggers.check_operator(1, &stats);
        assert!(events.iter().any(|e| e.kind
            == TriggerKind::CardinalityUnderestimate));
    }

    #[test]
    fn trigger_cardinality_overestimate() {
        let triggers = TriggerSet::new();
        let stats = make_stats(1_000, 100_000.0);
        let events = triggers.check_operator(1, &stats);
        assert!(events.iter().any(|e| e.kind
            == TriggerKind::CardinalityOverestimate));
    }

    #[test]
    fn no_trigger_within_threshold() {
        let triggers = TriggerSet::new();
        let stats = make_stats(5_000, 3_000.0);
        let events = triggers.check_operator(1, &stats);
        let cardinality_events: Vec<_> = events
            .iter()
            .filter(|e| {
                e.kind
                    == TriggerKind::CardinalityUnderestimate
                    || e.kind
                        == TriggerKind::CardinalityOverestimate
            })
            .collect();
        assert!(cardinality_events.is_empty());
    }

    #[test]
    fn no_trigger_below_min_rows() {
        let triggers = TriggerSet::new();
        let stats = make_stats(50, 1.0);
        let events = triggers.check_operator(1, &stats);
        let cardinality_events: Vec<_> = events
            .iter()
            .filter(|e| {
                e.kind
                    == TriggerKind::CardinalityUnderestimate
                    || e.kind
                        == TriggerKind::CardinalityOverestimate
            })
            .collect();
        assert!(cardinality_events.is_empty());
    }

    #[test]
    fn trigger_skew_detected() {
        let triggers = TriggerSet::new();
        let mut stats = make_stats(10_000, 10_000.0);
        let sketch = ColumnSketch {
            approx_distinct: 100,
            null_count: 0,
            total_count: 10_000,
            most_frequent: Some(("hot_value".into(), 6000)),
        };
        stats
            .column_sketches
            .insert("key_col".into(), sketch);
        let events = triggers.check_operator(1, &stats);
        assert!(events
            .iter()
            .any(|e| e.kind == TriggerKind::SkewDetected));
    }

    #[test]
    fn no_skew_trigger_below_threshold() {
        let triggers = TriggerSet::new();
        let mut stats = make_stats(10_000, 10_000.0);
        let sketch = ColumnSketch {
            approx_distinct: 100,
            null_count: 0,
            total_count: 10_000,
            most_frequent: Some(("common".into(), 200)),
        };
        stats
            .column_sketches
            .insert("col".into(), sketch);
        let events = triggers.check_operator(1, &stats);
        assert!(events
            .iter()
            .all(|e| e.kind != TriggerKind::SkewDetected));
    }

    #[test]
    fn trigger_memory_pressure() {
        let triggers = TriggerSet::new();
        let mut stats = make_stats(10_000, 1_000.0);
        // Budget = 1000 * 8 = 8000 bytes; actual = 100_000
        stats.record_memory(100_000);
        let events = triggers.check_operator(1, &stats);
        assert!(events
            .iter()
            .any(|e| e.kind == TriggerKind::MemoryPressure));
    }

    #[test]
    fn evaluate_full_plan() {
        let triggers = TriggerSet::new();
        let mut plan = PlanStats::new();
        plan.register(1, 100.0);
        plan.register(2, 10_000.0);
        plan.record_rows(1, 50_000);
        plan.record_rows(2, 9_500);

        let events = triggers.evaluate(&plan);
        // Node 1: 50000 actual vs 100 estimated = 500x underestimate
        assert!(events.iter().any(|e| e.node_id == 1
            && e.kind
                == TriggerKind::CardinalityUnderestimate));
        // Node 2: within threshold
        assert!(events.iter().all(|e| e.node_id != 2
            || e.kind == TriggerKind::MemoryPressure));
    }

    #[test]
    fn custom_config() {
        let config = TriggerConfig {
            cardinality_overcount_ratio: 2.0,
            cardinality_undercount_ratio: 2.0,
            skew_fraction: 0.3,
            memory_ratio: 1.5,
            min_rows_for_cardinality: 100,
        };
        let triggers = TriggerSet::with_config(config);
        let stats = make_stats(5_000, 2_000.0);
        let events = triggers.check_operator(1, &stats);
        assert!(events.iter().any(|e| e.kind
            == TriggerKind::CardinalityUnderestimate));
    }

    #[test]
    fn trigger_kind_display() {
        assert_eq!(
            TriggerKind::CardinalityUnderestimate.to_string(),
            "cardinality underestimate"
        );
        assert_eq!(
            TriggerKind::SkewDetected.to_string(),
            "skew detected"
        );
    }

    #[test]
    fn trigger_event_serialize_roundtrip() {
        let event = TriggerEvent {
            node_id: 42,
            kind: TriggerKind::CardinalityUnderestimate,
            deviation: 15.5,
            threshold: 10.0,
        };
        let json = serde_json::to_string(&event)
            .expect("serialization should succeed");
        let deserialized: TriggerEvent = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(event, deserialized);
    }
}
