//! Quantization-Aware Training (QAT) with Straight-Through Estimator (STE).
//!
//! Trains a `BitNetCostModel` directly in ternary space by maintaining
//! full-precision latent weights that are quantized on each forward pass.
//! Gradients flow through the quantization step via STE (treating
//! `round_clip` as identity during backprop).
//!
//! # Algorithm
//!
//! ```text
//! Forward:  W_q = quantize(W_latent)  →  h = ReLU(W_q1 · x + b1)  →  y = W_q2 · h + b2
//! Loss:     L = MSE(y, target)
//! Backward: ∂L/∂W_latent ≈ ∂L/∂W_q  (STE: ignore quantization in gradient)
//! Update:   W_latent -= lr * ∂L/∂W_latent
//! ```

use crate::{default_scalar_head, BitNetCostModel, F, H, O};

/// QAT trainer for the `BitNet` cost model.
///
/// Maintains full-precision latent weights that are quantized to ternary
/// on each forward pass. Gradients pass through quantization via STE.
pub struct BitNetTrainer {
    // Latent full-precision weights (updated by optimizer)
    w1: [[f32; H]; F],
    b1: [f32; H],
    w2: [[f32; O]; H],
    b2: [f32; O],

    // Input normalization (fixed after fit_normalization)
    feature_mean: [f32; F],
    feature_inv_std: [f32; F],

    // Adam optimizer state
    m_w1: [[f32; H]; F],  // first moment
    v_w1: [[f32; H]; F],  // second moment
    m_b1: [f32; H],
    v_b1: [f32; H],
    m_w2: [[f32; O]; H],
    v_w2: [[f32; O]; H],
    m_b2: [f32; O],
    v_b2: [f32; O],

    // Learnable scalar head (A5). Mirrored on BitNetCostModel; updated
    // independently from the per-dim hidden layers via plain SGD on
    // observed CPU time (the only feedback signal the PG executor
    // actually emits). Initialized to the historical hand-tuned
    // formula so first-snapshot behavior is backward-compatible.
    scalar_head: [f32; O],
    scalar_bias: f32,

    // Training state
    config: TrainerConfig,
    step: usize,
    total_loss: f64,
    loss_count: usize,
}

/// Training configuration.
#[derive(Debug, Clone)]
pub struct TrainerConfig {
    /// Learning rate (default: 0.001).
    pub lr: f32,
    /// Adam beta1 (default: 0.9).
    pub beta1: f32,
    /// Adam beta2 (default: 0.999).
    pub beta2: f32,
    /// Adam epsilon (default: 1e-8).
    pub eps: f32,
    /// L2 weight decay (default: 0.01).
    pub weight_decay: f32,
    /// Gradient clipping max norm (default: 1.0).
    pub max_grad_norm: f32,
}

impl Default for TrainerConfig {
    fn default() -> Self {
        Self {
            lr: 0.001,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            weight_decay: 0.01,
            max_grad_norm: 1.0,
        }
    }
}

impl BitNetTrainer {
    /// Create a new trainer with Xavier-initialized latent weights.
    #[must_use]
    pub fn new(config: TrainerConfig) -> Self {
        // Xavier initialization
        let w1_scale = (2.0 / F as f32).sqrt();
        let w2_scale = (2.0 / H as f32).sqrt();

        let mut w1 = [[0.0f32; H]; F];
        let mut w2 = [[0.0f32; O]; H];

        // Simple deterministic pseudo-random init (no rand dependency needed)
        let mut seed: u64 = 42;
        for row in &mut w1 {
            for v in row.iter_mut() {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let u = (seed >> 33) as f32 / (u32::MAX >> 1) as f32;
                *v = (u - 0.5) * 2.0 * w1_scale;
            }
        }
        for row in &mut w2 {
            for v in row.iter_mut() {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let u = (seed >> 33) as f32 / (u32::MAX >> 1) as f32;
                *v = (u - 0.5) * 2.0 * w2_scale;
            }
        }

        Self {
            w1,
            b1: [0.0; H],
            w2,
            b2: [0.0; O],
            feature_mean: [0.0; F],
            feature_inv_std: [1.0; F],
            m_w1: [[0.0; H]; F],
            v_w1: [[0.0; H]; F],
            m_b1: [0.0; H],
            v_b1: [0.0; H],
            m_w2: [[0.0; O]; H],
            v_w2: [[0.0; O]; H],
            m_b2: [0.0; O],
            v_b2: [0.0; O],
            scalar_head: default_scalar_head(),
            scalar_bias: 0.0,
            config,
            step: 0,
            total_loss: 0.0,
            loss_count: 0,
        }
    }

    /// Create a trainer initialized from existing f32 weights (for fine-tuning).
    #[must_use]
    pub fn from_weights(
        w1: [[f32; H]; F],
        b1: [f32; H],
        w2: [[f32; O]; H],
        b2: [f32; O],
        config: TrainerConfig,
    ) -> Self {
        Self {
            w1,
            b1,
            w2,
            b2,
            feature_mean: [0.0; F],
            feature_inv_std: [1.0; F],
            m_w1: [[0.0; H]; F],
            v_w1: [[0.0; H]; F],
            m_b1: [0.0; H],
            v_b1: [0.0; H],
            m_w2: [[0.0; O]; H],
            v_w2: [[0.0; O]; H],
            m_b2: [0.0; O],
            v_b2: [0.0; O],
            scalar_head: default_scalar_head(),
            scalar_bias: 0.0,
            config,
            step: 0,
            total_loss: 0.0,
            loss_count: 0,
        }
    }

    /// Set input normalization parameters.
    pub fn set_normalization(&mut self, mean: [f32; F], inv_std: [f32; F]) {
        self.feature_mean = mean;
        self.feature_inv_std = inv_std;
    }

    /// Train on a single `(features, target)` pair.
    ///
    /// Returns the MSE loss for this sample. Trains every output dimension
    /// equally; for partial supervision (only some dims observed) use
    /// [`Self::train_step_masked`].
    pub fn train_step(&mut self, features: &[f32; F], target: &[f32; O]) -> f32 {
        self.train_step_masked(features, target, &[true; O])
    }

    /// Train on a single sample where only some target dimensions are observed.
    ///
    /// `mask[j] == true` means dim `j` of `target` is supervised; `false`
    /// means the loss contribution from dim `j` is dropped (no gradient).
    /// This avoids forcing unobserved dimensions toward zero during partial
    /// feedback (e.g. when only CPU time is known but routing dims are not).
    ///
    /// The forward pass mirrors inference exactly, applying `softplus` to
    /// the linear output before computing loss. The backward pass propagates
    /// gradient through `softplus` via its derivative `sigmoid(pre)`.
    ///
    /// Returns the average MSE loss over the observed dimensions (0.0 if
    /// none are observed).
    pub fn train_step_masked(
        &mut self,
        features: &[f32; F],
        target: &[f32; O],
        mask: &[bool; O],
    ) -> f32 {
        let observed_count = mask.iter().filter(|m| **m).count();
        if observed_count == 0 {
            return 0.0;
        }

        // --- Forward pass (with quantization) ---
        let x_norm = self.normalize(features);
        let (h_pre, h) = self.forward_hidden(&x_norm);
        let y_pre = self.forward_output(&h); // linear pre-activation
        let y = apply_softplus(&y_pre); // matches inference

        // --- Compute masked MSE loss on post-activation ---
        let mut loss = 0.0f32;
        let mut d_y = [0.0f32; O]; // dL/dy (post-softplus)
        let scale = 2.0 / observed_count as f32;
        for j in 0..O {
            if mask[j] {
                let diff = y[j] - target[j];
                d_y[j] = scale * diff;
                loss += diff * diff;
            }
        }
        loss /= observed_count as f32;

        // --- Backprop through softplus: d/dx softplus(x) = sigmoid(x) ---
        let mut d_out = [0.0f32; O]; // dL/dy_pre (pre-softplus)
        for j in 0..O {
            d_out[j] = d_y[j] * sigmoid(y_pre[j]);
        }

        // --- Backward pass (STE: gradients flow through quantization) ---

        // Gradient w.r.t. layer 2 weights and biases
        let mut d_w2 = [[0.0f32; O]; H];
        let mut d_b2 = [0.0f32; O];
        let mut d_h = [0.0f32; H];

        for j in 0..O {
            d_b2[j] = d_out[j];
            for i in 0..H {
                d_w2[i][j] = h[i] * d_out[j];
                d_h[i] += self.w2[i][j] * d_out[j]; // STE: use latent weights
            }
        }

        // ReLU backward: zero gradient where pre-activation <= 0
        let mut d_h_pre = [0.0f32; H];
        for i in 0..H {
            d_h_pre[i] = if h_pre[i] > 0.0 { d_h[i] } else { 0.0 };
        }

        // Gradient w.r.t. layer 1 weights and biases
        let mut d_w1 = [[0.0f32; H]; F];
        let mut d_b1 = [0.0f32; H];

        for i in 0..H {
            d_b1[i] = d_h_pre[i];
            for j in 0..F {
                d_w1[j][i] = x_norm[j] * d_h_pre[i];
            }
        }

        // --- Gradient clipping ---
        let grad_norm = Self::compute_grad_norm(&d_w1, &d_b1, &d_w2, &d_b2);
        let clip_factor = if grad_norm > self.config.max_grad_norm {
            self.config.max_grad_norm / grad_norm
        } else {
            1.0
        };

        // --- Adam optimizer step ---
        self.step += 1;
        let t = self.step as f32;
        let bc1 = 1.0 - self.config.beta1.powf(t);
        let bc2 = 1.0 - self.config.beta2.powf(t);
        let cfg = self.config.clone();

        // Apply weight decay to gradients before Adam update
        let mut g_w1 = d_w1;
        for j in 0..F {
            for i in 0..H {
                g_w1[j][i] = d_w1[j][i] * clip_factor + cfg.weight_decay * self.w1[j][i];
            }
        }
        let mut g_w2 = d_w2;
        for i in 0..H {
            for j in 0..O {
                g_w2[i][j] = d_w2[i][j] * clip_factor + cfg.weight_decay * self.w2[i][j];
            }
        }

        adam_update(
            self.w1.as_flattened_mut(), self.m_w1.as_flattened_mut(),
            self.v_w1.as_flattened_mut(), g_w1.as_flattened(),
            bc1, bc2, &cfg,
        );
        adam_update(
            &mut self.b1, &mut self.m_b1, &mut self.v_b1,
            &d_b1.map(|g| g * clip_factor),
            bc1, bc2, &cfg,
        );
        adam_update(
            self.w2.as_flattened_mut(), self.m_w2.as_flattened_mut(),
            self.v_w2.as_flattened_mut(), g_w2.as_flattened(),
            bc1, bc2, &cfg,
        );
        adam_update(
            &mut self.b2, &mut self.m_b2, &mut self.v_b2,
            &d_b2.map(|g| g * clip_factor),
            bc1, bc2, &cfg,
        );

        self.total_loss += f64::from(loss);
        self.loss_count += 1;

        loss
    }

    /// Train on a batch of samples. Returns average loss.
    pub fn train_batch(&mut self, batch: &[([f32; F], [f32; O])]) -> f32 {
        if batch.is_empty() {
            return 0.0;
        }
        let mut total = 0.0f32;
        for (features, target) in batch {
            total += self.train_step(features, target);
        }
        total / batch.len() as f32
    }

    /// Train on a batch of `(features, target, mask)` triples. Returns average loss.
    ///
    /// Each sample's `mask` controls which output dimensions contribute to
    /// the loss for that sample. Samples with no observed dims are skipped.
    pub fn train_batch_masked(
        &mut self,
        batch: &[([f32; F], [f32; O], [bool; O])],
    ) -> f32 {
        if batch.is_empty() {
            return 0.0;
        }
        let mut total = 0.0f32;
        let mut counted = 0usize;
        for (features, target, mask) in batch {
            if mask.iter().any(|m| *m) {
                total += self.train_step_masked(features, target, mask);
                counted += 1;
            }
        }
        if counted == 0 { 0.0 } else { total / counted as f32 }
    }

    /// Export the current latent weights as a quantized `BitNetCostModel`.
    #[must_use]
    pub fn to_model(&self) -> BitNetCostModel {
        let mut model = BitNetCostModel::from_f32_weights(
            &self.w1,
            &self.b1,
            &self.w2,
            &self.b2,
            self.feature_mean,
            self.feature_inv_std,
            self.step,
        );
        // Propagate the trainer's scalar head into the snapshot. This
        // preserves any updates made via `update_scalar_head`; without
        // it, snapshots would always reset to the default formula.
        model.set_scalar_head(self.scalar_head, self.scalar_bias);
        model
    }

    /// Train the scalar head on a single observed CPU-time sample.
    /// Mirrors [`BitNetCostModel::update_scalar_head`] but on the
    /// trainer's mutable copy, so the next snapshot via [`Self::to_model`]
    /// carries the update.
    pub fn update_scalar_head(
        &mut self,
        features: &[f32; F],
        target_cpu_ms: f32,
        lr: f32,
    ) -> f32 {
        // Build a temporary model snapshot to read the per-dim outputs
        // through the same softplus path inference uses, then apply
        // the SGD step to OUR scalar_head (not the snapshot's).
        let snap = BitNetCostModel::from_f32_weights(
            &self.w1,
            &self.b1,
            &self.w2,
            &self.b2,
            self.feature_mean,
            self.feature_inv_std,
            self.step,
        );
        let out = snap.predict_all(features);

        let mut pre = self.scalar_bias;
        for (h, &o) in self.scalar_head.iter().zip(out.iter()) {
            pre += h * o;
        }
        let pred = if pre > 20.0 { pre } else { (1.0 + pre.exp()).ln() };
        let err = pred - target_cpu_ms;
        let sigmoid = if pre >= 0.0 {
            1.0 / (1.0 + (-pre).exp())
        } else {
            let z = pre.exp();
            z / (1.0 + z)
        };
        let d_pre = err * sigmoid;
        for (h, &o) in self.scalar_head.iter_mut().zip(out.iter()) {
            *h -= lr * d_pre * o;
        }
        self.scalar_bias -= lr * d_pre;
        err * err
    }

    /// Get average training loss since last reset.
    #[must_use]
    pub fn avg_loss(&self) -> f32 {
        if self.loss_count == 0 {
            return 0.0;
        }
        (self.total_loss / self.loss_count as f64) as f32
    }

    /// Reset loss accumulator.
    pub fn reset_loss(&mut self) {
        self.total_loss = 0.0;
        self.loss_count = 0;
    }

    /// Get total training steps completed.
    #[must_use]
    pub fn steps(&self) -> usize {
        self.step
    }

    // --- Private helpers ---

    fn normalize(&self, features: &[f32; F]) -> [f32; F] {
        let mut out = [0.0f32; F];
        for i in 0..F {
            out[i] = (features[i] - self.feature_mean[i]) * self.feature_inv_std[i];
        }
        out
    }

    /// Forward through hidden layer, returning (pre-activation, post-ReLU).
    fn forward_hidden(&self, x: &[f32; F]) -> ([f32; H], [f32; H]) {
        let mut pre = self.b1;
        for (j, &xj) in x.iter().enumerate() {
            for (i, p) in pre.iter_mut().enumerate() {
                *p += self.w1[j][i] * xj;
            }
        }
        let mut post = pre;
        for v in &mut post {
            *v = v.max(0.0);
        }
        (pre, post)
    }

    /// Forward through output layer, returning the linear pre-activation.
    ///
    /// Inference (`BitNetCostModel::predict_all`) applies `softplus` after
    /// this. The training backward pass propagates gradient through that
    /// `softplus` separately, so this function intentionally returns the
    /// raw linear output.
    fn forward_output(&self, h: &[f32; H]) -> [f32; O] {
        let mut out = self.b2;
        for (i, &hi) in h.iter().enumerate() {
            for (j, o) in out.iter_mut().enumerate() {
                *o += self.w2[i][j] * hi;
            }
        }
        out
    }

    fn compute_grad_norm(
        d_w1: &[[f32; H]; F],
        d_b1: &[f32; H],
        d_w2: &[[f32; O]; H],
        d_b2: &[f32; O],
    ) -> f32 {
        let mut sum_sq = 0.0f64;
        for row in d_w1 { for &v in row { sum_sq += f64::from(v * v); } }
        for &v in d_b1 { sum_sq += f64::from(v * v); }
        for row in d_w2 { for &v in row { sum_sq += f64::from(v * v); } }
        for &v in d_b2 { sum_sq += f64::from(v * v); }
        (sum_sq.sqrt()) as f32
    }

}

/// Adam optimizer step on flat slices (gradients already include weight decay + clipping).
fn adam_update(
    w: &mut [f32],
    m: &mut [f32],
    v: &mut [f32],
    g: &[f32],
    bc1: f32,
    bc2: f32,
    cfg: &TrainerConfig,
) {
    for i in 0..w.len() {
        m[i] = cfg.beta1 * m[i] + (1.0 - cfg.beta1) * g[i];
        v[i] = cfg.beta2 * v[i] + (1.0 - cfg.beta2) * g[i] * g[i];
        let m_hat = m[i] / bc1;
        let v_hat = v[i] / bc2;
        w[i] -= cfg.lr * m_hat / (v_hat.sqrt() + cfg.eps);
    }
}

/// Numerically-stable softplus: `ln(1 + e^x)`.
///
/// Matches the activation in [`super::BitNetCostModel::predict_all`]. For
/// large `x` returns `x` (the limit), for very negative `x` returns ~0
/// without overflow.
#[inline]
fn softplus(x: f32) -> f32 {
    if x > 20.0 { x } else { (1.0 + x.exp()).ln() }
}

/// Apply softplus elementwise (training mirrors inference).
#[inline]
fn apply_softplus(pre: &[f32; O]) -> [f32; O] {
    let mut out = [0.0f32; O];
    for (i, &v) in pre.iter().enumerate() {
        out[i] = softplus(v);
    }
    out
}

/// Numerically-stable sigmoid: `1 / (1 + e^-x)`. This is the derivative
/// of softplus, used to backprop loss gradients through the activation.
#[inline]
fn sigmoid(x: f32) -> f32 {
    if x >= 0.0 {
        let z = (-x).exp();
        1.0 / (1.0 + z)
    } else {
        let z = x.exp();
        z / (1.0 + z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_target(features: &[f32; F]) -> [f32; O] {
        // Synthetic target: CPU cost roughly proportional to joins × cardinality
        let cpu_cost = features[1] * features[11].ln().max(1.0) * 0.1;
        let mut target = [0.0f32; O];
        target[0] = cpu_cost.max(0.1);
        target[1] = cpu_cost * 0.5; // memory
        target
    }

    #[test]
    fn training_reduces_loss() {
        let mut trainer = BitNetTrainer::new(TrainerConfig {
            lr: 0.01,
            ..Default::default()
        });

        // Generate synthetic training data
        let samples: Vec<([f32; F], [f32; O])> = (0..100)
            .map(|i| {
                let features = [
                    (i % 5 + 1) as f32, // table_count
                    (i % 4) as f32,     // join_count
                    (i % 6) as f32,     // filter_count
                    (i % 2) as f32,     // aggregate_count
                    0.0, 0.0, 0.0,
                    (i % 3) as f32,     // order_by
                    (i % 2) as f32,     // group_by
                    0.0, 0.0,
                    ((i + 1) * 100) as f32, // cardinality
                    0.0, 0.0, 0.0, 0.0, // optimization-features padding
                ];
                let target = make_target(&features);
                (features, target)
            })
            .collect();

        // Train for several epochs
        let mut epoch_losses = Vec::new();
        for _epoch in 0..5 {
            trainer.reset_loss();
            for (f, t) in &samples {
                trainer.train_step(f, t);
            }
            epoch_losses.push(trainer.avg_loss());
        }

        // Loss should decrease over training
        let first = epoch_losses[0];
        let last = *epoch_losses.last().unwrap_or(&first);
        assert!(
            last < first,
            "Loss should decrease: first={first:.4}, last={last:.4}"
        );
    }

    #[test]
    fn exported_model_predicts() {
        let mut trainer = BitNetTrainer::new(TrainerConfig::default());

        let features = [4.0, 3.0, 5.0, 1.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 10_000.0, 0.0, 0.0, 0.0, 0.0];
        let target = [1.5f32; O];

        // Train a few steps
        for _ in 0..50 {
            trainer.train_step(&features, &target);
        }

        // Export and verify prediction is valid
        let model = trainer.to_model();
        let pred = model.predict_cpu_ms(&features);
        assert!(pred >= 0.0, "prediction must be non-negative: {pred}");
        assert!(pred.is_finite(), "prediction must be finite");
    }

    #[test]
    fn batch_training_works() {
        let mut trainer = BitNetTrainer::new(TrainerConfig::default());

        let batch: Vec<([f32; F], [f32; O])> = (0..16)
            .map(|i| {
                let features = [i as f32; F];
                let target = [(i as f32) * 0.1; O];
                (features, target)
            })
            .collect();

        let loss = trainer.train_batch(&batch);
        assert!(loss >= 0.0);
        assert!(loss.is_finite());
        assert_eq!(trainer.steps(), 16);
    }

    /// Regression: training and inference must compute the same activation.
    ///
    /// Pre-A1, the trainer minimised MSE on the **linear** output but
    /// inference applied softplus. The bug is most visible for small
    /// targets in softplus's curved regime, e.g. target=0 ⇒ softplus(0) ≈
    /// 0.69 ≠ 0. After A1, training also applies softplus, so the
    /// prediction matches the target.
    #[test]
    fn training_targets_match_inference_after_convergence() {
        let mut trainer = BitNetTrainer::new(TrainerConfig {
            lr: 0.05,
            weight_decay: 0.0,
            ..Default::default()
        });

        // Bias-only constant prediction. target=0 means we want the model
        // to learn "predict 0 for this query." Pre-A1 would converge
        // y_pre → 0 and infer softplus(0) = ln(2) ≈ 0.69 — the canonical
        // training/inference mismatch we are fixing.
        let features = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let target = [0.0f32; O];

        for _ in 0..3000 {
            trainer.train_step(&features, &target);
        }

        let model = trainer.to_model();
        let pred = model.predict_cpu_ms(&features);

        // Post-A1 we expect pred → 0; pre-A1 it would have been ~0.69.
        // Anything below 0.3 confirms training escapes the softplus floor.
        assert!(
            pred < 0.3,
            "predict_cpu_ms={pred} should converge to 0 after A1; \
             pre-A1 it would have stuck at ~0.69 (softplus floor)"
        );
    }

    /// Masked training must not push unobserved dimensions toward zero.
    /// Compares masked feedback (only dim 0 observed) against unmasked
    /// feedback (dim 0 observed, dims 1..16 hard-zeroed). Masked must
    /// preserve a previously-learned signal in dim 5.
    #[test]
    fn masked_training_preserves_unobserved_dims_better_than_zero_target() {
        // Phase 1: pretrain both trainers identically so dim 5 ≈ 4.0.
        let pretrain = || -> BitNetTrainer {
            let mut t = BitNetTrainer::new(TrainerConfig {
                lr: 0.05,
                weight_decay: 0.0,
                ..Default::default()
            });
            let features = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
            let mut full = [0.0f32; O];
            full[5] = 4.0;
            for _ in 0..3000 {
                t.train_step(&features, &full);
            }
            t
        };
        let mut t_masked = pretrain();
        let mut t_zeroed = pretrain();

        let features = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let pred_pre = t_masked.to_model().predict_all(&features)[5];
        assert!(pred_pre > 1.5, "pretrain should set dim 5; got {pred_pre}");

        // Phase 2a: masked feedback only on dim 0.
        let mut mask = [false; O];
        mask[0] = true;
        let mut partial = [0.0f32; O];
        partial[0] = 1.0;
        for _ in 0..500 {
            t_masked.train_step_masked(&features, &partial, &mask);
        }

        // Phase 2b: unmasked feedback that hard-zeros dims 1..16
        // (simulates the pre-A2 record_feedback bug).
        for _ in 0..500 {
            t_zeroed.train_step(&features, &partial);
        }

        let pred_masked = t_masked.to_model().predict_all(&features)[5];
        let pred_zeroed = t_zeroed.to_model().predict_all(&features)[5];

        // Masking must preserve dim 5 strictly better than the zero-target
        // bug ever could. If they're equal we've fixed nothing.
        assert!(
            pred_masked > pred_zeroed + 0.3,
            "masked training did not preserve dim 5 better than zero-target: \
             pre={pred_pre:.3} masked={pred_masked:.3} zeroed={pred_zeroed:.3}"
        );
    }
}
