//! Adaptive cost model calibration from execution feedback.
//!
//! Adjusts cost model parameters based on observed discrepancies
//! between estimated and actual execution costs. Uses an
//! exponentially-weighted moving average (EWMA) to smooth parameter
//! updates and detect systematic bias.
//!
//! # Design
//!
//! The calibrator maintains per-operator correction factors that
//! multiply the base cost model output. When actual execution
//! consistently differs from estimates, the correction factors
//! converge toward the true ratio.
//!
//! Three-tier approach (from RFC Proposal 2):
//! 1. **Static calibration**: Hardware-based factors from
//!    [`CostCalibration`](crate::cost::CostCalibration)
//! 2. **Dynamic calibration**: Track actual vs estimated per query
//! 3. **Adaptive correction**: EWMA-based parameter adjustment
//!
//! # References
//! - Van Aken et al. "`OtterTune`" (2017)
//! - CMU 15-721 Lecture 18: Cost Models

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Minimum sample count before applying corrections.
const MIN_SAMPLES_FOR_CORRECTION: usize = 5;

/// Default EWMA smoothing factor (alpha). Higher values weight
/// recent observations more heavily. Range: (0.0, 1.0].
const DEFAULT_ALPHA: f64 = 0.2;

/// Bias threshold: systematic error must exceed this ratio before
/// corrections are applied. 1.2 means 20% consistent over/under.
const DEFAULT_BIAS_THRESHOLD: f64 = 1.2;

/// Operator categories for which we track separate calibration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperatorKind {
    /// Sequential scan.
    Scan,
    /// Filter / predicate evaluation.
    Filter,
    /// Hash join.
    HashJoin,
    /// Merge join.
    MergeJoin,
    /// Nested-loop join.
    NestedLoopJoin,
    /// Sort.
    Sort,
    /// Aggregate (hash or sort-based).
    Aggregate,
    /// Index scan / index-only scan.
    IndexScan,
}

impl fmt::Display for OperatorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Scan => "scan",
            Self::Filter => "filter",
            Self::HashJoin => "hash_join",
            Self::MergeJoin => "merge_join",
            Self::NestedLoopJoin => "nested_loop_join",
            Self::Sort => "sort",
            Self::Aggregate => "aggregate",
            Self::IndexScan => "index_scan",
        };
        f.write_str(label)
    }
}

impl OperatorKind {
    /// All operator kinds.
    pub const ALL: [Self; 8] = [
        Self::Scan,
        Self::Filter,
        Self::HashJoin,
        Self::MergeJoin,
        Self::NestedLoopJoin,
        Self::Sort,
        Self::Aggregate,
        Self::IndexScan,
    ];
}

/// EWMA state for a single operator's correction factor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorCalibration {
    /// Current EWMA of the ratio `actual_cost / estimated_cost`.
    pub correction_factor: f64,
    /// Number of observations incorporated.
    pub sample_count: usize,
    /// EWMA of the absolute Q-error (estimation quality metric).
    pub mean_q_error: f64,
    /// Whether systematic bias has been detected.
    pub bias_detected: bool,
}

impl Default for OperatorCalibration {
    fn default() -> Self {
        Self {
            correction_factor: 1.0,
            sample_count: 0,
            mean_q_error: 1.0,
            bias_detected: false,
        }
    }
}

/// A single feedback observation for calibration.
#[derive(Debug, Clone)]
pub struct CostFeedback {
    /// Which operator produced this feedback.
    pub operator: OperatorKind,
    /// Optimizer's estimated cost for this operator.
    pub estimated_cost: f64,
    /// Actual observed cost (wall-clock time in ms, or normalized).
    pub actual_cost: f64,
    /// Estimated row count.
    pub estimated_rows: f64,
    /// Actual row count.
    pub actual_rows: f64,
}

impl CostFeedback {
    /// Cost ratio: actual / estimated. Values > 1.0 mean
    /// the optimizer underestimated.
    #[must_use]
    pub fn cost_ratio(&self) -> f64 {
        let est = self.estimated_cost.max(1e-9);
        let act = self.actual_cost.max(1e-9);
        act / est
    }

    /// Q-error for row estimates.
    #[must_use]
    pub fn q_error(&self) -> f64 {
        let est = self.estimated_rows.max(1.0);
        let act = self.actual_rows.max(1.0);
        (est / act).max(act / est)
    }
}

/// Configuration for the adaptive calibrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationConfig {
    /// EWMA smoothing factor. Range: (0.0, 1.0].
    pub alpha: f64,
    /// Minimum samples before applying corrections.
    pub min_samples: usize,
    /// Bias detection threshold (ratio, e.g. 1.2 = 20%).
    pub bias_threshold: f64,
    /// Maximum correction factor (caps extreme adjustments).
    pub max_correction: f64,
    /// Minimum correction factor.
    pub min_correction: f64,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            alpha: DEFAULT_ALPHA,
            min_samples: MIN_SAMPLES_FOR_CORRECTION,
            bias_threshold: DEFAULT_BIAS_THRESHOLD,
            max_correction: 10.0,
            min_correction: 0.1,
        }
    }
}

/// Persistent calibration state for all operators.
///
/// Serializable to TOML for persistence across optimizer restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationState {
    /// Per-operator calibration data.
    pub operators: HashMap<OperatorKind, OperatorCalibration>,
    /// Total feedback observations processed.
    pub total_observations: u64,
    /// Configuration used for calibration.
    pub config: CalibrationConfig,
}

impl Default for CalibrationState {
    fn default() -> Self {
        let mut operators = HashMap::new();
        for kind in OperatorKind::ALL {
            operators.insert(kind, OperatorCalibration::default());
        }
        Self {
            operators,
            total_observations: 0,
            config: CalibrationConfig::default(),
        }
    }
}

impl CalibrationState {
    /// Create a new state with custom configuration.
    #[must_use]
    pub fn with_config(config: CalibrationConfig) -> Self {
        let mut operators = HashMap::new();
        for kind in OperatorKind::ALL {
            operators.insert(kind, OperatorCalibration::default());
        }
        Self {
            operators,
            total_observations: 0,
            config,
        }
    }

    /// Serialize to TOML string for persistence.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Deserialize from TOML string.
    ///
    /// # Errors
    /// Returns an error if the TOML is malformed or missing fields.
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }
}

/// Adaptive cost model calibrator.
///
/// Ingests execution feedback and maintains EWMA correction factors
/// per operator type. Correction factors are applied multiplicatively
/// to base cost estimates.
#[derive(Debug, Clone, Default)]
pub struct AdaptiveCalibrator {
    state: CalibrationState,
}

impl AdaptiveCalibrator {
    /// Create a calibrator with custom configuration.
    #[must_use]
    pub fn with_config(config: CalibrationConfig) -> Self {
        Self {
            state: CalibrationState::with_config(config),
        }
    }

    /// Restore from a previously persisted state.
    #[must_use]
    pub fn from_state(state: CalibrationState) -> Self {
        Self { state }
    }

    /// Get the current calibration state (for persistence).
    #[must_use]
    pub fn state(&self) -> &CalibrationState {
        &self.state
    }

    /// Get the correction factor for an operator kind.
    ///
    /// Returns 1.0 (no correction) until enough samples are
    /// collected and systematic bias is detected.
    #[must_use]
    pub fn correction_factor(&self, operator: OperatorKind) -> f64 {
        self.state
            .operators
            .get(&operator)
            .filter(|cal| cal.sample_count >= self.state.config.min_samples && cal.bias_detected)
            .map_or(1.0, |cal| cal.correction_factor)
    }

    /// Apply a cost estimate adjusted by the calibration factor.
    #[must_use]
    pub fn adjust_cost(&self, operator: OperatorKind, base_cost: f64) -> f64 {
        base_cost * self.correction_factor(operator)
    }

    /// Ingest a batch of feedback observations.
    ///
    /// Updates EWMA correction factors and detects bias.
    /// Returns the number of operators whose bias status changed.
    pub fn ingest(&mut self, feedback: &[CostFeedback]) -> usize {
        let alpha = self.state.config.alpha;
        let threshold = self.state.config.bias_threshold;
        let max_corr = self.state.config.max_correction;
        let min_corr = self.state.config.min_correction;

        let mut bias_changes = 0;

        for fb in feedback {
            self.state.total_observations += 1;

            let cal = self.state.operators.entry(fb.operator).or_default();

            let ratio = fb.cost_ratio();
            let q_err = fb.q_error();

            if cal.sample_count == 0 {
                cal.correction_factor = ratio;
                cal.mean_q_error = q_err;
            } else {
                cal.correction_factor = alpha * ratio + (1.0 - alpha) * cal.correction_factor;
                cal.mean_q_error = alpha * q_err + (1.0 - alpha) * cal.mean_q_error;
            }

            cal.correction_factor = cal.correction_factor.clamp(min_corr, max_corr);
            cal.sample_count += 1;

            let was_biased = cal.bias_detected;
            cal.bias_detected = cal.sample_count >= self.state.config.min_samples
                && (cal.correction_factor > threshold || cal.correction_factor < 1.0 / threshold);

            if cal.bias_detected != was_biased {
                bias_changes += 1;
            }
        }

        bias_changes
    }

    /// Reset calibration for a specific operator.
    pub fn reset_operator(&mut self, operator: OperatorKind) {
        self.state
            .operators
            .insert(operator, OperatorCalibration::default());
    }

    /// Reset all calibration state.
    pub fn reset_all(&mut self) {
        self.state = CalibrationState::with_config(self.state.config.clone());
    }

    /// Total feedback observations processed.
    #[must_use]
    pub fn total_observations(&self) -> u64 {
        self.state.total_observations
    }

    /// Summary of operators with detected bias.
    #[must_use]
    pub fn biased_operators(&self) -> Vec<(OperatorKind, f64)> {
        let mut result = Vec::new();
        for (kind, cal) in &self.state.operators {
            if cal.bias_detected {
                result.push((*kind, cal.correction_factor));
            }
        }
        result.sort_by_key(|(k, _)| *k as u8);
        result
    }
}

/// Parse an operator description string into an [`OperatorKind`].
///
/// Recognizes common PostgreSQL-style operator names from EXPLAIN
/// output (e.g. `SeqScan`, `Hash Join`, `Index Scan`).
#[must_use]
pub fn classify_operator(description: &str) -> Option<OperatorKind> {
    let lower = description.to_lowercase();
    if lower.contains("seq") && lower.contains("scan") {
        Some(OperatorKind::Scan)
    } else if lower.contains("index") && lower.contains("scan") {
        Some(OperatorKind::IndexScan)
    } else if lower.contains("hash") && lower.contains("join") {
        Some(OperatorKind::HashJoin)
    } else if lower.contains("merge") && lower.contains("join") {
        Some(OperatorKind::MergeJoin)
    } else if lower.contains("nested") && lower.contains("loop") {
        Some(OperatorKind::NestedLoopJoin)
    } else if lower.contains("sort") {
        Some(OperatorKind::Sort)
    } else if lower.contains("aggregate") || lower.contains("group") || lower.contains("hash agg") {
        Some(OperatorKind::Aggregate)
    } else if lower.contains("filter") {
        Some(OperatorKind::Filter)
    } else {
        None
    }
}

/// Convert [`ra_stats::timeline::ExecutionFeedback`] entries into
/// [`CostFeedback`] suitable for the adaptive calibrator.
///
/// Entries without an operator description or without cost/time data
/// are skipped.
#[must_use]
#[cfg(feature = "timeline")]
pub fn feedback_from_timeline(
    entries: &[ra_stats::timeline::ExecutionFeedback],
) -> Vec<CostFeedback> {
    let mut result = Vec::new();
    for entry in entries {
        let operator = entry.operator.as_deref().and_then(classify_operator);
        let Some(operator) = operator else {
            continue;
        };

        let estimated_cost = entry.estimated_cost.unwrap_or(0.0);
        let actual_cost = entry.actual_time_ms.unwrap_or(0.0);

        if estimated_cost <= 0.0 || actual_cost <= 0.0 {
            continue;
        }

        result.push(CostFeedback {
            operator,
            estimated_cost,
            actual_cost,
            estimated_rows: entry.estimated_rows,
            actual_rows: entry.actual_rows,
        });
    }
    result
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn scan_feedback(estimated_cost: f64, actual_cost: f64) -> CostFeedback {
        CostFeedback {
            operator: OperatorKind::Scan,
            estimated_cost,
            actual_cost,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
        }
    }

    fn join_feedback(estimated_cost: f64, actual_cost: f64) -> CostFeedback {
        CostFeedback {
            operator: OperatorKind::HashJoin,
            estimated_cost,
            actual_cost,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
        }
    }

    // -- CostFeedback --

    #[test]
    fn cost_ratio_underestimate() {
        let fb = CostFeedback {
            operator: OperatorKind::Scan,
            estimated_cost: 100.0,
            actual_cost: 300.0,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
        };
        assert!((fb.cost_ratio() - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_ratio_overestimate() {
        let fb = CostFeedback {
            operator: OperatorKind::Scan,
            estimated_cost: 300.0,
            actual_cost: 100.0,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
        };
        let ratio = fb.cost_ratio();
        assert!((ratio - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn cost_ratio_perfect() {
        let fb = scan_feedback(100.0, 100.0);
        assert!((fb.cost_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_ratio_near_zero_clamped() {
        let fb = CostFeedback {
            operator: OperatorKind::Scan,
            estimated_cost: 0.0,
            actual_cost: 0.0,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
        };
        let ratio = fb.cost_ratio();
        assert!(ratio.is_finite());
        assert!(ratio > 0.0);
    }

    #[test]
    fn q_error_perfect() {
        let fb = scan_feedback(100.0, 100.0);
        assert!((fb.q_error() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_overestimate() {
        let fb = CostFeedback {
            operator: OperatorKind::Scan,
            estimated_cost: 100.0,
            actual_cost: 100.0,
            estimated_rows: 2000.0,
            actual_rows: 1000.0,
        };
        assert!((fb.q_error() - 2.0).abs() < f64::EPSILON);
    }

    // -- OperatorCalibration defaults --

    #[test]
    fn default_operator_calibration() {
        let cal = OperatorCalibration::default();
        assert_eq!(cal.correction_factor, 1.0);
        assert_eq!(cal.sample_count, 0);
        assert_eq!(cal.mean_q_error, 1.0);
        assert!(!cal.bias_detected);
    }

    // -- CalibrationConfig --

    #[test]
    fn default_config_values() {
        let cfg = CalibrationConfig::default();
        assert!((cfg.alpha - 0.2).abs() < f64::EPSILON);
        assert_eq!(cfg.min_samples, 5);
        assert!((cfg.bias_threshold - 1.2).abs() < f64::EPSILON);
        assert!((cfg.max_correction - 10.0).abs() < f64::EPSILON);
        assert!((cfg.min_correction - 0.1).abs() < f64::EPSILON);
    }

    // -- AdaptiveCalibrator basics --

    #[test]
    fn default_calibrator_no_correction() {
        let cal = AdaptiveCalibrator::default();
        for kind in OperatorKind::ALL {
            assert!((cal.correction_factor(kind) - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn adjust_cost_no_bias() {
        let cal = AdaptiveCalibrator::default();
        let adjusted = cal.adjust_cost(OperatorKind::Scan, 100.0);
        assert!((adjusted - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn total_observations_starts_zero() {
        let cal = AdaptiveCalibrator::default();
        assert_eq!(cal.total_observations(), 0);
    }

    // -- Ingestion --

    #[test]
    fn ingest_single_observation() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback = [scan_feedback(100.0, 200.0)];
        cal.ingest(&feedback);
        assert_eq!(cal.total_observations(), 1);
    }

    #[test]
    fn ingest_below_min_samples_no_correction() {
        let mut cal = AdaptiveCalibrator::default();
        // 4 observations < min_samples(5)
        let feedback: Vec<CostFeedback> = (0..4).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);
        assert!((cal.correction_factor(OperatorKind::Scan) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ingest_enough_samples_detects_bias() {
        let mut cal = AdaptiveCalibrator::default();
        // cost ratio = 3.0 consistently, well above threshold 1.2
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);

        let factor = cal.correction_factor(OperatorKind::Scan);
        assert!(factor > 1.0, "should detect underestimate bias");

        let biased = cal.biased_operators();
        assert!(biased.iter().any(|(k, _)| *k == OperatorKind::Scan));
    }

    #[test]
    fn ingest_perfect_estimates_no_bias() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 100.0)).collect();
        cal.ingest(&feedback);

        assert!(!cal
            .state
            .operators
            .get(&OperatorKind::Scan)
            .map_or(false, |c| c.bias_detected));
    }

    #[test]
    fn ingest_overestimate_bias() {
        let mut cal = AdaptiveCalibrator::default();
        // actual << estimated => correction factor < 1.0
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(300.0, 100.0)).collect();
        cal.ingest(&feedback);

        let factor = cal.correction_factor(OperatorKind::Scan);
        assert!(factor < 1.0, "correction should be < 1 for overestimates");
    }

    #[test]
    fn correction_factor_clamped_high() {
        let mut cal = AdaptiveCalibrator::default();
        // extreme underestimate: actual = 10000x estimated
        let feedback: Vec<CostFeedback> = (0..20).map(|_| scan_feedback(1.0, 100_000.0)).collect();
        cal.ingest(&feedback);

        let factor = cal.correction_factor(OperatorKind::Scan);
        assert!(factor <= 10.0, "should be clamped to max_correction");
    }

    #[test]
    fn correction_factor_clamped_low() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..20).map(|_| scan_feedback(100_000.0, 1.0)).collect();
        cal.ingest(&feedback);

        let factor = cal
            .state
            .operators
            .get(&OperatorKind::Scan)
            .map_or(1.0, |c| c.correction_factor);
        assert!(factor >= 0.1, "should be clamped to min_correction");
    }

    // -- EWMA convergence --

    #[test]
    fn ewma_converges_toward_ratio() {
        let mut cal = AdaptiveCalibrator::default();
        // Feed consistent 2x underestimate
        for _ in 0..50 {
            cal.ingest(&[scan_feedback(100.0, 200.0)]);
        }
        let factor = cal
            .state
            .operators
            .get(&OperatorKind::Scan)
            .map_or(1.0, |c| c.correction_factor);
        // Should converge close to 2.0
        assert!(
            (factor - 2.0).abs() < 0.1,
            "EWMA should converge to 2.0, got {factor}"
        );
    }

    // -- Multiple operators --

    #[test]
    fn independent_operator_calibration() {
        let mut cal = AdaptiveCalibrator::default();
        let scan_fb: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 300.0)).collect();
        let join_fb: Vec<CostFeedback> = (0..10).map(|_| join_feedback(100.0, 100.0)).collect();

        cal.ingest(&scan_fb);
        cal.ingest(&join_fb);

        let scan_factor = cal.correction_factor(OperatorKind::Scan);
        let join_factor = cal.correction_factor(OperatorKind::HashJoin);

        assert!(scan_factor > 1.0);
        assert!(
            (join_factor - 1.0).abs() < f64::EPSILON,
            "join should have no bias"
        );
    }

    // -- Reset --

    #[test]
    fn reset_operator_clears_state() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);

        cal.reset_operator(OperatorKind::Scan);
        let factor = cal
            .state
            .operators
            .get(&OperatorKind::Scan)
            .map_or(1.0, |c| c.correction_factor);
        assert!((factor - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_all_clears_everything() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);

        cal.reset_all();
        assert_eq!(cal.total_observations(), 0);
        assert!(cal.biased_operators().is_empty());
    }

    // -- Persistence --

    #[test]
    fn roundtrip_toml_serialization() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 200.0)).collect();
        cal.ingest(&feedback);

        let toml_str = cal.state().to_toml().expect("serialization failed");
        let restored = CalibrationState::from_toml(&toml_str).expect("deserialization failed");

        assert_eq!(restored.total_observations, cal.state().total_observations);
        let orig = cal
            .state()
            .operators
            .get(&OperatorKind::Scan)
            .expect("scan should exist");
        let rest = restored
            .operators
            .get(&OperatorKind::Scan)
            .expect("scan should exist");
        assert!((orig.correction_factor - rest.correction_factor).abs() < f64::EPSILON);
        assert_eq!(orig.sample_count, rest.sample_count);
    }

    #[test]
    fn from_state_preserves_calibration() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);

        let state = cal.state().clone();
        let restored = AdaptiveCalibrator::from_state(state);

        assert_eq!(restored.total_observations(), cal.total_observations());
        assert!(
            (restored.correction_factor(OperatorKind::Scan)
                - cal.correction_factor(OperatorKind::Scan))
            .abs()
                < f64::EPSILON
        );
    }

    // -- classify_operator --

    #[test]
    fn classify_seq_scan() {
        assert_eq!(
            classify_operator("SeqScan on lineitem"),
            Some(OperatorKind::Scan)
        );
    }

    #[test]
    fn classify_index_scan() {
        assert_eq!(
            classify_operator("Index Scan on orders_pkey"),
            Some(OperatorKind::IndexScan)
        );
    }

    #[test]
    fn classify_hash_join() {
        assert_eq!(classify_operator("Hash Join"), Some(OperatorKind::HashJoin));
    }

    #[test]
    fn classify_merge_join() {
        assert_eq!(
            classify_operator("Merge Join"),
            Some(OperatorKind::MergeJoin)
        );
    }

    #[test]
    fn classify_nested_loop() {
        assert_eq!(
            classify_operator("Nested Loop"),
            Some(OperatorKind::NestedLoopJoin)
        );
    }

    #[test]
    fn classify_sort() {
        assert_eq!(classify_operator("Sort"), Some(OperatorKind::Sort));
    }

    #[test]
    fn classify_aggregate() {
        assert_eq!(
            classify_operator("HashAggregate"),
            Some(OperatorKind::Aggregate)
        );
    }

    #[test]
    fn classify_unknown_operator() {
        assert_eq!(classify_operator("Materialize"), None);
    }

    // -- feedback_from_timeline --

    #[test]
    #[cfg(feature = "timeline")]
    fn timeline_feedback_skips_missing_operator() {
        let entries = [ra_stats::timeline::ExecutionFeedback {
            time_offset: 0,
            query: "SELECT 1".to_string(),
            operator: None,
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
            estimated_cost: Some(100.0),
            actual_time_ms: Some(200.0),
        }];
        let result = feedback_from_timeline(&entries);
        assert!(result.is_empty());
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn timeline_feedback_skips_zero_cost() {
        let entries = [ra_stats::timeline::ExecutionFeedback {
            time_offset: 0,
            query: "SELECT 1".to_string(),
            operator: Some("SeqScan on t".to_string()),
            estimated_rows: 1000.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        }];
        let result = feedback_from_timeline(&entries);
        assert!(result.is_empty());
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn timeline_feedback_converts_valid_entry() {
        let entries = [ra_stats::timeline::ExecutionFeedback {
            time_offset: 0,
            query: "SELECT * FROM orders".to_string(),
            operator: Some("SeqScan on orders".to_string()),
            estimated_rows: 1000.0,
            actual_rows: 500.0,
            estimated_cost: Some(100.0),
            actual_time_ms: Some(200.0),
        }];
        let result = feedback_from_timeline(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].operator, OperatorKind::Scan);
        assert!((result[0].estimated_cost - 100.0).abs() < f64::EPSILON);
        assert!((result[0].actual_cost - 200.0).abs() < f64::EPSILON);
    }

    // -- adjust_cost with bias --

    #[test]
    fn adjust_cost_with_detected_bias() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);

        let adjusted = cal.adjust_cost(OperatorKind::Scan, 100.0);
        assert!(adjusted > 100.0, "adjusted cost should be higher than base");
    }

    // -- biased_operators --

    #[test]
    fn biased_operators_empty_initially() {
        let cal = AdaptiveCalibrator::default();
        assert!(cal.biased_operators().is_empty());
    }

    #[test]
    fn biased_operators_returns_only_biased() {
        let mut cal = AdaptiveCalibrator::default();
        let feedback: Vec<CostFeedback> = (0..10).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);

        let biased = cal.biased_operators();
        assert_eq!(biased.len(), 1);
        assert_eq!(biased[0].0, OperatorKind::Scan);
    }

    // -- OperatorKind Display --

    #[test]
    fn operator_kind_display() {
        assert_eq!(OperatorKind::Scan.to_string(), "scan");
        assert_eq!(OperatorKind::HashJoin.to_string(), "hash_join");
        assert_eq!(OperatorKind::Sort.to_string(), "sort");
    }

    // -- Custom config --

    #[test]
    fn custom_config_higher_alpha() {
        let config = CalibrationConfig {
            alpha: 0.5,
            min_samples: 3,
            ..CalibrationConfig::default()
        };
        let mut cal = AdaptiveCalibrator::with_config(config);
        let feedback: Vec<CostFeedback> = (0..5).map(|_| scan_feedback(100.0, 300.0)).collect();
        cal.ingest(&feedback);

        // Higher alpha => converges faster toward 3.0
        let factor = cal
            .state
            .operators
            .get(&OperatorKind::Scan)
            .map_or(1.0, |c| c.correction_factor);
        assert!(factor > 2.0);
    }

    // -- Ingest returns bias change count --

    #[test]
    fn ingest_returns_bias_changes() {
        let mut cal = AdaptiveCalibrator::default();
        // First 4 samples: no bias detected (below min_samples)
        let fb4: Vec<CostFeedback> = (0..4).map(|_| scan_feedback(100.0, 300.0)).collect();
        let changes = cal.ingest(&fb4);
        assert_eq!(changes, 0);

        // 5th sample: bias now detected
        let fb1 = [scan_feedback(100.0, 300.0)];
        let changes = cal.ingest(&fb1);
        assert_eq!(changes, 1);

        // Further samples: no change in bias status
        let changes = cal.ingest(&fb1);
        assert_eq!(changes, 0);
    }
}
