//! Continuation gate for adaptive e-graph termination.
//!
//! Decides whether to continue e-graph iterations based on observed
//! cost improvement trajectory. Called every N iterations to amortize
//! the cost of plan extraction.
//!
//! Two termination signals:
//! 1. **Cost stagnation**: If cost hasn't improved >0.1% in 2 checks, stop.
//! 2. **Model-based**: `BitNet` predicts probability of meaningful improvement
//!    in the next N iterations; stop if probability < 0.3.

use std::sync::Arc;

use crate::cost_model::BitNetCostModel;
use crate::speculative_router::OptimizationFeatures;

/// Decides whether to continue e-graph iterations.
///
/// Called every `check_interval` iterations to amortize extraction cost.
pub struct ContinuationGate {
    model: Option<Arc<BitNetCostModel>>,
    base_features: OptimizationFeatures,
    cost_history: Vec<f64>,
    check_interval: usize,
    stagnation_threshold: f64,
    continuation_threshold: f32,
}

/// Result of a continuation check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationDecision {
    /// Continue iterating.
    Continue,
    /// Stop: cost stagnation detected.
    StopCostStagnant,
    /// Stop: model predicts no meaningful improvement.
    StopModelPrediction,
}

impl ContinuationGate {
    /// Create a new gate with default settings.
    ///
    /// - `check_interval`: Check every N iterations (default: 2)
    /// - `stagnation_threshold`: Stop if improvement < this (default: 0.001 = 0.1%)
    /// - `continuation_threshold`: Stop if model confidence < this (default: 0.3)
    #[must_use]
    pub fn new(
        base_features: OptimizationFeatures,
        model: Option<Arc<BitNetCostModel>>,
    ) -> Self {
        Self {
            model,
            base_features,
            cost_history: Vec::with_capacity(32),
            check_interval: 2,
            stagnation_threshold: 0.001,
            continuation_threshold: 0.3,
        }
    }

    /// Create with custom check interval.
    #[must_use]
    pub fn with_check_interval(mut self, interval: usize) -> Self {
        self.check_interval = interval.max(1);
        self
    }

    /// Create with custom stagnation threshold.
    #[must_use]
    pub fn with_stagnation_threshold(mut self, threshold: f64) -> Self {
        self.stagnation_threshold = threshold.max(0.0);
        self
    }

    /// Should we continue iterating?
    ///
    /// Returns `Continue` during non-check iterations (when `iteration %
    /// check_interval != 0`). On check iterations, evaluates cost
    /// trajectory and optionally queries the model.
    pub fn should_continue(
        &mut self,
        iteration: usize,
        current_cost: f64,
        egraph_nodes: usize,
    ) -> ContinuationDecision {
        self.cost_history.push(current_cost);

        // Only check at intervals (and after at least 2 data points)
        if iteration < self.check_interval || !iteration.is_multiple_of(self.check_interval) {
            return ContinuationDecision::Continue;
        }

        // Need at least check_interval+1 data points to compare
        if self.cost_history.len() < self.check_interval + 1 {
            return ContinuationDecision::Continue;
        }

        // Fast check: cost improvement over the last check_interval iterations
        let prev_idx = self.cost_history.len() - self.check_interval - 1;
        let prev_cost = self.cost_history[prev_idx];

        if prev_cost <= 0.0 || !prev_cost.is_finite() {
            return ContinuationDecision::Continue;
        }

        let improvement = (prev_cost - current_cost) / prev_cost;

        // If cost hasn't improved at all (or got worse), stop
        if improvement < self.stagnation_threshold {
            return ContinuationDecision::StopCostStagnant;
        }

        // Model-based check: predict expected improvement of next iterations
        if let Some(ref model) = self.model {
            let continuation_features = self.build_continuation_features(
                iteration,
                current_cost,
                improvement,
                egraph_nodes,
            );
            let prediction = model.predict_cpu_ms(&continuation_features);
            // Interpret as probability of meaningful improvement (sigmoid-like)
            let continue_prob = 1.0 / (1.0 + (-prediction).exp());
            if continue_prob < self.continuation_threshold {
                return ContinuationDecision::StopModelPrediction;
            }
        }

        ContinuationDecision::Continue
    }

    /// Build extended features for continuation prediction.
    ///
    /// Packs base features + iteration context into 12D input.
    fn build_continuation_features(
        &self,
        iteration: usize,
        current_cost: f64,
        improvement_rate: f64,
        egraph_nodes: usize,
    ) -> [f32; 12] {
        [
            self.base_features.table_count,
            self.base_features.join_count,
            self.base_features.filter_count,
            self.base_features.join_graph_density,
            self.base_features.equi_join_fraction,
            self.base_features.cross_join_present,
            iteration as f32 / 20.0,               // normalized iteration
            (current_cost as f32).log2().max(0.0),  // log cost
            improvement_rate as f32,                // recent improvement
            (egraph_nodes as f32).log2().max(0.0),  // log graph size
            self.base_features.avg_predicate_selectivity,
            self.base_features.has_limit,
        ]
    }

    /// Number of costs recorded.
    #[must_use]
    pub fn history_len(&self) -> usize {
        self.cost_history.len()
    }

    /// Get the cost improvement trajectory (for diagnostics).
    #[must_use]
    pub fn cost_history(&self) -> &[f64] {
        &self.cost_history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_features() -> OptimizationFeatures {
        OptimizationFeatures {
            table_count: 4.0,
            join_count: 3.0,
            filter_count: 2.0,
            aggregate_count: 1.0,
            subquery_count: 0.0,
            window_count: 0.0,
            join_graph_density: 0.5,
            max_join_fan_out: 2.0,
            equi_join_fraction: 1.0,
            cross_join_present: 0.0,
            avg_predicate_selectivity: 0.01,
            has_limit: 0.0,
            has_distinct_or_group: 1.0,
            log_estimated_rows: 5.0,
            total_table_pages: 100.0,
            index_coverage: 0.5,
        }
    }

    #[test]
    fn continues_on_non_check_iterations() {
        let mut gate = ContinuationGate::new(test_features(), None);
        // Iteration 0 and 1 should always continue
        assert_eq!(
            gate.should_continue(0, 100.0, 500),
            ContinuationDecision::Continue
        );
        assert_eq!(
            gate.should_continue(1, 95.0, 600),
            ContinuationDecision::Continue
        );
    }

    #[test]
    fn stops_on_cost_stagnation() {
        let mut gate = ContinuationGate::new(test_features(), None)
            .with_check_interval(2);

        // Simulate iterations with no improvement
        gate.should_continue(0, 100.0, 500);
        gate.should_continue(1, 100.0, 600);
        let decision = gate.should_continue(2, 100.0, 700);
        assert_eq!(decision, ContinuationDecision::StopCostStagnant);
    }

    #[test]
    fn continues_with_improvement() {
        let mut gate = ContinuationGate::new(test_features(), None)
            .with_check_interval(2);

        // Simulate iterations with meaningful improvement
        gate.should_continue(0, 100.0, 500);
        gate.should_continue(1, 90.0, 600);
        let decision = gate.should_continue(2, 80.0, 700);
        assert_eq!(decision, ContinuationDecision::Continue);
    }

    #[test]
    fn respects_check_interval() {
        let mut gate = ContinuationGate::new(test_features(), None)
            .with_check_interval(3);

        // No checks until iteration 3
        gate.should_continue(0, 100.0, 500);
        gate.should_continue(1, 100.0, 500);
        let decision = gate.should_continue(2, 100.0, 500);
        // Iteration 2 is not a check point for interval=3
        assert_eq!(decision, ContinuationDecision::Continue);

        let decision = gate.should_continue(3, 100.0, 500);
        assert_eq!(decision, ContinuationDecision::StopCostStagnant);
    }

    #[test]
    fn threshold_sensitivity() {
        let mut gate = ContinuationGate::new(test_features(), None)
            .with_check_interval(2)
            .with_stagnation_threshold(0.05); // 5% required improvement

        gate.should_continue(0, 100.0, 500);
        gate.should_continue(1, 98.0, 600);
        // 2% improvement is below 5% threshold
        let decision = gate.should_continue(2, 98.0, 700);
        assert_eq!(decision, ContinuationDecision::StopCostStagnant);
    }
}
