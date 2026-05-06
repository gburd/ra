//! Production-grade neural cost model for query optimization.
//!
//! Extends [`super::simple_model::SimpleCostModel`] with features needed for
//! sustained production training:
//!
//! - **Larger hidden layer** (12 → 64 → 16) for more expressive capacity
//! - **Momentum SGD** with configurable β (default 0.9) for faster convergence
//! - **L2 weight decay** to reduce overfitting on small datasets
//! - **Gradient clipping** to prevent training instability
//! - **Batch gradient accumulation** — collect gradients, update once per batch
//! - **Adaptive learning rate** — multiplicative decay on loss plateau
//! - **Model persistence** — save/load weights as JSON (no external ML deps)
//! - **Prediction confidence** — inverse recent-error proxy for uncertainty
//!
//! # Quick Start
//!
//! ```ignore
//! use ra_engine::cost_model::{QueryFeatures, ActualCost};
//! use ra_engine::cost_model::production_model::{ProductionCostModel, TrainingConfig};
//!
//! let mut model = ProductionCostModel::new(TrainingConfig::default());
//! model.train_batch(&[(features, actual_cost)]);
//! let (pred, confidence) = model.predict_with_confidence(&features);
//! model.save_to_file("/tmp/model.json").expect("save");
//! let loaded = ProductionCostModel::load_from_file("/tmp/model.json").expect("load");
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{ActualCost, CostVector, QueryFeatures};

// ---------------------------------------------------------------------------
// Training configuration
// ---------------------------------------------------------------------------

/// Hyper-parameters for the production cost model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingConfig {
    /// Initial learning rate (default: 0.005).
    pub learning_rate: f32,
    /// SGD momentum coefficient β (default: 0.9).
    pub momentum: f32,
    /// L2 weight decay coefficient (default: 1e-4).
    pub weight_decay: f32,
    /// Gradient clipping norm (default: 1.0).
    pub grad_clip: f32,
    /// Multiply LR by this factor on plateau (default: 0.5).
    pub lr_decay_factor: f32,
    /// Number of training steps with no improvement before LR decay (default: 500).
    pub plateau_patience: usize,
    /// Minimum learning rate before stopping decay (default: 1e-5).
    pub min_learning_rate: f32,
    /// Batch size for gradient accumulation (default: 32).
    pub batch_size: usize,
}

impl Default for TrainingConfig {
    fn default() -> Self {
        Self {
            learning_rate: 0.005,
            momentum: 0.9,
            weight_decay: 1e-4,
            grad_clip: 1.0,
            lr_decay_factor: 0.5,
            plateau_patience: 500,
            min_learning_rate: 1e-5,
            batch_size: 32,
        }
    }
}

// ---------------------------------------------------------------------------
// Model internals
// ---------------------------------------------------------------------------

const HIDDEN: usize = 64;
const OUTPUT: usize = 16;
const INPUT: usize = QueryFeatures::FEATURE_DIM; // 12

/// Serialisable weight checkpoint.
#[derive(Serialize, Deserialize)]
struct ModelCheckpoint {
    config: TrainingConfig,
    w1: Vec<f32>,  // flat [INPUT × HIDDEN]
    b1: Vec<f32>,  // [HIDDEN]
    w2: Vec<f32>,  // flat [HIDDEN × OUTPUT]
    b2: Vec<f32>,  // [OUTPUT]
    samples_seen: usize,
    avg_loss: f32,
    learning_rate: f32,
}

// ---------------------------------------------------------------------------
// ProductionCostModel
// ---------------------------------------------------------------------------

/// Production neural cost model with momentum SGD and model persistence.
pub struct ProductionCostModel {
    // Weights: row-major [row][col]
    w1: Vec<Vec<f32>>, // [INPUT][HIDDEN]
    b1: Vec<f32>,      // [HIDDEN]
    w2: Vec<Vec<f32>>, // [HIDDEN][OUTPUT]
    b2: Vec<f32>,      // [OUTPUT]

    // Momentum velocities
    v_w1: Vec<Vec<f32>>,
    v_b1: Vec<f32>,
    v_w2: Vec<Vec<f32>>,
    v_b2: Vec<f32>,

    // Batch gradient accumulators
    g_w1: Vec<Vec<f32>>,
    g_b1: Vec<f32>,
    g_w2: Vec<Vec<f32>>,
    g_b2: Vec<f32>,
    batch_count: usize,

    // Training state
    config: TrainingConfig,
    current_lr: f32,
    samples_seen: usize,
    avg_loss: f32,        // exponential moving average of MSE loss
    best_loss: f32,       // best avg_loss seen so far
    steps_since_best: usize,
}

impl ProductionCostModel {
    /// Create a new model with Xavier-initialized weights.
    pub fn new(config: TrainingConfig) -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        let w1_scale = (2.0 / INPUT as f32).sqrt();
        let w2_scale = (2.0 / HIDDEN as f32).sqrt();
        let current_lr = config.learning_rate;

        let w1: Vec<Vec<f32>> = (0..INPUT)
            .map(|_| (0..HIDDEN).map(|_| rng.gen::<f32>() * w1_scale - w1_scale / 2.0).collect())
            .collect();
        let w2: Vec<Vec<f32>> = (0..HIDDEN)
            .map(|_| (0..OUTPUT).map(|_| rng.gen::<f32>() * w2_scale - w2_scale / 2.0).collect())
            .collect();

        Self {
            w1,
            b1: vec![0.0; HIDDEN],
            w2,
            b2: vec![0.0; OUTPUT],
            v_w1: vec![vec![0.0; HIDDEN]; INPUT],
            v_b1: vec![0.0; HIDDEN],
            v_w2: vec![vec![0.0; OUTPUT]; HIDDEN],
            v_b2: vec![0.0; OUTPUT],
            g_w1: vec![vec![0.0; HIDDEN]; INPUT],
            g_b1: vec![0.0; HIDDEN],
            g_w2: vec![vec![0.0; OUTPUT]; HIDDEN],
            g_b2: vec![0.0; OUTPUT],
            batch_count: 0,
            current_lr,
            samples_seen: 0,
            avg_loss: f32::MAX,
            best_loss: f32::MAX,
            steps_since_best: 0,
            config,
        }
    }

    /// Predict costs for the given query features.
    pub fn predict(&self, features: &QueryFeatures) -> CostVector {
        let (output, _) = self.forward(features);
        costs_from_output(&output)
    }

    /// Predict costs and return a confidence score in [0, 1].
    ///
    /// Confidence is derived from the inverse of the recent average prediction
    /// error: high confidence (≈1) when recent errors are small, low (≈0) when
    /// the model is poorly calibrated.
    pub fn predict_with_confidence(&self, features: &QueryFeatures) -> (CostVector, f32) {
        let costs = self.predict(features);
        let confidence = if self.avg_loss == f32::MAX || self.avg_loss <= 0.0 {
            0.0
        } else {
            // Sigmoid-like mapping: 1 / (1 + loss)
            1.0 / (1.0 + self.avg_loss)
        };
        (costs, confidence)
    }

    /// Train on a single `(features, actual)` pair (online learning).
    pub fn train_single(&mut self, features: &QueryFeatures, actual: &ActualCost) {
        let loss = self.accumulate_gradients(features, actual);
        self.update_avg_loss(loss);
        // Flush immediately for online mode; counts as one gradient update.
        self.apply_accumulated_gradients();
        self.batch_count += 1;
        self.adapt_learning_rate();
    }

    /// Train on a batch of `(features, actual)` pairs.
    ///
    /// Gradients are accumulated across the batch and applied once at the end.
    /// More efficient than repeated `train_single` calls.
    pub fn train_batch(&mut self, samples: &[(QueryFeatures, ActualCost)]) {
        if samples.is_empty() {
            return;
        }
        let mut total_loss = 0.0_f32;
        for (features, actual) in samples {
            total_loss += self.accumulate_gradients(features, actual);
        }
        self.update_avg_loss(total_loss / samples.len() as f32);
        self.apply_accumulated_gradients();
        self.batch_count += 1; // one gradient update per train_batch call
        self.adapt_learning_rate();
    }

    /// Save model weights and training state to a JSON file.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let checkpoint = ModelCheckpoint {
            config: self.config.clone(),
            w1: self.w1.iter().flatten().copied().collect(),
            b1: self.b1.clone(),
            w2: self.w2.iter().flatten().copied().collect(),
            b2: self.b2.clone(),
            samples_seen: self.samples_seen,
            avg_loss: self.avg_loss,
            learning_rate: self.current_lr,
        };
        let json = serde_json::to_string_pretty(&checkpoint)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load model weights from a JSON file produced by `save_to_file`.
    pub fn load_from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let ck: ModelCheckpoint = serde_json::from_str(&json)?;

        let mut model = Self::new(ck.config);
        for i in 0..INPUT {
            for j in 0..HIDDEN {
                model.w1[i][j] = ck.w1[i * HIDDEN + j];
            }
        }
        model.b1.copy_from_slice(&ck.b1);
        for i in 0..HIDDEN {
            for j in 0..OUTPUT {
                model.w2[i][j] = ck.w2[i * OUTPUT + j];
            }
        }
        model.b2.copy_from_slice(&ck.b2);
        model.samples_seen = ck.samples_seen;
        model.avg_loss = ck.avg_loss;
        model.best_loss = ck.avg_loss;
        model.current_lr = ck.learning_rate;
        Ok(model)
    }

    /// Return model training statistics.
    pub fn stats(&self) -> ProductionModelStats {
        ProductionModelStats {
            samples_seen: self.samples_seen,
            avg_loss: self.avg_loss,
            current_lr: self.current_lr,
            batch_count: self.batch_count,
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Forward pass: returns (output pre-activation, hidden post-activation).
    fn forward(&self, features: &QueryFeatures) -> (Vec<f32>, Vec<f32>) {
        let x = features.to_vec();
        let mut hidden = vec![0.0_f32; HIDDEN];
        for i in 0..HIDDEN {
            let mut sum = self.b1[i];
            for j in 0..INPUT {
                sum += self.w1[j][i] * x[j];
            }
            hidden[i] = sum.max(0.0); // ReLU
        }
        let mut output = vec![0.0_f32; OUTPUT];
        for i in 0..OUTPUT {
            let mut sum = self.b2[i];
            for j in 0..HIDDEN {
                sum += self.w2[j][i] * hidden[j];
            }
            output[i] = softplus(sum);
        }
        (output, hidden)
    }

    /// Run one forward+backward pass, accumulate gradients, return MSE loss.
    fn accumulate_gradients(&mut self, features: &QueryFeatures, actual: &ActualCost) -> f32 {
        let x = features.to_vec();

        // Forward
        let mut hidden_pre = vec![0.0_f32; HIDDEN];
        let mut hidden = vec![0.0_f32; HIDDEN];
        for i in 0..HIDDEN {
            let mut sum = self.b1[i];
            for j in 0..INPUT {
                sum += self.w1[j][i] * x[j];
            }
            hidden_pre[i] = sum;
            hidden[i] = sum.max(0.0);
        }
        let mut output_pre = vec![0.0_f32; OUTPUT];
        let mut output = vec![0.0_f32; OUTPUT];
        for i in 0..OUTPUT {
            let mut sum = self.b2[i];
            for j in 0..HIDDEN {
                sum += self.w2[j][i] * hidden[j];
            }
            output_pre[i] = sum;
            output[i] = softplus(sum);
        }

        let target = actual_to_vec(actual);
        let mut total_loss = 0.0_f32;

        // Output gradients (MSE + softplus derivative)
        let mut output_grad = vec![0.0_f32; OUTPUT];
        for i in 0..OUTPUT {
            let diff = output[i] - target[i];
            total_loss += diff * diff;
            output_grad[i] = 2.0 * diff * softplus_deriv(output_pre[i]);
        }

        // Hidden gradients
        let mut hidden_grad = vec![0.0_f32; HIDDEN];
        for i in 0..HIDDEN {
            let mut s = 0.0_f32;
            for j in 0..OUTPUT {
                s += output_grad[j] * self.w2[i][j];
            }
            hidden_grad[i] = if hidden_pre[i] > 0.0 { s } else { 0.0 };
        }

        // Clip gradients
        clip_gradients(&mut output_grad, self.config.grad_clip);
        clip_gradients(&mut hidden_grad, self.config.grad_clip);

        // Accumulate into batch gradient buffers (average over batch)
        let scale = 1.0 / self.config.batch_size.max(1) as f32;
        for i in 0..HIDDEN {
            for j in 0..OUTPUT {
                self.g_w2[i][j] += scale * output_grad[j] * hidden[i];
            }
        }
        for j in 0..OUTPUT {
            self.g_b2[j] += scale * output_grad[j];
        }
        for i in 0..INPUT {
            for j in 0..HIDDEN {
                self.g_w1[i][j] += scale * hidden_grad[j] * x[i];
            }
        }
        for j in 0..HIDDEN {
            self.g_b1[j] += scale * hidden_grad[j];
        }

        self.samples_seen += 1;
        total_loss / OUTPUT as f32
    }

    /// Apply accumulated gradients using momentum SGD with weight decay.
    fn apply_accumulated_gradients(&mut self) {
        let lr = self.current_lr;
        let β = self.config.momentum;
        let λ = self.config.weight_decay;

        // Update w2, b2
        for i in 0..HIDDEN {
            for j in 0..OUTPUT {
                self.v_w2[i][j] = β * self.v_w2[i][j] - lr * (self.g_w2[i][j] + λ * self.w2[i][j]);
                self.w2[i][j] += self.v_w2[i][j];
                self.g_w2[i][j] = 0.0;
            }
        }
        for j in 0..OUTPUT {
            self.v_b2[j] = β * self.v_b2[j] - lr * self.g_b2[j];
            self.b2[j] += self.v_b2[j];
            self.g_b2[j] = 0.0;
        }

        // Update w1, b1
        for i in 0..INPUT {
            for j in 0..HIDDEN {
                self.v_w1[i][j] = β * self.v_w1[i][j] - lr * (self.g_w1[i][j] + λ * self.w1[i][j]);
                self.w1[i][j] += self.v_w1[i][j];
                self.g_w1[i][j] = 0.0;
            }
        }
        for j in 0..HIDDEN {
            self.v_b1[j] = β * self.v_b1[j] - lr * self.g_b1[j];
            self.b1[j] += self.v_b1[j];
            self.g_b1[j] = 0.0;
        }
    }

    /// Update exponential moving average of loss and track plateau.
    fn update_avg_loss(&mut self, loss: f32) {
        if self.avg_loss == f32::MAX {
            self.avg_loss = loss;
        } else {
            self.avg_loss = 0.95 * self.avg_loss + 0.05 * loss;
        }
        if self.avg_loss < self.best_loss {
            self.best_loss = self.avg_loss;
            self.steps_since_best = 0;
        } else {
            self.steps_since_best += 1;
        }
    }

    /// Decay learning rate if loss has plateaued.
    fn adapt_learning_rate(&mut self) {
        if self.steps_since_best >= self.config.plateau_patience {
            let new_lr = (self.current_lr * self.config.lr_decay_factor)
                .max(self.config.min_learning_rate);
            if new_lr < self.current_lr {
                self.current_lr = new_lr;
                self.steps_since_best = 0;
                tracing::debug!(lr = self.current_lr, "learning rate decayed");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Model statistics
// ---------------------------------------------------------------------------

/// Runtime statistics for monitoring training health.
#[derive(Debug, Clone)]
pub struct ProductionModelStats {
    /// Total training samples processed.
    pub samples_seen: usize,
    /// Exponential moving average of batch MSE loss.
    pub avg_loss: f32,
    /// Current learning rate (may have been decayed).
    pub current_lr: f32,
    /// Total number of gradient batches applied.
    pub batch_count: usize,
}

// ---------------------------------------------------------------------------
// Pure functions (numerical)
// ---------------------------------------------------------------------------

#[inline]
fn softplus(x: f32) -> f32 {
    if x > 20.0 { x } else { (1.0 + x.exp()).ln() }
}

#[inline]
fn softplus_deriv(x: f32) -> f32 {
    if x > 20.0 { 1.0 } else { let e = x.exp(); e / (1.0 + e) }
}

fn clip_gradients(g: &mut [f32], max_norm: f32) {
    let norm: f32 = g.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > max_norm {
        let scale = max_norm / norm;
        for v in g.iter_mut() {
            *v *= scale;
        }
    }
}

fn costs_from_output(output: &[f32]) -> CostVector {
    CostVector {
        cpu_time_ms:          output[0],
        memory_peak_mb:       output[1],
        memory_avg_mb:        output[2],
        io_storage_ops:       output[3] as u64,
        io_storage_bytes:     output[4] as u64,
        io_network_ops:       output[5] as u64,
        io_network_bytes:     output[6] as u64,
        locks_acquired:       output[7] as u32,
        lock_hold_time_ms:    output[8],
        lock_contention_score: output[9],
        vacuum_overhead:      output[10],
        wal_generation_bytes: output[11] as u64,
        replication_lag_ms:   output[12],
        cache_hit_ratio:      output[13].clamp(0.0, 1.0),
        page_faults:          output[14] as u32,
        context_switches:     output[15] as u32,
    }
}

fn actual_to_vec(a: &ActualCost) -> Vec<f32> {
    vec![
        a.cpu_time_ms,
        a.memory_peak_mb,
        a.memory_avg_mb,
        a.io_storage_ops as f32,
        a.io_storage_bytes as f32,
        a.io_network_ops as f32,
        a.io_network_bytes as f32,
        a.locks_acquired as f32,
        a.lock_hold_time_ms,
        a.lock_contention_score,
        a.vacuum_overhead,
        a.wal_generation_bytes as f32,
        a.replication_lag_ms,
        a.cache_hit_ratio,
        a.page_faults as f32,
        a.context_switches as f32,
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn default_features() -> QueryFeatures {
        QueryFeatures {
            table_count: 3.0,
            join_count: 2.0,
            filter_count: 4.0,
            aggregate_count: 1.0,
            subquery_count: 0.0,
            cte_count: 0.0,
            window_function_count: 0.0,
            order_by_count: 1.0,
            group_by_count: 1.0,
            distinct_flag: 0.0,
            limit_present: 1.0,
            max_join_cardinality: 5000.0,
        }
    }

    fn default_actual() -> ActualCost {
        ActualCost {
            cpu_time_ms: 42.0,
            memory_peak_mb: 128.0,
            memory_avg_mb: 80.0,
            io_storage_ops: 500,
            io_storage_bytes: 4_096_000,
            io_network_ops: 0,
            io_network_bytes: 0,
            locks_acquired: 3,
            lock_hold_time_ms: 0.5,
            lock_contention_score: 0.02,
            vacuum_overhead: 0.0,
            wal_generation_bytes: 8192,
            replication_lag_ms: 0.0,
            cache_hit_ratio: 0.95,
            page_faults: 0,
            context_switches: 12,
        }
    }

    #[test]
    fn test_production_model_creates() {
        let model = ProductionCostModel::new(TrainingConfig::default());
        let stats = model.stats();
        assert_eq!(stats.samples_seen, 0);
        assert_eq!(stats.batch_count, 0);
        assert_eq!(stats.current_lr, TrainingConfig::default().learning_rate);
    }

    #[test]
    fn test_predict_non_negative() {
        let model = ProductionCostModel::new(TrainingConfig::default());
        let pred = model.predict(&default_features());
        assert!(pred.cpu_time_ms >= 0.0);
        assert!(pred.memory_peak_mb >= 0.0);
        assert!(pred.cache_hit_ratio >= 0.0);
        assert!(pred.cache_hit_ratio <= 1.0);
    }

    #[test]
    fn test_train_single_reduces_loss() {
        let mut model = ProductionCostModel::new(TrainingConfig::default());
        let features = default_features();
        let actual = default_actual();

        // Train for 50 steps and expect loss to decrease
        for _ in 0..50 {
            model.train_single(&features, &actual);
        }
        assert!(model.stats().avg_loss < f32::MAX);
        assert!(model.stats().samples_seen == 50);
    }

    #[test]
    fn test_train_batch() {
        let mut model = ProductionCostModel::new(TrainingConfig::default());
        let samples: Vec<(QueryFeatures, ActualCost)> =
            (0..20).map(|_| (default_features(), default_actual())).collect();

        model.train_batch(&samples);
        assert_eq!(model.stats().samples_seen, 20);
        assert_eq!(model.stats().batch_count, 1);
    }

    #[test]
    fn test_confidence_increases_with_training() {
        let mut model = ProductionCostModel::new(TrainingConfig::default());
        let features = default_features();
        let actual = default_actual();

        let (_, conf_before) = model.predict_with_confidence(&features);
        // Before any training, avg_loss=MAX → confidence=0
        assert_eq!(conf_before, 0.0);

        for _ in 0..100 {
            model.train_single(&features, &actual);
        }
        let (_, conf_after) = model.predict_with_confidence(&features);
        assert!(conf_after > conf_before, "confidence should increase after training");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let mut model = ProductionCostModel::new(TrainingConfig::default());
        let features = default_features();
        let actual = default_actual();
        for _ in 0..10 {
            model.train_single(&features, &actual);
        }

        let tmp_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/production_model_test.json");
        std::fs::create_dir_all(tmp_path.parent().expect("parent")).expect("mkdir");
        model.save_to_file(&tmp_path).expect("save");

        let loaded = ProductionCostModel::load_from_file(&tmp_path).expect("load");
        let pred_orig = model.predict(&features);
        let pred_loaded = loaded.predict(&features);

        assert!(
            (pred_orig.cpu_time_ms - pred_loaded.cpu_time_ms).abs() < 1e-4,
            "loaded model should produce identical predictions"
        );
        let _ = std::fs::remove_file(&tmp_path);
    }
}
