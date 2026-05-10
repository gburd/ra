//! Absmean ternary quantization for `BitNet` 1.58-bit.
//!
//! Implements the quantization scheme from "The Era of 1-bit LLMs" (Microsoft, 2024):
//! ```text
//! α = mean(|W|)
//! W_q = round_clip(W / α, -1, 1)
//! ```
//!
//! Each weight is mapped to {-1, 0, +1} based on its magnitude relative to
//! the average absolute weight value.

use crate::PackedTernary;

/// Quantize f32 weights to packed ternary using absmean method.
///
/// Returns the packed weights and the scale factor α.
///
/// The scale factor captures the average magnitude of the original weights,
/// allowing the ternary representation to approximate the original values
/// when multiplied: `W_approx ≈ α * W_q`
pub fn absmean_ternary_pack(
    weights: impl Iterator<Item = f32>,
    count: usize,
) -> (PackedTernary, f32) {
    let weights: Vec<f32> = weights.collect();
    debug_assert_eq!(weights.len(), count);

    // Compute absmean: α = mean(|W|)
    let alpha = if count > 0 {
        let sum_abs: f32 = weights.iter().map(|w| w.abs()).sum();
        sum_abs / count as f32
    } else {
        1.0
    };

    // Avoid division by zero for all-zero weight matrices
    let alpha = if alpha < 1e-10 { 1.0 } else { alpha };

    // Quantize: W_q = round_clip(W / α, -1, 1)
    let ternary = weights.iter().map(|&w| {
        let scaled = w / alpha;
        round_clip(scaled)
    });

    (PackedTernary::pack(ternary), alpha)
}

/// Round and clip a scaled weight to ternary {-1, 0, +1}.
///
/// Uses the standard `BitNet` rounding: values near 0 stay 0,
/// values above threshold become +1, below become -1.
#[inline]
fn round_clip(x: f32) -> i8 {
    // Standard round-to-nearest with clip to [-1, 1]
    let rounded = x.round();
    if rounded >= 1.0 {
        1
    } else if rounded <= -1.0 {
        -1
    } else {
        0
    }
}

/// Compute the reconstruction error (RMSE) for quantized weights.
///
/// Useful for evaluating quantization quality:
/// `error = sqrt(mean((W - α * W_q)²))`
#[cfg(test)]
pub fn reconstruction_rmse(original: &[f32], packed: &PackedTernary, alpha: f32) -> f32 {
    if original.is_empty() {
        return 0.0;
    }

    let mut sum_sq_error = 0.0f64;
    for (i, &orig) in original.iter().enumerate() {
        let reconstructed = f64::from(alpha) * f64::from(packed.get(i));
        let error = f64::from(orig) - reconstructed;
        sum_sq_error += error * error;
    }

    (sum_sq_error / original.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_clip_values() {
        assert_eq!(round_clip(0.3), 0);
        assert_eq!(round_clip(0.6), 1);
        assert_eq!(round_clip(-0.6), -1);
        assert_eq!(round_clip(-0.3), 0);
        assert_eq!(round_clip(2.5), 1);
        assert_eq!(round_clip(-2.5), -1);
        assert_eq!(round_clip(0.0), 0);
    }

    #[test]
    fn absmean_quantization_basic() {
        // Weights: [1.0, -1.0, 0.1, 0.5, -0.5]
        // absmean = (1.0 + 1.0 + 0.1 + 0.5 + 0.5) / 5 = 0.62
        // scaled = [1.61, -1.61, 0.16, 0.81, -0.81]
        // ternary = [1, -1, 0, 1, -1]
        let weights = vec![1.0f32, -1.0, 0.1, 0.5, -0.5];
        let (packed, alpha) = absmean_ternary_pack(weights.iter().copied(), 5);

        assert!((alpha - 0.62).abs() < 0.01);
        assert_eq!(packed.get(0), 1);
        assert_eq!(packed.get(1), -1);
        assert_eq!(packed.get(2), 0);
        assert_eq!(packed.get(3), 1);
        assert_eq!(packed.get(4), -1);
    }

    #[test]
    fn absmean_all_zeros() {
        let weights = vec![0.0f32; 10];
        let (packed, alpha) = absmean_ternary_pack(weights.iter().copied(), 10);
        assert_eq!(alpha, 1.0); // fallback for zero weights
        for i in 0..10 {
            assert_eq!(packed.get(i), 0);
        }
    }

    #[test]
    fn reconstruction_error_bounded() {
        let weights: Vec<f32> = (0..100)
            .map(|i| (i as f32 - 50.0) * 0.02)
            .collect();
        let (packed, alpha) = absmean_ternary_pack(weights.iter().copied(), 100);
        let rmse = reconstruction_rmse(&weights, &packed, alpha);
        // RMSE should be bounded — ternary quantization loses precision but
        // reconstruction error should be less than the max weight magnitude
        assert!(rmse < 1.0, "RMSE too high: {rmse}");
    }

    #[test]
    fn uniform_positive_quantizes_to_all_ones() {
        let weights = vec![1.0f32; 8];
        let (packed, alpha) = absmean_ternary_pack(weights.iter().copied(), 8);
        assert!((alpha - 1.0).abs() < 1e-6);
        for i in 0..8 {
            assert_eq!(packed.get(i), 1);
        }
    }
}
