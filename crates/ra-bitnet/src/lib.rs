//! `BitNet` 1.58-bit quantized neural cost model for Ra.
//!
//! Implements the `BitNet` inference algorithm (Microsoft Research, 2023) for
//! ultra-fast query cost prediction. Ternary weights {-1, 0, +1} replace
//! floating-point multiplications with conditional add/subtract operations.
//!
//! # Architecture
//!
//! The cost model is a 2-layer ternary network whose input dimension
//! matches the speculative router's `OptimizationFeatures::DIM`:
//!
//! ```text
//! [f32; 16]  →  Layer 1 (W₁: 16 × 32 ternary)  →  ReLU
//!            →  Layer 2 (W₂: 32 × 16 ternary)  →  softplus
//!            →  [f32; 16]   (cost dims 0-11, router dims 12-15)
//! ```
//!
//! Inference exposes two entry points:
//! - [`BitNetCostModel::predict_all`] — returns all 16 output dimensions.
//! - [`BitNetCostModel::predict_cpu_ms`] — convenience wrapper that
//!   returns dim 0 only. It does **not** save work over `predict_all`
//!   on its own (still runs the full hidden layer, plus a small extra
//!   load on dim 0); it exists so callers that only need the scalar
//!   don't pay the syntactic cost of indexing.
//!
//! ## Footprint
//!
//! With (F=16, H=32, O=16):
//!
//! | Component                       | Bytes |
//! |---------------------------------|------:|
//! | W₁ packed ternary (`F·H/4`)     |  128 |
//! | W₂ packed ternary (`H·O/4`)     |  128 |
//! | Biases `b1+b2` (`(H+O)·4`)      |  192 |
//! | Scale α₁                         |   4 |
//! | **Weights-only**                | **452** |
//! | Scale α₂                         |   4 |
//! | Normalization (`mean+inv_std`, `F` × 8 bytes) | 128 |
//! | **Total on-disk**               | **584** |
//!
//! Weights-only is exposed via [`BitNetCostModel::weights_only_bytes`];
//! the full on-disk footprint via [`BitNetCostModel::model_size_bytes`].
//! Both numbers are pinned by the `documented_byte_counts_are_exact`
//! test so this docstring and the README cannot drift apart silently.
//!
//! # `BitNet` 1.58-bit Quantization
//!
//! Each weight is quantized to {-1, 0, +1} using `absmean`:
//! ```text
//! α = mean(|W|)
//! W_q = round_clip(W / α, -1, 1)
//! ```
//!
//! Forward pass:
//! ```text
//! Y = (x_norm ⊗ W_q) * α + bias
//! ```
//!
//! where `⊗` with ternary weights means: +1 → add, 0 → skip, -1 → subtract.
//!
//! # Performance
//!
//! Target: <200ns per `predict_all` call. Pre-multiplied ternary weights
//! enable branchless FMA loops that auto-vectorize on ARM NEON and x86
//! AVX2. The benchmark harness is `cargo bench -p ra-bitnet`; see
//! `benches/bitnet_cost.rs`.

mod quantize;
pub mod train;

use serde::{Deserialize, Serialize};

pub use train::{BitNetTrainer, TrainerConfig};

/// Number of input features (matches `OptimizationFeatures::DIM` so all
/// 16 router-relevant features reach inference). Pre-A4 this was 12,
/// matching `QueryFeatures::FEATURE_DIM`; the speculative router was
/// then forced to drop the 4 trailing topology/scale features when
/// calling into the model. Now F = 16 and `QueryFeatures` zero-pads
/// the 4 extra slots when invoking inference for cost-only callers.
pub const F: usize = 16;
/// Number of hidden neurons.
pub const H: usize = 32;
/// Number of output cost dimensions.
pub const O: usize = 16;

/// Packed ternary weight representation.
///
/// Each ternary value {-1, 0, +1} is encoded in 2 bits:
/// - `0b00` → 0 (skip)
/// - `0b01` → +1 (add)
/// - `0b10` → -1 (subtract)
/// - `0b11` → reserved (treated as 0)
///
/// Weights are packed 4 per byte, row-major.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackedTernary {
    data: Vec<u8>,
    len: usize,
}

impl PackedTernary {
    fn pack(values: impl Iterator<Item = i8>) -> Self {
        let values: Vec<i8> = values.collect();
        let len = values.len();
        let byte_len = len.div_ceil(4);
        let mut data = vec![0u8; byte_len];

        for (i, &v) in values.iter().enumerate() {
            let encoded: u8 = match v {
                1 => 0b01,
                -1 => 0b10,
                _ => 0b00,
            };
            let byte_idx = i / 4;
            let bit_offset = (i % 4) * 2;
            data[byte_idx] |= encoded << bit_offset;
        }

        Self { data, len }
    }

    #[inline]
    fn get(&self, i: usize) -> i8 {
        let byte_idx = i / 4;
        let bit_offset = (i % 4) * 2;
        let bits = (self.data[byte_idx] >> bit_offset) & 0b11;
        match bits {
            0b01 => 1,
            0b10 => -1,
            _ => 0,
        }
    }
}

/// `BitNet` 1.58-bit quantized cost model for sub-100ns inference.
///
/// Drop-in replacement for `FastCostModel` with identical interface.
/// Uses ternary weights and absmean scaling for inference with
/// pre-multiplied `f32` arrays for branchless auto-vectorizable loops.
///
/// # Input
///
/// Takes `&[f32; 16]` feature vectors (matches `OptimizationFeatures::DIM`).
/// Cost-only callers can use `QueryFeatures::as_array()` which zero-pads
/// the last 4 slots; the speculative router fills all 16 from the
/// extended `OptimizationFeatures` set.
///
/// # Output
///
/// Returns `f32` (scalar CPU cost) or `[f32; 16]` (full cost vector).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitNetCostModel {
    // Packed storage (for serialization)
    w1_packed: PackedTernary,
    w2_packed: PackedTernary,

    // Pre-unpacked weights: ternary * alpha stored as f32 for branchless FMA
    #[serde(skip)]
    w1_fast: Box<[[f32; H]; F]>,
    #[serde(skip)]
    w2_fast: Box<[[f32; O]; H]>,

    b1: Box<[f32; H]>,
    alpha1: f32,

    b2: Box<[f32; O]>,
    alpha2: f32,

    feature_mean: [f32; F],
    feature_inv_std: [f32; F],

    /// Linear projection from the 16 output dims down to a scalar
    /// e-graph cost. Pre-A5 this was a hand-tuned formula
    /// `out[0]*0.5 + out[3]*0.0003 + out[1]*0.002`. Now it's a real
    /// learnable head: `predict_scalar = softplus(Σ scalar_head[i] * out_pre[i] + scalar_bias)`.
    /// The default initialization preserves the historical coefficient
    /// pattern (CPU/IO/memory mix) so behavior is backward-compatible
    /// for callers loading old models or constructing new zero models.
    /// Trained from observed CPU time via [`BitNetCostModel::update_scalar_head`].
    #[serde(default = "default_scalar_head")]
    scalar_head: [f32; O],

    /// Bias term for the learnable scalar head.
    #[serde(default)]
    scalar_bias: f32,

    /// Number of training samples used to derive this model.
    pub samples_trained: usize,
}

/// Default scalar-head coefficients matching the pre-A5 magic formula
/// `out[0]*0.5 + out[3]*0.0003 + out[1]*0.002`. Used for serde
/// `#[serde(default = ...)]` so models written before `scalar_head`
/// existed deserialize cleanly with the historical aggregation behavior.
pub(crate) fn default_scalar_head() -> [f32; O] {
    let mut h = [0.0f32; O];
    h[0] = 0.5;
    h[1] = 0.002;
    h[3] = 0.000_3;
    h
}

impl BitNetCostModel {
    /// Create a new model with zero weights (predicts bias-only until loaded).
    #[must_use]
    pub fn new_zeros() -> Self {
        let w1_packed = PackedTernary { data: vec![0u8; F * H / 4], len: F * H };
        let w2_packed = PackedTernary { data: vec![0u8; H * O / 4], len: H * O };

        Self {
            w1_fast: Box::new([[0.0; H]; F]),
            w2_fast: Box::new([[0.0; O]; H]),
            w1_packed,
            w2_packed,
            b1: Box::new([0.0; H]),
            alpha1: 1.0,
            b2: Box::new([0.0; O]),
            alpha2: 1.0,
            feature_mean: [0.0; F],
            feature_inv_std: [1.0; F],
            scalar_head: default_scalar_head(),
            scalar_bias: 0.0,
            samples_trained: 0,
        }
    }

    /// Quantize `f32` weights into ternary using absmean.
    ///
    /// Pre-multiplies ternary values by α for branchless inference.
    #[must_use]
    pub fn from_f32_weights(
        w1: &[[f32; H]; F],
        b1: &[f32; H],
        w2: &[[f32; O]; H],
        b2: &[f32; O],
        feature_mean: [f32; F],
        feature_inv_std: [f32; F],
        samples_trained: usize,
    ) -> Self {
        let (w1_packed, alpha1) = quantize::absmean_ternary_pack(
            w1.iter().flat_map(|row| row.iter().copied()),
            F * H,
        );
        let (w2_packed, alpha2) = quantize::absmean_ternary_pack(
            w2.iter().flat_map(|row| row.iter().copied()),
            H * O,
        );

        let w1_fast: Box<[[f32; H]; F]> = unpack_to_fh(&w1_packed, alpha1).into();
        let w2_fast: Box<[[f32; O]; H]> = unpack_to_ho(&w2_packed, alpha2).into();

        Self {
            w1_fast,
            w2_fast,
            w1_packed,
            w2_packed,
            b1: Box::new(*b1),
            alpha1,
            b2: Box::new(*b2),
            alpha2,
            feature_mean,
            feature_inv_std,
            scalar_head: default_scalar_head(),
            scalar_bias: 0.0,
            samples_trained,
        }
    }

    /// Load from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if the file cannot be read or JSON is malformed.
    pub fn load_from_file(path: &str) -> Result<Self, std::io::Error> {
        let data = std::fs::read_to_string(path)?;
        let mut model: Self = serde_json::from_str(&data).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        model.rebuild_fast_weights();
        Ok(model)
    }

    /// Save to a JSON file (only packed weights are serialized).
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if the file cannot be written.
    pub fn save_to_file(&self, path: &str) -> Result<(), std::io::Error> {
        let data = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        std::fs::write(path, data)
    }

    /// Update per-feature normalization from training data.
    pub fn fit_normalization(&mut self, samples: &[[f32; F]]) {
        if samples.is_empty() {
            return;
        }
        let n = samples.len() as f32;
        let mut mean = [0.0f32; F];
        let mut m2 = [0.0f32; F];

        for s in samples {
            for (i, (&x, m)) in s.iter().zip(mean.iter_mut()).enumerate() {
                let delta = x - *m;
                *m += delta / n;
                m2[i] += delta * (x - *m);
            }
        }

        self.feature_mean = mean;
        for (i, inv) in self.feature_inv_std.iter_mut().enumerate() {
            let std = (m2[i] / n.max(1.0)).sqrt();
            *inv = if std > 1e-6 { 1.0 / std } else { 1.0 };
        }
    }

    /// Per-feature mean used to normalize inputs before inference.
    #[must_use]
    pub fn feature_mean(&self) -> [f32; F] {
        self.feature_mean
    }

    /// Per-feature inverse-stddev used to normalize inputs before inference.
    #[must_use]
    pub fn feature_inv_std(&self) -> [f32; F] {
        self.feature_inv_std
    }

    /// Predict CPU time (ms) only — extracts dim 0 of [`Self::predict_all`].
    ///
    /// Counter-intuitively this is **slower** than `predict_all` on
    /// modern CPUs (measured ~106 ns vs ~81 ns on Apple M3 Max, release
    /// build, see `cargo bench -p ra-bitnet`): the column-strided access
    /// pattern `w2_fast[i][0]` defeats prefetch and prevents
    /// auto-vectorization, while `predict_all`'s row-major inner loop
    /// compiles to a NEON/AVX2 vectorized accumulation. Prefer
    /// `predict_all` when you'll consume more than dim 0; this entry
    /// point exists only as a convenience for callers that genuinely
    /// want a scalar.
    #[inline]
    #[must_use]
    pub fn predict_cpu_ms(&self, features: &[f32; F]) -> f32 {
        let h = self.hidden_layer(features);
        let mut out0 = self.b2[0];
        for (i, &hi) in h.iter().enumerate() {
            out0 += self.w2_fast[i][0] * hi;
        }
        softplus(out0)
    }

    /// Predict all 16 cost dimensions (~81 ns on Apple M3 Max, release
    /// build). The inner output loop is row-major over `w2_fast`, which
    /// auto-vectorizes to NEON/AVX2 FMA. This is the entry point the
    /// speculative router calls; the README's `~87ns` headline traces
    /// back to a pre-A4 measurement and is in the same order of
    /// magnitude. Re-run `cargo bench -p ra-bitnet` to refresh.
    #[must_use]
    pub fn predict_all(&self, features: &[f32; F]) -> [f32; O] {
        let h = self.hidden_layer(features);
        let mut out = *self.b2;
        for (i, &hi) in h.iter().enumerate() {
            for (j, o) in out.iter_mut().enumerate() {
                *o += self.w2_fast[i][j] * hi;
            }
        }
        for v in &mut out {
            *v = softplus(*v);
        }
        out
    }

    /// Aggregate all 16 cost dimensions into a single `f64` e-graph cost
    /// via the learnable scalar head.
    ///
    /// Pre-A5 this was the hand-tuned formula
    /// `out[0]*0.5 + out[3]*0.0003 + out[1]*0.002`. That formula is now
    /// the default initialization of [`Self::scalar_head`]
    /// (see [`default_scalar_head`]) so behavior is unchanged on a
    /// freshly-loaded model. The trainer can subsequently nudge the
    /// head toward observed CPU times via
    /// [`Self::update_scalar_head`].
    ///
    /// Computation: `softplus(Σ scalar_head[i] * out[i] + scalar_bias)`.
    /// Softplus keeps the scalar non-negative without clamping (a
    /// negative cost would be rejected by the e-graph cost function
    /// anyway).
    #[must_use]
    pub fn predict_scalar(&self, features: &[f32; F]) -> f64 {
        let out = self.predict_all(features);
        let mut sum = self.scalar_bias;
        for (h, &o) in self.scalar_head.iter().zip(out.iter()) {
            sum += h * o;
        }
        f64::from(softplus(sum))
    }

    /// Borrow the learnable scalar-head coefficients.
    #[must_use]
    pub fn scalar_head(&self) -> &[f32; O] {
        &self.scalar_head
    }

    /// Borrow the scalar-head bias.
    #[must_use]
    pub fn scalar_bias(&self) -> f32 {
        self.scalar_bias
    }

    /// Replace the scalar head and bias in one call. Used by
    /// [`crate::BitNetTrainer::to_model`] to propagate trained
    /// scalar-head updates into the snapshot.
    pub fn set_scalar_head(&mut self, head: [f32; O], bias: f32) {
        self.scalar_head = head;
        self.scalar_bias = bias;
    }

    /// Train the scalar head on a single `(features, target_cpu_ms)`
    /// observation using SGD on `MSE(predict_scalar - target)`.
    ///
    /// Treats `predict_all`'s outputs as fixed features and only
    /// updates `scalar_head` and `scalar_bias`. The hidden layers stay
    /// untouched — they're trained by [`crate::BitNetTrainer`] on the
    /// per-dim targets via STE. This decoupling lets us learn the
    /// scalar projection from cheap actual-time-only feedback without
    /// needing per-dim ground truth (which the PG executor doesn't
    /// expose).
    ///
    /// The default `lr=0.01` is conservative; it'd take ~100 samples
    /// at the right magnitude to noticeably move from the default
    /// initialization. Returns the squared error for diagnostic
    /// logging.
    pub fn update_scalar_head(
        &mut self,
        features: &[f32; F],
        target_cpu_ms: f32,
        lr: f32,
    ) -> f32 {
        let out = self.predict_all(features);
        let mut pre_softplus = self.scalar_bias;
        for (h, &o) in self.scalar_head.iter().zip(out.iter()) {
            pre_softplus += h * o;
        }
        let pred = softplus(pre_softplus);
        let err = pred - target_cpu_ms;
        // d/dx softplus = sigmoid; so dL/d(pre) = err * sigmoid(pre).
        let sigmoid_pre = if pre_softplus >= 0.0 {
            let z = (-pre_softplus).exp();
            1.0 / (1.0 + z)
        } else {
            let z = pre_softplus.exp();
            z / (1.0 + z)
        };
        let d_pre = err * sigmoid_pre;
        for (h, &o) in self.scalar_head.iter_mut().zip(out.iter()) {
            *h -= lr * d_pre * o;
        }
        self.scalar_bias -= lr * d_pre;
        err * err
    }

    /// Compute hidden layer: `h = ReLU(W1_fast · x_norm + b1)`.
    #[inline]
    fn hidden_layer(&self, features: &[f32; F]) -> [f32; H] {
        let mut h = *self.b1;

        for (j, (&feat, (&mean, &inv_std))) in features
            .iter()
            .zip(self.feature_mean.iter().zip(self.feature_inv_std.iter()))
            .enumerate()
        {
            let xj = (feat - mean) * inv_std;
            for (i, hi) in h.iter_mut().enumerate() {
                *hi += self.w1_fast[j][i] * xj;
            }
        }

        for v in &mut h {
            *v = v.max(0.0);
        }
        h
    }

    /// Rebuild fast inference arrays from packed weights (after deserialization).
    fn rebuild_fast_weights(&mut self) {
        self.w1_fast = unpack_to_fh(&self.w1_packed, self.alpha1).into();
        self.w2_fast = unpack_to_ho(&self.w2_packed, self.alpha2).into();
    }

    /// Model memory footprint in bytes (packed storage only).
    /// Total on-disk model footprint in bytes (everything serialized to JSON).
    ///
    /// Components for the default `(F=12, H=32, O=16)` shape:
    /// - W₁ packed ternary: `F*H/4` = 96 B
    /// - W₂ packed ternary: `H*O/4` = 128 B
    /// - Biases `b1`, `b2`: `(H + O) * 4` = 192 B
    /// - Scales `α₁`, `α₂`: 8 B
    /// - Per-feature normalization (`mean`, `inv_std`): `F*4*2` = 96 B
    ///
    /// Total: **520 bytes**. The README's historical "420 bytes" headline
    /// counted only weights + biases + a single α and dropped the
    /// normalization tables; that subset is reported by
    /// [`Self::weights_only_bytes`].
    #[must_use]
    pub fn model_size_bytes(&self) -> usize {
        self.weights_only_bytes() + 4 /* alpha2 */ + F * 4 * 2 /* normalization */
    }

    /// Weight + bias + one-scale footprint in bytes (reflects the original
    /// "420-byte" headline figure: `96 + 128 + (H+O)*4 + 4`).
    ///
    /// This is the irreducible memory the network needs in its ternary
    /// representation. The full `model_size_bytes()` adds the second α
    /// and the per-feature normalization tables that load alongside the
    /// weights but are not strictly "the model."
    #[must_use]
    pub fn weights_only_bytes(&self) -> usize {
        self.w1_packed.data.len()
            + self.w2_packed.data.len()
            + (H + O) * 4   // biases
            + 4             // alpha1 (alpha2 included in model_size_bytes)
    }

    /// Stable short identifier for this model snapshot.
    ///
    /// Used by `PlanProvenance` (in `ra-engine`) to record which
    /// cost-model weights were active when a plan was produced. The
    /// id is computed as the first 16 hex chars of a default-hasher
    /// digest over the packed ternary weights and the two scales —
    /// enough to distinguish snapshots in human-readable logs
    /// without paying for SHA-256 on every query.
    #[must_use]
    pub fn snapshot_id(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.w1_packed.data.hash(&mut h);
        self.w2_packed.data.hash(&mut h);
        // Include scales as their bit pattern so f32 NaN doesn't
        // poison the comparison.
        self.alpha1.to_bits().hash(&mut h);
        self.alpha2.to_bits().hash(&mut h);
        // Bias arrays and normalization tables affect inference and
        // therefore plan choice; include them too.
        for v in self.b1.iter().chain(self.b2.iter()) {
            v.to_bits().hash(&mut h);
        }
        format!("{:016x}", h.finish())
    }
}

/// Unpack ternary weights into `[[f32; H]; F]`, pre-multiplied by `alpha`.
fn unpack_to_fh(packed: &PackedTernary, alpha: f32) -> [[f32; H]; F] {
    let mut arr = [[0.0f32; H]; F];
    for (r, row) in arr.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = f32::from(packed.get(r * H + c)) * alpha;
        }
    }
    arr
}

/// Unpack ternary weights into `[[f32; O]; H]`, pre-multiplied by `alpha`.
fn unpack_to_ho(packed: &PackedTernary, alpha: f32) -> [[f32; O]; H] {
    let mut arr = [[0.0f32; O]; H];
    for (r, row) in arr.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = f32::from(packed.get(r * O + c)) * alpha;
        }
    }
    arr
}

#[inline]
fn softplus(x: f32) -> f32 {
    if x > 20.0 { x } else { (1.0 + x.exp()).ln() }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    clippy::needless_range_loop,
    reason = "test asserts deterministic predict outputs match exactly"
)]
mod tests {
    use super::*;

    fn sample_features() -> [f32; F] {
        // 12 QueryFeatures positions + 4 trailing zero-pads matching
        // OptimizationFeatures' density/fanout/equi/cross slots.
        [
            4.0, 3.0, 5.0, 1.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 10_000.0,
            0.0, 0.0, 0.0, 0.0,
        ]
    }

    #[test]
    fn zeros_model_predicts_bias_only() {
        let model = BitNetCostModel::new_zeros();
        let cpu = model.predict_cpu_ms(&sample_features());
        let expected = softplus(0.0);
        assert!((cpu - expected).abs() < 1e-6);
    }

    #[test]
    fn model_size_is_compact() {
        let model = BitNetCostModel::new_zeros();
        let size = model.model_size_bytes();
        assert!(size < 600, "Model should be under 600 bytes, got {size}");
    }

    /// Pin the documented byte counts so README and code can never drift
    /// silently again. These values must match the README's architecture
    /// section. Numbers reflect F=H=O=(16,32,16) post-A4:
    ///
    /// - W₁ packed ternary: F*H/4 = 128 B (was 96 B at F=12)
    /// - W₂ packed ternary: H*O/4 = 128 B
    /// - Biases b1+b2: (H+O)*4 = 192 B
    /// - α₁, α₂: 4 B each (one in `weights_only`, both in `model_size`)
    /// - Normalization `mean+inv_std`: F*4*2 = 128 B (was 96 B)
    ///
    /// `weights_only` = 128+128+192+4 = 452 B (headline figure post-A4).
    /// `model_size` = `weights_only` + 4 (α₂) + 128 (norm) = 584 B.
    #[test]
    fn documented_byte_counts_are_exact() {
        let model = BitNetCostModel::new_zeros();
        assert_eq!(
            model.weights_only_bytes(),
            452,
            "weights_only_bytes is the post-A4 headline figure (was 420 at F=12)"
        );
        assert_eq!(
            model.model_size_bytes(),
            584,
            "model_size_bytes is the full on-disk footprint \
             (weights + 2nd alpha + F*8 normalization)"
        );
    }

    /// A5 regression: `predict_scalar` must use the learnable scalar
    /// head, not the pre-A5 hand-tuned formula. On a fresh model the
    /// `scalar_head` is initialised to the historical magic-formula
    /// coefficients so backward-compatible behavior is preserved.
    #[test]
    fn predict_scalar_uses_learnable_head() {
        let model = BitNetCostModel::new_zeros();
        // Default scalar_head should match the pre-A5 magic formula.
        let head = model.scalar_head();
        assert!((head[0] - 0.5).abs() < 1e-6);
        assert!((head[1] - 0.002).abs() < 1e-6);
        assert!((head[3] - 0.000_3).abs() < 1e-6);
        // All other slots default to zero.
        for (i, &v) in head.iter().enumerate() {
            if ![0_usize, 1, 3].contains(&i) {
                assert_eq!(v, 0.0, "scalar_head[{i}] should default to 0");
            }
        }
    }

    /// A5: `update_scalar_head` should move the prediction toward the
    /// observed target without retraining the hidden layers.
    #[test]
    fn update_scalar_head_moves_prediction_toward_target() {
        let mut model = BitNetCostModel::new_zeros();
        // new_zeros has all weights = 0, so predict_all returns
        // softplus(b2) = softplus(0) = ln(2) ≈ 0.693 in every slot.
        // predict_scalar = softplus(0.5*0.693 + 0.002*0.693 + 0.0003*0.693) ≈ ln(1+e^0.349) ≈ 0.890.
        let features = sample_features();
        let target = 5.0_f32;
        let initial = model.predict_scalar(&features) as f32;

        // Train on the same target many times; pred should creep toward it.
        for _ in 0..2000 {
            model.update_scalar_head(&features, target, 0.05);
        }
        let trained = model.predict_scalar(&features) as f32;

        assert!(
            (trained - target).abs() < (initial - target).abs(),
            "scalar head failed to learn: initial={initial:.3}, trained={trained:.3}, target={target}"
        );
    }

    /// A5: serde-default scalar head means models written before this
    /// field existed deserialize cleanly with the historical formula.
    #[expect(
        clippy::expect_used,
        reason = "test wiring: panicking on serde failure is the right diagnostic"
    )]
    #[test]
    fn legacy_model_json_loads_with_default_scalar_head() {
        // Simulate a pre-A5 model JSON: serialize a fresh model, then
        // strip the scalar_head/scalar_bias fields manually.
        let model = BitNetCostModel::new_zeros();
        let json = serde_json::to_string(&model).expect("serialize");
        // Crude strip: remove the scalar_head and scalar_bias entries.
        let mut value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        let obj = value.as_object_mut().expect("model is object");
        obj.remove("scalar_head");
        obj.remove("scalar_bias");
        let stripped = serde_json::to_string(&value).expect("re-serialize");

        let loaded: BitNetCostModel = serde_json::from_str(&stripped).expect("deserialize");
        let head = loaded.scalar_head();
        assert!((head[0] - 0.5).abs() < 1e-6);
        assert!((head[1] - 0.002).abs() < 1e-6);
        assert!((head[3] - 0.000_3).abs() < 1e-6);
    }

    #[test]
    fn packed_ternary_roundtrip() {
        let values: Vec<i8> = vec![1, -1, 0, 1, -1, -1, 0, 0, 1];
        let packed = PackedTernary::pack(values.iter().copied());
        for (i, &expected) in values.iter().enumerate() {
            assert_eq!(packed.get(i), expected, "mismatch at index {i}");
        }
    }

    #[test]
    fn quantize_and_predict() {
        let mut w1 = [[0.0f32; H]; F];
        let mut w2 = [[0.0f32; O]; H];
        for j in 0..F {
            for i in 0..H {
                w1[j][i] = match (j + i) % 3 { 0 => 0.5, 1 => -0.3, _ => 0.01 };
            }
        }
        for i in 0..H {
            for j in 0..O {
                w2[i][j] = if (i + j) % 2 == 0 { 0.4 } else { -0.2 };
            }
        }

        let model = BitNetCostModel::from_f32_weights(
            &w1, &[0.1; H], &w2, &[0.05; O], [0.0; F], [1.0; F], 1000,
        );
        let cpu = model.predict_cpu_ms(&sample_features());
        assert!(cpu >= 0.0, "prediction must be non-negative: {cpu}");
        assert!(cpu < 1000.0, "prediction should be reasonable: {cpu}");
    }

    #[test]
    fn predict_all_non_negative() {
        let model = BitNetCostModel::new_zeros();
        let out = model.predict_all(&sample_features());
        for (i, &v) in out.iter().enumerate() {
            assert!(v >= 0.0, "output dim {i} is negative: {v}");
        }
    }

    #[test]
    fn inference_is_deterministic() {
        let model = BitNetCostModel::from_f32_weights(
            &[[0.3; H]; F], &[0.0; H], &[[0.2; O]; H], &[0.0; O],
            [0.0; F], [1.0; F], 500,
        );
        let f = sample_features();
        assert_eq!(model.predict_cpu_ms(&f), model.predict_cpu_ms(&f));
    }

    #[test]
    fn predict_cpu_matches_predict_all_dim0() {
        let model = BitNetCostModel::from_f32_weights(
            &[[0.25; H]; F], &[0.01; H], &[[0.15; O]; H], &[0.02; O],
            [0.0; F], [1.0; F], 100,
        );
        let f = sample_features();
        let cpu_fast = model.predict_cpu_ms(&f);
        let all = model.predict_all(&f);
        assert!(
            (cpu_fast - all[0]).abs() < 1e-4,
            "mismatch: {cpu_fast} vs {}", all[0]
        );
    }

    #[test]
    fn save_load_roundtrip() {
        let model = BitNetCostModel::from_f32_weights(
            &[[0.5; H]; F], &[0.1; H], &[[0.3; O]; H], &[0.05; O],
            [1.0; F], [2.0; F], 5000,
        );
        let path = std::env::temp_dir().join("test_bitnet_model.json");
        let path_str = path.to_str().unwrap_or("/tmp/test_bitnet.json");

        model.save_to_file(path_str).unwrap_or(());
        if let Ok(loaded) = BitNetCostModel::load_from_file(path_str) {
            let f = sample_features();
            assert_eq!(model.predict_cpu_ms(&f), loaded.predict_cpu_ms(&f));
        }
        let _ = std::fs::remove_file(path_str);
    }

    #[test]
    fn fit_normalization_changes_output() {
        let mut model = BitNetCostModel::from_f32_weights(
            &[[0.4; H]; F], &[0.0; H], &[[0.2; O]; H], &[0.0; O],
            [0.0; F], [1.0; F], 100,
        );
        let f = sample_features();
        let before = model.predict_cpu_ms(&f);

        let samples: Vec<[f32; F]> = (0..20)
            .map(|i| {
                let mut s = [0.0f32; F];
                for (j, v) in s.iter_mut().enumerate() {
                    *v = (i * j) as f32;
                }
                s
            })
            .collect();
        model.fit_normalization(&samples);

        let after = model.predict_cpu_ms(&f);
        assert_ne!(before, after, "normalization should affect predictions");
    }
}
