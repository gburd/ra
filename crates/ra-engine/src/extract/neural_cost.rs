//! Neural cost scoring for whole-plan evaluation.
//!
//! [`NeuralPlanScorer`] scores a complete `RelExpr` tree using the
//! [`BitNetCostModel`]. This provides a whole-plan neural cost estimate
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
//! let scorer = NeuralPlanScorer::from_file("model.bitnet.json").unwrap_or_default();
//! let (neural_cost, confidence) = scorer.score(&relexpr);
//! tracing::debug!(neural_cost, confidence, "neural plan score");
//! ```

use ra_core::algebra::RelExpr;

use crate::cost_model::extract_features;
use crate::cost_model::BitNetCostModel;

/// Neural plan scorer that re-scores a `RelExpr` using a [`BitNetCostModel`].
pub struct NeuralPlanScorer {
    model: BitNetCostModel,
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
    /// Create a scorer with a zero-weight model and default weights.
    pub fn new() -> Self {
        Self { model: BitNetCostModel::new_zeros(), weights: CostWeights::default() }
    }

    /// Create a scorer by loading a persisted `BitNetCostModel`.
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let model = BitNetCostModel::load_from_file(path)
            .map_err(|e| anyhow::anyhow!("failed to load model: {e}"))?;
        Ok(Self { model, weights: CostWeights::default() })
    }

    /// Create a scorer from an existing model.
    pub fn from_model(model: BitNetCostModel) -> Self {
        Self { model, weights: CostWeights::default() }
    }

    /// Override the aggregation weights.
    pub fn with_weights(mut self, weights: CostWeights) -> Self {
        self.weights = weights;
        self
    }

    /// Score a `RelExpr` plan.
    ///
    /// Returns `(scalar_cost: f64, confidence: f32)`:
    /// - `scalar_cost` — weighted aggregate of the 16 neural cost dimensions.
    /// - `confidence` — `0.0` (no training) to `1.0` (fully calibrated).
    pub fn score(&self, plan: &RelExpr) -> (f64, f32) {
        let features = extract_features(plan);
        let costs = self.model.predict_all(&features.as_array());
        let scalar = self.aggregate(&costs);
        let confidence = self.confidence();
        (scalar, confidence)
    }

    /// Predict only the CPU time dimension (~87ns).
    pub fn predict_cpu_ms(&self, plan: &RelExpr) -> f32 {
        let features = extract_features(plan);
        self.model.predict_cpu_ms(&features.as_array())
    }

    /// Compute the cost ratio between the neural estimate and a reference cost.
    pub fn cost_ratio(&self, plan: &RelExpr, reference_cost: f64) -> f64 {
        let (neural_cost, _) = self.score(plan);
        if reference_cost > 0.0 {
            neural_cost / reference_cost
        } else {
            1.0
        }
    }

    fn aggregate(&self, costs: &[f32; 16]) -> f64 {
        let w = &self.weights;
        let cpu_part = costs[0] * w.cpu;
        let io_part = (costs[3] / w.io_ops_scale) * w.io;
        let mem_part = (costs[1] / w.mem_scale) * w.memory;
        f64::from(cpu_part + io_part + mem_part)
    }

    fn confidence(&self) -> f32 {
        let trained = self.model.samples_trained;
        if trained == 0 {
            0.0
        } else {
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
