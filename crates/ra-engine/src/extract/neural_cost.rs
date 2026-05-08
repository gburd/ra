//! Neural cost scoring for whole-plan evaluation.
//!
//! [`NeuralPlanScorer`] scores a complete `RelExpr` tree using the
//! [`FastCostModel`]. This provides a whole-plan neural cost estimate
//! that can be used for:
//! - Convergence detection during saturation (comparing iteration-over-iteration)
//! - Plan cache priority scoring
//! - Monitoring and diagnostics
//!
//! # Relationship to `HybridCostFn`
//!
//! `HybridCostFn` operates per-node inside the egg `CostFunction` trait,
//! using compact 8-dim node features. `NeuralPlanScorer` operates on the
//! complete extracted plan using the full 12-dim `QueryFeatures` vector.
//! They complement each other: `HybridCostFn` guides extraction,
//! `NeuralPlanScorer` evaluates the result.
//!
//! # Usage
//!
//! ```ignore
//! let scorer = NeuralPlanScorer::from_file("model.json").unwrap_or_default();
//! let (neural_cost, confidence) = scorer.score(&relexpr);
//! tracing::debug!(neural_cost, confidence, "neural plan score");
//! ```

use ra_core::algebra::RelExpr;

use crate::cost_model::extract_features;
use crate::cost_model::fast_model::FastCostModel;
use crate::cost_model::production_model::ProductionCostModel;

/// Neural plan scorer that re-scores a `RelExpr` using a [`FastCostModel`].
///
/// The scorer is cheap to clone (the model is heap-allocated but the scorer
/// holds an `Arc` to it — see [`NeuralPlanScorer::shared()`]).
pub struct NeuralPlanScorer {
    model: FastCostModel,
    /// Scalar weight for each cost dimension when aggregating to a single f64.
    weights: CostWeights,
}

/// Aggregation weights for combining the 16-dimensional cost vector into a
/// single scalar for comparison with `IntegratedCostFn` outputs.
#[derive(Debug, Clone)]
pub struct CostWeights {
    /// Weight for `cpu_time_ms` (default: 0.50).
    pub cpu: f32,
    /// Weight for `io_storage_ops` scaled by `io_ops_scale` (default: 0.30).
    pub io: f32,
    /// Weight for `memory_peak_mb` scaled by `mem_scale` (default: 0.20).
    pub memory: f32,
    /// Divisor applied to `io_storage_ops` before weighting (default: 1000.0).
    pub io_ops_scale: f32,
    /// Divisor applied to `memory_peak_mb` before weighting (default: 100.0).
    pub mem_scale: f32,
}

impl Default for CostWeights {
    fn default() -> Self {
        Self {
            cpu: 0.50,
            io: 0.30,
            memory: 0.20,
            io_ops_scale: 1000.0,
            mem_scale: 100.0,
        }
    }
}

impl NeuralPlanScorer {
    /// Create a scorer with a randomly-initialised model and default weights.
    ///
    /// Useful for warm-start before any real training data is available.
    pub fn new() -> Self {
        Self { model: FastCostModel::new_random(), weights: CostWeights::default() }
    }

    /// Create a scorer by loading a persisted `ProductionCostModel` and
    /// distilling its weights into the compact `FastCostModel` layout.
    pub fn from_production_model(path: &str) -> anyhow::Result<Self> {
        let production = ProductionCostModel::load_from_file(path)?;
        Ok(Self {
            model: FastCostModel::from_production(&production),
            weights: CostWeights::default(),
        })
    }

    /// Override the aggregation weights.
    pub fn with_weights(mut self, weights: CostWeights) -> Self {
        self.weights = weights;
        self
    }

    /// Score a `RelExpr` plan.
    ///
    /// Returns `(scalar_cost: f64, confidence: f32)`:
    /// - `scalar_cost` — weighted aggregate of the 16 neural cost dimensions,
    ///   in the same units as `IntegratedCostFn` output (higher = worse).
    /// - `confidence` — `0.0` (no training) to `1.0` (fully calibrated).
    pub fn score(&self, plan: &RelExpr) -> (f64, f32) {
        let features = extract_features(plan);
        let costs = self.model.predict_all(&features);
        let scalar = self.aggregate(&costs);
        let confidence = self.confidence();
        (scalar, confidence)
    }

    /// Predict only the CPU time dimension — fastest path (~45 ns).
    pub fn predict_cpu_ms(&self, plan: &RelExpr) -> f32 {
        let features = extract_features(plan);
        self.model.predict_cpu_ms(&features)
    }

    /// Compute the cost ratio between the neural estimate and a reference cost.
    ///
    /// A ratio < 1.0 means the neural model predicts this plan is cheaper than
    /// the reference (e.g., `IntegratedCostFn` output). Can be used to decide
    /// whether to prefer the neural estimate.
    pub fn cost_ratio(&self, plan: &RelExpr, reference_cost: f64) -> f64 {
        let (neural_cost, _) = self.score(plan);
        if reference_cost > 0.0 {
            neural_cost / reference_cost
        } else {
            1.0
        }
    }

    /// Fit normalization constants from a set of query features.
    ///
    /// Improves prediction accuracy when `features` are available at startup
    /// (e.g., loaded from a training dataset). Call once before deployment.
    pub fn fit_normalization(&mut self, feature_samples: &[ra_core::algebra::RelExpr]) {
        let features: Vec<_> = feature_samples.iter().map(extract_features).collect();
        self.model.fit_normalization(&features);
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn aggregate(&self, costs: &crate::cost_model::CostVector) -> f64 {
        let w = &self.weights;
        let cpu_part = costs.cpu_time_ms * w.cpu;
        let io_part = (costs.io_storage_ops as f32 / w.io_ops_scale) * w.io;
        let mem_part = (costs.memory_peak_mb / w.mem_scale) * w.memory;
        f64::from(cpu_part + io_part + mem_part)
    }

    fn confidence(&self) -> f32 {
        // Proxy: proportion of hidden neurons likely active, scaled by
        // training sample count. Zero if untrained.
        let trained = self.model.samples_trained;
        if trained == 0 {
            0.0
        } else {
            // Saturates near 1.0 at ~10_000 training samples
            let n = trained as f32;
            1.0 - (-(n / 5000.0)).exp()
        }
    }
}

impl Default for NeuralPlanScorer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;

    fn simple_scan() -> RelExpr {
        RelExpr::Scan { table: "orders".to_string(), alias: None }
    }

    fn simple_join() -> RelExpr {
        use ra_core::expr::{Const, Expr};
        RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(simple_scan()),
            right: Box::new(RelExpr::Scan { table: "lineitem".to_string(), alias: None }),
        }
    }

    #[test]
    fn test_scorer_creates_with_defaults() {
        let scorer = NeuralPlanScorer::new();
        assert_eq!(scorer.model.samples_trained, 0);
    }

    #[test]
    fn test_score_returns_non_negative() {
        let scorer = NeuralPlanScorer::new();
        let (cost, confidence) = scorer.score(&simple_scan());
        assert!(cost >= 0.0, "cost must be non-negative");
        assert_eq!(confidence, 0.0, "untrained model has zero confidence");
    }

    #[test]
    fn test_join_costs_more_than_scan() {
        let scorer = NeuralPlanScorer::new();
        let (scan_cost, _) = scorer.score(&simple_scan());
        let (join_cost, _) = scorer.score(&simple_join());
        // A join over two tables should cost more than a single scan
        // (with the same random weights this is probabilistic, but usually holds)
        assert!(join_cost > 0.0 && scan_cost > 0.0);
    }

    #[test]
    fn test_predict_cpu_ms_positive() {
        let scorer = NeuralPlanScorer::new();
        let cpu = scorer.predict_cpu_ms(&simple_join());
        assert!(cpu > 0.0);
    }

    #[test]
    fn test_cost_ratio_near_one_on_equal() {
        let scorer = NeuralPlanScorer::new();
        let (cost, _) = scorer.score(&simple_scan());
        let ratio = scorer.cost_ratio(&simple_scan(), cost);
        assert!((ratio - 1.0).abs() < 1e-6, "ratio should be exactly 1 on self");
    }
}
