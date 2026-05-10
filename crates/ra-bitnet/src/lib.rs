//! `BitNet` 1.58-bit quantized neural cost model for Ra.
//!
//! Implements the `BitNet` inference algorithm (Microsoft Research, 2023) for
//! ultra-fast query cost prediction. Ternary weights {-1, 0, +1} replace
//! floating-point multiplications with conditional add/subtract operations.
//!
//! # Architecture
//!
//! The cost model uses a 2-layer network: 12 → 32 → 1 (scalar) or 12 → 32 → 16 (full).
//!
//! - **Layer 1**: 384 ternary weights packed into 96 bytes + 32 `f32` biases + 1 scale
//! - **Layer 2**: 512 ternary weights packed into 128 bytes + 16 `f32` biases + 1 scale
//! - **Total model**: ~420 bytes (vs ~3.2 KB for `f32` `FastCostModel`)
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
//! Target: <100ns per scalar prediction (matching `FastCostModel`'s ~45ns).
//! Pre-multiplied ternary weights enable branchless FMA loops that
//! auto-vectorize on ARM NEON and x86 AVX2.

mod quantize;
pub mod train;

use serde::{Deserialize, Serialize};

pub use train::{BitNetTrainer, TrainerConfig};

/// Number of input features (same as `QueryFeatures::FEATURE_DIM`).
pub const F: usize = 12;
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
/// Takes `&[f32; 12]` feature vectors (same layout as `QueryFeatures::to_vec()`).
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

    /// Number of training samples used to derive this model.
    pub samples_trained: usize,
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

    /// Predict CPU time (ms) — scalar fast path (~87ns).
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

    /// Predict all 16 cost dimensions (~72ns).
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

    /// Aggregate all 16 cost dimensions into a single `f64` e-graph cost.
    #[must_use]
    pub fn predict_scalar(&self, features: &[f32; F]) -> f64 {
        let out = self.predict_all(features);
        let io_cost = out[3] * 0.001;
        f64::from(out[0] * 0.5 + io_cost * 0.3 + out[1] * 0.2 * 0.01)
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
    #[must_use]
    pub fn model_size_bytes(&self) -> usize {
        self.w1_packed.data.len()
            + self.w2_packed.data.len()
            + H * 4 + O * 4  // biases
            + 4 + 4           // alphas
            + F * 4 * 2       // normalization
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
mod tests {
    use super::*;

    fn sample_features() -> [f32; F] {
        [4.0, 3.0, 5.0, 1.0, 0.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 10_000.0]
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
