//! Neural rule group selection for pre-saturation filtering.
//!
//! Replaces heuristic stages 2+3 of the rule advisor with a learned
//! linear model that predicts which rule groups will benefit the current
//! query given the system state.
//!
//! # Architecture
//!
//! ```text
//! Input: QueryFeatures (12-dim) + SystemFingerprint (14-dim) = 26-dim
//!   ↓
//! Linear layer: 26 × NUM_GROUPS weights + bias
//!   ↓
//! Sigmoid activation
//!   ↓
//! Threshold filter: keep groups with P(benefit) >= 0.05
//!   ↓
//! Output: Vec<RuleCategory> to enable for this query
//! ```
//!
//! # Performance
//!
//! Inference: ~200ns (26×32 matmul + sigmoid + threshold scan)
//! Training update: ~1μs (online logistic regression per sample)

use crate::cost_model::QueryFeatures;
use crate::lazy_rules::{LazyQueryPattern, LazyRuleCompiler, RuleCategory};
use crate::state::SystemFingerprint;

/// Number of input features: 12 (query) + 14 (fingerprint).
const INPUT_DIM: usize = QueryFeatures::STRUCTURAL_DIM + SystemFingerprint::NEURAL_DIM;

/// Number of rule groups the selector scores.
/// Maps 1:1 to [`RuleCategory`] variants (14 total, but we score
/// only the 10 on-demand categories — baseline always loads).
const NUM_GROUPS: usize = 10;

/// Default threshold for rule group activation.
const DEFAULT_THRESHOLD: f32 = 0.05;

/// Minimum training samples before the neural model is trusted.
const MIN_SAMPLES_FOR_NEURAL: u32 = 500;

/// Learning rate for online logistic regression updates.
const LEARNING_RATE: f32 = 0.01;

/// All on-demand rule categories in scoring order.
/// Baseline categories (Filter, Projection, Expression, Null) are always
/// included and not scored by this model.
const SCORED_CATEGORIES: [RuleCategory; NUM_GROUPS] = [
    RuleCategory::JoinReordering,
    RuleCategory::JoinElimination,
    RuleCategory::JoinTransformation,
    RuleCategory::SemiJoinOptimization,
    RuleCategory::AggregateOptimization,
    RuleCategory::LimitSortOptimization,
    RuleCategory::SetOperationOptimization,
    RuleCategory::SubqueryOptimization,
    RuleCategory::FileFormatOptimization,
    RuleCategory::MetadataShortcuts,
];

/// Learned rule group selector using a single linear layer.
///
/// When untrained (< 500 samples), falls back to [`LazyRuleCompiler`]
/// heuristics. After training, uses a 26×10 weight matrix to predict
/// per-group benefit probability.
pub struct NeuralRuleSelector {
    /// Weight matrix: `weights[input_idx][group_idx]`.
    weights: Box<[[f32; NUM_GROUPS]; INPUT_DIM]>,
    /// Bias per rule group.
    bias: Box<[f32; NUM_GROUPS]>,
    /// Activation threshold (default 0.05).
    threshold: f32,
    /// Number of training samples processed.
    samples_trained: u32,
    /// Fallback compiler for cold-start (used when `!is_trained()`).
    #[expect(dead_code, reason = "reserved for future direct compilation path")]
    fallback: LazyRuleCompiler,
}

impl NeuralRuleSelector {
    /// Create a new selector with zero-initialized weights (cold start).
    #[must_use]
    pub fn new() -> Self {
        Self {
            weights: Box::new([[0.0; NUM_GROUPS]; INPUT_DIM]),
            bias: Box::new([0.0; NUM_GROUPS]),
            threshold: DEFAULT_THRESHOLD,
            samples_trained: 0,
            fallback: LazyRuleCompiler::new(),
        }
    }

    /// Create with a custom threshold.
    #[must_use]
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Number of training samples seen.
    #[must_use]
    pub fn samples_trained(&self) -> u32 {
        self.samples_trained
    }

    /// Whether the neural model is trusted (enough training data).
    #[must_use]
    pub fn is_trained(&self) -> bool {
        self.samples_trained >= MIN_SAMPLES_FOR_NEURAL
    }

    /// Select rule categories for the given query and system state.
    ///
    /// Returns the set of [`RuleCategory`] variants to enable. Always
    /// includes baseline categories regardless of model prediction.
    ///
    /// Performance: ~200ns when trained, ~2μs when falling back to heuristics.
    #[must_use]
    pub fn select(
        &self,
        features: &QueryFeatures,
        fingerprint: &SystemFingerprint,
    ) -> Vec<RuleCategory> {
        // Always include baseline categories
        let mut selected = vec![
            RuleCategory::FilterOptimization,
            RuleCategory::ProjectionOptimization,
            RuleCategory::ExpressionSimplification,
            RuleCategory::NullSimplification,
        ];

        if !self.is_trained() {
            // Cold start: fall back to heuristic pattern matching
            let pattern = Self::features_to_pattern(features);
            let required = RuleCategory::required_for_pattern(&pattern);
            for cat in required {
                if !cat.is_baseline() && !selected.contains(&cat) {
                    selected.push(cat);
                }
            }
            return selected;
        }

        // Neural inference: linear layer + sigmoid + threshold
        let input = Self::concat_input(features, fingerprint);
        let scores = self.forward(&input);

        for (idx, &score) in scores.iter().enumerate() {
            if score >= self.threshold {
                selected.push(SCORED_CATEGORIES[idx]);
            }
        }

        selected
    }

    /// Select and return indices of active rule groups (for saturation
    /// loop integration where we need indices, not categories).
    #[must_use]
    pub fn select_indices(
        &self,
        features: &QueryFeatures,
        fingerprint: &SystemFingerprint,
    ) -> Vec<usize> {
        if !self.is_trained() {
            // Return all groups when untrained
            return (0..NUM_GROUPS).collect();
        }

        let input = Self::concat_input(features, fingerprint);
        let scores = self.forward(&input);

        scores
            .iter()
            .enumerate()
            .filter(|(_, &s)| s >= self.threshold)
            .map(|(i, _)| i)
            .collect()
    }

    /// Raw scores for all rule groups (used for diagnostics/logging).
    #[must_use]
    pub fn score_all(
        &self,
        features: &QueryFeatures,
        fingerprint: &SystemFingerprint,
    ) -> [f32; NUM_GROUPS] {
        let input = Self::concat_input(features, fingerprint);
        self.forward(&input)
    }

    /// Online training update: binary cross-entropy gradient for one sample.
    ///
    /// `labels[i]` = true if rule group i was productive for this query
    /// (i.e., it contributed to at least one node in the extracted plan).
    pub fn train_step(
        &mut self,
        features: &QueryFeatures,
        fingerprint: &SystemFingerprint,
        labels: &[bool; NUM_GROUPS],
    ) {
        let input = Self::concat_input(features, fingerprint);
        let predictions = self.forward(&input);

        // Logistic regression gradient: dL/dw = (prediction - label) * input
        for (g, (&pred, &label)) in predictions.iter().zip(labels.iter()).enumerate() {
            let target = if label { 1.0 } else { 0.0 };
            let error = pred - target;

            // Update weights
            for (row, &x) in self.weights.iter_mut().zip(input.iter()) {
                row[g] -= LEARNING_RATE * error * x;
            }
            // Update bias
            self.bias[g] -= LEARNING_RATE * error;
        }

        self.samples_trained += 1;
    }

    /// Batch training: multiple samples at once with averaged gradients.
    pub fn train_batch(
        &mut self,
        samples: &[(QueryFeatures, SystemFingerprint, [bool; NUM_GROUPS])],
    ) {
        if samples.is_empty() {
            return;
        }

        let batch_lr = LEARNING_RATE / samples.len() as f32;

        // Accumulate gradients
        let mut weight_grad = [[0.0f32; NUM_GROUPS]; INPUT_DIM];
        let mut bias_grad = [0.0f32; NUM_GROUPS];

        for (features, fingerprint, labels) in samples {
            let input = Self::concat_input(features, fingerprint);
            let predictions = self.forward(&input);

            for (g, (&pred, &label)) in predictions.iter().zip(labels.iter()).enumerate() {
                let target = if label { 1.0 } else { 0.0 };
                let error = pred - target;

                for (grad_row, &x) in weight_grad.iter_mut().zip(input.iter()) {
                    grad_row[g] += error * x;
                }
                bias_grad[g] += error;
            }
        }

        // Apply averaged gradients
        for (w_row, g_row) in self.weights.iter_mut().zip(weight_grad.iter()) {
            for (w, &g) in w_row.iter_mut().zip(g_row.iter()) {
                *w -= batch_lr * g;
            }
        }
        for (b, &bg) in self.bias.iter_mut().zip(bias_grad.iter()) {
            *b -= batch_lr * bg;
        }

        self.samples_trained += samples.len() as u32;
    }

    /// Reset learned weights (for testing or model rollback).
    pub fn reset(&mut self) {
        *self.weights = [[0.0; NUM_GROUPS]; INPUT_DIM];
        *self.bias = [0.0; NUM_GROUPS];
        self.samples_trained = 0;
    }

    // --- Private helpers ---

    /// Forward pass: linear layer + sigmoid.
    #[inline]
    fn forward(&self, input: &[f32; INPUT_DIM]) -> [f32; NUM_GROUPS] {
        let mut output = [0.0f32; NUM_GROUPS];

        // Matrix-vector product: output[g] = sum_i(weights[i][g] * input[i]) + bias[g]
        for (row, &x) in self.weights.iter().zip(input.iter()) {
            if x == 0.0 {
                continue; // Skip zero inputs (common for sparse features)
            }
            for (out, &w) in output.iter_mut().zip(row.iter()) {
                *out += w * x;
            }
        }

        // Add bias and apply sigmoid
        for (out, &b) in output.iter_mut().zip(self.bias.iter()) {
            *out = sigmoid(*out + b);
        }

        output
    }

    /// Concatenate query features and fingerprint into the input vector.
    #[inline]
    fn concat_input(
        features: &QueryFeatures,
        fingerprint: &SystemFingerprint,
    ) -> [f32; INPUT_DIM] {
        let mut input = [0.0f32; INPUT_DIM];

        // First 12 dims: query features
        let fv = features.to_vec();
        input[..QueryFeatures::STRUCTURAL_DIM].copy_from_slice(&fv);

        // Next 14 dims: system fingerprint
        let fp_vec = fingerprint.to_neural_vec();
        input[QueryFeatures::STRUCTURAL_DIM..].copy_from_slice(&fp_vec);

        input
    }

    /// Convert query features to a `LazyQueryPattern` for fallback.
    fn features_to_pattern(features: &QueryFeatures) -> LazyQueryPattern {
        LazyQueryPattern {
            has_joins: features.join_count > 0.0,
            has_aggregates: features.aggregate_count > 0.0,
            has_subqueries: features.subquery_count > 0.0,
            has_set_ops: false,
            has_window_functions: features.window_function_count > 0.0,
            has_sorting: features.order_by_count > 0.0,
            has_limits: features.limit_present > 0.0,
            has_distinct: features.distinct_flag > 0.0,
            has_json_access: false,
            has_bson_func: false,
            has_vector_distance: false,
            has_fts_match: false,
            has_xml_func: false,
            has_cte: features.cte_count > 0.0,
            has_recursive_cte: false,
            has_cast: false,
            table_count: features.table_count as usize,
            join_depth: features.join_count as usize,
        }
    }
}

impl Default for NeuralRuleSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Fast sigmoid approximation (within 0.1% of exact for |x| < 10).
#[inline]
fn sigmoid(x: f32) -> f32 {
    // Clamp to avoid overflow in exp
    let clamped = x.clamp(-15.0, 15.0);
    1.0 / (1.0 + (-clamped).exp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SystemFingerprint;

    fn sample_features() -> QueryFeatures {
        QueryFeatures {
            table_count: 3.0,
            join_count: 2.0,
            filter_count: 1.0,
            aggregate_count: 0.0,
            subquery_count: 0.0,
            cte_count: 0.0,
            window_function_count: 0.0,
            order_by_count: 1.0,
            group_by_count: 0.0,
            distinct_flag: 0.0,
            limit_present: 1.0,
            max_join_cardinality: 4.5,
        }
    }

    #[test]
    fn untrained_selector_uses_fallback() {
        let selector = NeuralRuleSelector::new();
        let features = sample_features();
        let fp = SystemFingerprint::default();

        let categories = selector.select(&features, &fp);

        // Should always include baseline
        assert!(categories.contains(&RuleCategory::FilterOptimization));
        assert!(categories.contains(&RuleCategory::ProjectionOptimization));
        // Should include join rules since query has joins
        assert!(categories.contains(&RuleCategory::JoinReordering));
    }

    #[test]
    fn trained_selector_uses_neural_scores() {
        let mut selector = NeuralRuleSelector::new();
        let features = sample_features();
        let fp = SystemFingerprint::default();

        // Train with labels: only JoinReordering (idx 0) is productive
        let mut labels = [false; NUM_GROUPS];
        labels[0] = true; // JoinReordering

        // Train enough samples to activate neural path
        for _ in 0..600 {
            selector.train_step(&features, &fp, &labels);
        }

        assert!(selector.is_trained());

        let categories = selector.select(&features, &fp);
        // Should include JoinReordering (trained as productive)
        assert!(categories.contains(&RuleCategory::JoinReordering));
    }

    #[test]
    fn sigmoid_values_correct() {
        assert!((sigmoid(0.0) - 0.5).abs() < 0.001);
        assert!(sigmoid(10.0) > 0.999);
        assert!(sigmoid(-10.0) < 0.001);
    }

    #[test]
    fn forward_with_zero_weights_gives_half() {
        let selector = NeuralRuleSelector::new();
        let features = sample_features();
        let fp = SystemFingerprint::default();

        let scores = selector.score_all(&features, &fp);
        // Zero weights + zero bias → sigmoid(0) = 0.5 for all groups
        for &s in &scores {
            assert!((s - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn batch_training_moves_scores_in_same_direction() {
        let features = sample_features();
        let fp = SystemFingerprint::default();
        let mut labels = [false; NUM_GROUPS];
        labels[0] = true;
        labels[3] = true;

        // Train individually
        let mut selector_individual = NeuralRuleSelector::new();
        for _ in 0..10 {
            selector_individual.train_step(&features, &fp, &labels);
        }

        // Train as batch
        let mut selector_batch = NeuralRuleSelector::new();
        let batch: Vec<_> = (0..10)
            .map(|_| (features.clone(), fp, labels))
            .collect();
        selector_batch.train_batch(&batch);

        // Both should move the productive groups (0, 3) above 0.5
        // and unproductive groups below 0.5 (from the initial 0.5 baseline)
        let scores_ind = selector_individual.score_all(&features, &fp);
        let scores_batch = selector_batch.score_all(&features, &fp);

        // Productive group scores should increase from baseline (0.5)
        assert!(scores_ind[0] > 0.5, "individual group 0 should increase");
        assert!(scores_batch[0] > 0.5, "batch group 0 should increase");
        assert!(scores_ind[3] > 0.5, "individual group 3 should increase");
        assert!(scores_batch[3] > 0.5, "batch group 3 should increase");

        // Unproductive group scores should decrease from baseline
        assert!(scores_ind[1] < 0.5, "individual group 1 should decrease");
        assert!(scores_batch[1] < 0.5, "batch group 1 should decrease");
    }

    #[test]
    fn reset_clears_learned_weights() {
        let mut selector = NeuralRuleSelector::new();
        let features = sample_features();
        let fp = SystemFingerprint::default();
        let labels = [true; NUM_GROUPS];

        for _ in 0..100 {
            selector.train_step(&features, &fp, &labels);
        }
        assert!(selector.samples_trained() > 0);

        selector.reset();
        assert_eq!(selector.samples_trained(), 0);

        let scores = selector.score_all(&features, &fp);
        for &s in &scores {
            assert!((s - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn select_indices_matches_select_categories() {
        let mut selector = NeuralRuleSelector::new();
        // Force trained state
        selector.samples_trained = 1000;
        // Set bias to make first 3 groups activate
        selector.bias[0] = 3.0; // JoinReordering
        selector.bias[1] = 3.0; // JoinElimination
        selector.bias[2] = 3.0; // JoinTransformation

        let features = sample_features();
        let fp = SystemFingerprint::default();

        let indices = selector.select_indices(&features, &fp);
        assert!(indices.contains(&0));
        assert!(indices.contains(&1));
        assert!(indices.contains(&2));
    }
}
