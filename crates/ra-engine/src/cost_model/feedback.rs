//! Execution feedback collection for closing the learning loop.
//!
//! After `PostgreSQL` executes a plan, the extension collects actual performance
//! data and feeds it back to the learner for model improvement and rule
//! effectiveness tracking.
//!
//! # Data Flow
//!
//! ```text
//! Plan executes (PostgreSQL)
//!     ↓
//! ExecutionFeedback { features, predicted, actual, rules_fired, ... }
//!     ↓
//! FeedbackCollector.record(feedback)
//!     ↓
//! ├── OnlineLearner.record(features, actual_cost)  [cost model training]
//! ├── NeuralRuleSelector.train_step(features, fp, rule_labels)  [rule selection]
//! └── model_recent_mape update  [fingerprint refresh]
//! ```

use serde::{Deserialize, Serialize};

use super::QueryFeatures;
use crate::state::SystemFingerprint;

/// Complete execution feedback record for one query.
///
/// Collected post-execution and consumed by both the cost model learner
/// and the rule selector trainer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionFeedback {
    /// Hash of the normalized query text (for deduplication/grouping).
    pub query_fingerprint: u64,
    /// Hash of the plan structure (operator tree shape).
    pub plan_fingerprint: u64,
    /// 12-dimensional structural features extracted from the [`RelExpr`].
    pub features: QueryFeatures,
    /// [`SystemFingerprint`] at planning time.
    pub system_fingerprint: SystemFingerprint,
    /// Cost predicted by the neural model at plan time.
    pub predicted_cost: f64,
    /// Actual wall-clock execution time in milliseconds.
    pub actual_time_ms: f64,
    /// Actual rows produced by the query.
    pub actual_rows: u64,
    /// Shared buffer cache hits during execution.
    pub buffers_hit: u64,
    /// Disk block reads during execution.
    pub buffers_read: u64,
    /// Which rule group indices were productive (contributed to final plan).
    pub rules_fired: Vec<u32>,
    /// Total rule groups that were enabled for this query.
    pub rules_enabled: u32,
}

/// Serializable version of [`SystemFingerprint`] for JSON persistence.
impl Serialize for SystemFingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("SystemFingerprint", 14)?;
        s.serialize_field("cpu_load_fraction", &self.cpu_load_fraction)?;
        s.serialize_field("memory_pressure", &self.memory_pressure)?;
        s.serialize_field("io_saturation", &self.io_saturation)?;
        s.serialize_field("shared_buffers_hit_rate", &self.shared_buffers_hit_rate)?;
        s.serialize_field("capabilities", &self.capabilities)?;
        s.serialize_field("avg_staleness", &self.avg_staleness)?;
        s.serialize_field("worst_staleness", &self.worst_staleness)?;
        s.serialize_field("stats_coverage", &self.stats_coverage)?;
        s.serialize_field("oltp_fraction", &self.oltp_fraction)?;
        s.serialize_field("avg_tables_per_query", &self.avg_tables_per_query)?;
        s.serialize_field("plan_cache_hit_rate", &self.plan_cache_hit_rate)?;
        s.serialize_field("model_samples_trained", &self.model_samples_trained)?;
        s.serialize_field("model_recent_mape", &self.model_recent_mape)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for SystemFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct FpHelper {
            cpu_load_fraction: f32,
            memory_pressure: f32,
            io_saturation: f32,
            shared_buffers_hit_rate: f32,
            capabilities: u64,
            avg_staleness: f32,
            worst_staleness: f32,
            stats_coverage: f32,
            oltp_fraction: f32,
            avg_tables_per_query: f32,
            plan_cache_hit_rate: f32,
            model_samples_trained: u32,
            model_recent_mape: f32,
        }

        let h = FpHelper::deserialize(deserializer)?;
        Ok(SystemFingerprint {
            cpu_load_fraction: h.cpu_load_fraction,
            memory_pressure: h.memory_pressure,
            io_saturation: h.io_saturation,
            shared_buffers_hit_rate: h.shared_buffers_hit_rate,
            capabilities: h.capabilities,
            avg_staleness: h.avg_staleness,
            worst_staleness: h.worst_staleness,
            stats_coverage: h.stats_coverage,
            oltp_fraction: h.oltp_fraction,
            avg_tables_per_query: h.avg_tables_per_query,
            plan_cache_hit_rate: h.plan_cache_hit_rate,
            model_samples_trained: h.model_samples_trained,
            model_recent_mape: h.model_recent_mape,
        })
    }
}

/// Training signal for the speculative router and continuation gate.
///
/// Records the full optimization trajectory for a single query, enabling
/// post-hoc computation of the optimal stopping point and route label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationTrace {
    /// 12-dimensional structural features at optimization time.
    pub features: QueryFeatures,
    /// Total iterations actually run.
    pub iterations_run: usize,
    /// Cost at each iteration (index 0 = after first iteration).
    pub cost_per_iteration: Vec<f64>,
    /// Why the optimizer stopped.
    pub termination_reason: String,
    /// Final cost improvement vs iteration 0 (as percentage).
    pub final_improvement_pct: f64,
    /// Iteration where 95% of total improvement was achieved.
    /// This is the "ground truth" for the router's prediction target.
    pub optimal_stop_point: usize,
    /// E-graph size at termination (nodes).
    pub egraph_nodes_final: usize,
    /// Wall-clock time spent in e-graph optimization (ms).
    pub optimization_time_ms: f64,
}

impl OptimizationTrace {
    /// Compute the optimal stopping point from cost history.
    ///
    /// Finds the first iteration N where `cost[N] <= cost[final] * 1.05`.
    #[must_use]
    pub fn compute_optimal_stop(cost_history: &[f64]) -> usize {
        if cost_history.is_empty() {
            return 0;
        }

        let final_cost = cost_history[cost_history.len() - 1];
        if final_cost <= 0.0 || !final_cost.is_finite() {
            return 0;
        }

        let threshold = final_cost * 1.05;
        for (i, &cost) in cost_history.iter().enumerate() {
            if cost <= threshold {
                return i;
            }
        }
        cost_history.len().saturating_sub(1)
    }

    /// Map the optimal stop point to a route label for training.
    ///
    /// - 0 iterations needed → Skip
    /// - 1 iteration → `LeftDeep`
    /// - 2-3 → `EGraphLow`
    /// - 4-8 → `EGraphMedium`
    /// - 9+ → `EGraphHigh`
    #[must_use]
    pub fn route_label(&self) -> u8 {
        match self.optimal_stop_point {
            0 => 0,     // Skip
            1 => 1,     // LeftDeep
            2..=3 => 2, // EGraphLow
            4..=8 => 3, // EGraphMedium
            _ => 4,     // EGraphHigh
        }
    }
}

/// Computes the mean absolute percentage error between predicted and actual.
#[must_use]
pub fn compute_mape(predicted: f64, actual: f64) -> f64 {
    if actual.abs() < 1e-9 {
        if predicted.abs() < 1e-9 {
            return 0.0;
        }
        return 1.0; // max error when actual is zero but predicted isn't
    }
    ((predicted - actual) / actual).abs()
}

/// Rolling MAPE tracker with exponential decay.
///
/// Maintains a running estimate of model prediction accuracy
/// that the [`SystemFingerprint`] reports as `model_recent_mape`.
///
/// With β=0.99 the half-life is `ln(2)/ln(1/0.99) ≈ 69` samples — i.e.
/// after 69 records the influence of the oldest observation is halved.
#[derive(Debug, Clone)]
pub struct MapeTracker {
    /// Exponential moving average of MAPE.
    ema: f64,
    /// Decay factor (default 0.99 = ~69-sample half-life).
    decay: f64,
    /// Total samples seen.
    count: u64,
}

impl MapeTracker {
    /// Create a new tracker (starts at MAPE = 1.0 = maximum uncertainty).
    #[must_use]
    pub fn new() -> Self {
        Self {
            ema: 1.0,
            decay: 0.99,
            count: 0,
        }
    }

    /// Create with a custom decay factor.
    #[must_use]
    pub fn with_decay(decay: f64) -> Self {
        Self {
            ema: 1.0,
            decay: decay.clamp(0.9, 0.999),
            count: 0,
        }
    }

    /// Record one prediction/actual pair and update the running MAPE.
    pub fn record(&mut self, predicted: f64, actual: f64) {
        let mape = compute_mape(predicted, actual);
        self.ema = self.decay * self.ema + (1.0 - self.decay) * mape;
        self.count += 1;
    }

    /// Current MAPE estimate (0.0 = perfect, 1.0 = maximum error).
    #[must_use]
    pub fn current_mape(&self) -> f32 {
        self.ema.clamp(0.0, 1.0) as f32
    }

    /// Number of samples tracked.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }
}

impl Default for MapeTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregated feedback collector that routes execution data to multiple
/// learner subsystems.
///
/// This is the central coordination point: it receives raw feedback and
/// dispatches to the cost model learner, rule selector, and MAPE tracker.
pub struct FeedbackCollector {
    /// Ring buffer of recent feedback for batch processing.
    buffer: Vec<ExecutionFeedback>,
    /// Maximum buffer size before oldest entries are dropped.
    max_buffer: usize,
    /// Rolling prediction accuracy tracker.
    mape_tracker: MapeTracker,
    /// Total feedback records processed.
    total_processed: u64,
}

impl FeedbackCollector {
    /// Create a new collector with default buffer size (512).
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(512),
            max_buffer: 512,
            mape_tracker: MapeTracker::new(),
            total_processed: 0,
        }
    }

    /// Create with a custom buffer size.
    #[must_use]
    pub fn with_buffer_size(max_buffer: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(max_buffer),
            max_buffer,
            mape_tracker: MapeTracker::new(),
            total_processed: 0,
        }
    }

    /// Record execution feedback.
    ///
    /// Updates the MAPE tracker immediately and buffers the feedback
    /// for batch processing by downstream learners.
    pub fn record(&mut self, feedback: ExecutionFeedback) {
        // Update MAPE immediately
        self.mape_tracker.record(feedback.predicted_cost, feedback.actual_time_ms);
        self.total_processed += 1;

        // Buffer for batch processing
        if self.buffer.len() >= self.max_buffer {
            self.buffer.remove(0);
        }
        self.buffer.push(feedback);
    }

    /// Drain buffered feedback for batch processing.
    ///
    /// Returns all accumulated feedback and clears the buffer. Call this
    /// periodically (e.g., every 64 samples) to train models.
    pub fn drain(&mut self) -> Vec<ExecutionFeedback> {
        std::mem::take(&mut self.buffer)
    }

    /// Current prediction accuracy (for [`SystemFingerprint`] updates).
    #[must_use]
    pub fn current_mape(&self) -> f32 {
        self.mape_tracker.current_mape()
    }

    /// Total feedback records processed since creation.
    #[must_use]
    pub fn total_processed(&self) -> u64 {
        self.total_processed
    }

    /// Number of items currently buffered.
    #[must_use]
    pub fn buffered_count(&self) -> usize {
        self.buffer.len()
    }

    /// Extract rule effectiveness labels from a batch of feedback.
    ///
    /// Returns per-sample binary labels suitable for
    /// `NeuralRuleSelector::train_batch()`. Each label array indicates
    /// which rule groups were productive for that query.
    #[must_use]
    pub fn extract_rule_labels(
        feedback: &[ExecutionFeedback],
        num_groups: usize,
    ) -> Vec<[bool; 10]> {
        feedback
            .iter()
            .map(|f| {
                let mut labels = [false; 10];
                for &rule_idx in &f.rules_fired {
                    if (rule_idx as usize) < num_groups {
                        labels[rule_idx as usize] = true;
                    }
                }
                labels
            })
            .collect()
    }
}

impl Default for FeedbackCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "tests assert exact serialization/EMA round-trip"
)]
mod tests {
    use super::*;

    fn sample_feedback() -> ExecutionFeedback {
        ExecutionFeedback {
            query_fingerprint: 12345,
            plan_fingerprint: 67890,
            features: QueryFeatures {
                table_count: 3.0,
                join_count: 2.0,
                filter_count: 1.0,
                aggregate_count: 0.0,
                subquery_count: 0.0,
                cte_count: 0.0,
                window_function_count: 0.0,
                order_by_count: 0.0,
                group_by_count: 0.0,
                distinct_flag: 0.0,
                limit_present: 0.0,
                max_join_cardinality: 1000.0,
            },
            system_fingerprint: SystemFingerprint::default(),
            predicted_cost: 50.0,
            actual_time_ms: 45.0,
            actual_rows: 1000,
            buffers_hit: 500,
            buffers_read: 50,
            rules_fired: vec![0, 2, 4],
            rules_enabled: 10,
        }
    }

    #[test]
    fn mape_tracker_starts_at_one() {
        let tracker = MapeTracker::new();
        assert!((tracker.current_mape() - 1.0).abs() < 0.01);
    }

    #[test]
    fn mape_tracker_converges_to_zero_on_perfect_predictions() {
        let mut tracker = MapeTracker::new();
        for _ in 0..500 {
            tracker.record(100.0, 100.0);
        }
        assert!(
            tracker.current_mape() < 0.05,
            "MAPE={} should converge to 0",
            tracker.current_mape()
        );
    }

    #[test]
    fn mape_tracker_reflects_errors() {
        let mut tracker = MapeTracker::new();
        for _ in 0..200 {
            // 50% error consistently
            tracker.record(150.0, 100.0);
        }
        let mape = tracker.current_mape();
        assert!(
            mape > 0.3 && mape < 0.7,
            "MAPE={mape} should reflect ~50% error"
        );
    }

    #[test]
    fn compute_mape_handles_zero_actual() {
        assert!((compute_mape(0.0, 0.0) - 0.0).abs() < f64::EPSILON);
        assert!((compute_mape(1.0, 0.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_collector_records_and_drains() {
        let mut collector = FeedbackCollector::new();
        collector.record(sample_feedback());
        collector.record(sample_feedback());

        assert_eq!(collector.buffered_count(), 2);
        assert_eq!(collector.total_processed(), 2);

        let drained = collector.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(collector.buffered_count(), 0);
    }

    #[test]
    fn feedback_collector_respects_max_buffer() {
        let mut collector = FeedbackCollector::with_buffer_size(3);
        for _ in 0..5 {
            collector.record(sample_feedback());
        }
        assert_eq!(collector.buffered_count(), 3);
        assert_eq!(collector.total_processed(), 5);
    }

    #[test]
    fn extract_rule_labels_maps_correctly() {
        let feedback = vec![sample_feedback()]; // rules_fired = [0, 2, 4]
        let labels = FeedbackCollector::extract_rule_labels(&feedback, 10);

        assert_eq!(labels.len(), 1);
        assert!(labels[0][0]);
        assert!(!labels[0][1]);
        assert!(labels[0][2]);
        assert!(!labels[0][3]);
        assert!(labels[0][4]);
    }

    #[test]
    fn system_fingerprint_serialization_roundtrip() {
        let fp = SystemFingerprint::default();
        let json = serde_json::to_string(&fp).unwrap();
        let deserialized: SystemFingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(fp.cpu_load_fraction, deserialized.cpu_load_fraction);
        assert_eq!(fp.capabilities, deserialized.capabilities);
        assert_eq!(fp.model_samples_trained, deserialized.model_samples_trained);
    }
}
