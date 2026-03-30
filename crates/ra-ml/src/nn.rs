//! Lightweight feed-forward neural network for inference.
//!
//! Implements a multi-layer perceptron (MLP) suitable for
//! cardinality estimation. Weights are loaded from serialized
//! JSON; training happens externally (e.g., in Python).

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from neural network operations.
#[derive(Debug, Error)]
pub enum NnError {
    /// Weight matrix dimensions do not match.
    #[error(
        "dimension mismatch: expected input size {expected}, \
         got {actual}"
    )]
    DimensionMismatch {
        /// Expected dimension.
        expected: usize,
        /// Actual dimension.
        actual: usize,
    },

    /// Model has no layers.
    #[error("model has no layers")]
    EmptyModel,

    /// JSON deserialization failed.
    #[error("failed to deserialize model: {0}")]
    Deserialize(#[from] serde_json::Error),
}

/// Activation function applied after each layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Activation {
    /// Rectified linear unit: max(0, x).
    ReLU,
    /// Sigmoid: 1 / (1 + exp(-x)).
    Sigmoid,
    /// No activation (identity).
    Linear,
    /// Leaky `ReLU`: max(alpha * x, x) with alpha = 0.01.
    LeakyReLU,
}

/// A single dense (fully connected) layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenseLayer {
    /// Weight matrix stored row-major: `weights[output][input]`.
    pub weights: Vec<Vec<f64>>,
    /// Bias vector, one per output neuron.
    pub biases: Vec<f64>,
    /// Activation function for this layer.
    pub activation: Activation,
}

/// A feed-forward neural network (MLP).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedForwardNet {
    layers: Vec<DenseLayer>,
}

impl FeedForwardNet {
    /// Build a network from a list of layers.
    ///
    /// # Errors
    ///
    /// Returns `NnError::EmptyModel` if layers is empty, or
    /// `NnError::DimensionMismatch` if consecutive layers have
    /// incompatible dimensions.
    pub fn new(layers: Vec<DenseLayer>) -> Result<Self, NnError> {
        if layers.is_empty() {
            return Err(NnError::EmptyModel);
        }
        for pair in layers.windows(2) {
            let out_size = pair[0].biases.len();
            let next_in =
                pair[1].weights.first().map_or(0, Vec::len);
            if out_size != next_in {
                return Err(NnError::DimensionMismatch {
                    expected: out_size,
                    actual: next_in,
                });
            }
        }
        Ok(Self { layers })
    }

    /// Run a forward pass on the given input vector.
    ///
    /// # Errors
    ///
    /// Returns `NnError::DimensionMismatch` if `input` length does
    /// not match the first layer's expected input size.
    pub fn forward(
        &self,
        input: &[f64],
    ) -> Result<Vec<f64>, NnError> {
        let first = &self.layers[0];
        let expected_in =
            first.weights.first().map_or(0, Vec::len);
        if input.len() != expected_in {
            return Err(NnError::DimensionMismatch {
                expected: expected_in,
                actual: input.len(),
            });
        }

        let mut current = input.to_vec();
        for layer in &self.layers {
            current = forward_layer(layer, &current);
        }
        Ok(current)
    }

    /// Deserialize a network from JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns `NnError::Deserialize` on parse failure, or
    /// validation errors from [`Self::new`].
    pub fn from_json(json: &[u8]) -> Result<Self, NnError> {
        let layers: Vec<DenseLayer> = serde_json::from_slice(json)?;
        Self::new(layers)
    }

    /// Return the expected input dimension.
    #[must_use]
    pub fn input_size(&self) -> usize {
        self.layers
            .first()
            .and_then(|l| l.weights.first())
            .map_or(0, Vec::len)
    }

    /// Return the output dimension.
    #[must_use]
    pub fn output_size(&self) -> usize {
        self.layers.last().map_or(0, |l| l.biases.len())
    }

    /// Return the number of layers.
    #[must_use]
    pub fn num_layers(&self) -> usize {
        self.layers.len()
    }
}

fn forward_layer(layer: &DenseLayer, input: &[f64]) -> Vec<f64> {
    let mut output = Vec::with_capacity(layer.biases.len());
    for (row, bias) in layer.weights.iter().zip(&layer.biases) {
        let sum: f64 = row
            .iter()
            .zip(input)
            .map(|(w, x)| w * x)
            .sum::<f64>()
            + bias;
        output.push(activate(sum, layer.activation));
    }
    output
}

fn activate(x: f64, activation: Activation) -> f64 {
    match activation {
        Activation::ReLU => x.max(0.0),
        Activation::Sigmoid => 1.0 / (1.0 + (-x).exp()),
        Activation::Linear => x,
        Activation::LeakyReLU => {
            if x >= 0.0 {
                x
            } else {
                0.01 * x
            }
        }
    }
}

/// Create a simple MLP with the given layer sizes and `ReLU`
/// activations (linear on the output layer). Weights are
/// initialized to small deterministic values derived from the
/// layer dimensions.
///
/// Useful for testing and as a baseline before loading trained
/// weights.
#[must_use]
pub fn build_default_mlp(layer_sizes: &[usize]) -> FeedForwardNet {
    let mut layers = Vec::with_capacity(layer_sizes.len() - 1);
    for (i, pair) in layer_sizes.windows(2).enumerate() {
        let (in_size, out_size) = (pair[0], pair[1]);
        let is_last = i == layer_sizes.len() - 2;

        let scale = (2.0 / in_size as f64).sqrt();
        let weights = (0..out_size)
            .map(|row| {
                (0..in_size)
                    .map(|col| {
                        let seed = (row * 7 + col * 13 + i * 31)
                            as f64
                            / 100.0;
                        (seed.sin() * scale).clamp(-scale, scale)
                    })
                    .collect()
            })
            .collect();

        let biases = vec![0.0; out_size];
        let activation = if is_last {
            Activation::Linear
        } else {
            Activation::ReLU
        };

        layers.push(DenseLayer {
            weights,
            biases,
            activation,
        });
    }

    FeedForwardNet { layers }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn simple_net() -> FeedForwardNet {
        let layer1 = DenseLayer {
            weights: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
            biases: vec![0.0, 0.0],
            activation: Activation::ReLU,
        };
        let layer2 = DenseLayer {
            weights: vec![vec![1.0, 1.0]],
            biases: vec![0.0],
            activation: Activation::Linear,
        };
        FeedForwardNet::new(vec![layer1, layer2])
            .expect("valid network")
    }

    #[test]
    fn forward_identity_sum() {
        let net = simple_net();
        let out =
            net.forward(&[3.0, 4.0]).expect("forward pass");
        assert_eq!(out.len(), 1);
        assert!((out[0] - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn relu_clips_negatives() {
        let net = simple_net();
        let out =
            net.forward(&[-5.0, 3.0]).expect("forward pass");
        assert!((out[0] - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dimension_mismatch_input() {
        let net = simple_net();
        let result = net.forward(&[1.0, 2.0, 3.0]);
        assert!(result.is_err());
    }

    #[test]
    fn dimension_mismatch_layers() {
        let layer1 = DenseLayer {
            weights: vec![vec![1.0]],
            biases: vec![0.0],
            activation: Activation::ReLU,
        };
        let layer2 = DenseLayer {
            weights: vec![vec![1.0, 2.0]],
            biases: vec![0.0],
            activation: Activation::Linear,
        };
        let result = FeedForwardNet::new(vec![layer1, layer2]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_model() {
        let result = FeedForwardNet::new(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn sigmoid_activation() {
        let val = activate(0.0, Activation::Sigmoid);
        assert!((val - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn leaky_relu_positive() {
        let val = activate(5.0, Activation::LeakyReLU);
        assert!((val - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn leaky_relu_negative() {
        let val = activate(-10.0, Activation::LeakyReLU);
        assert!((val - (-0.1)).abs() < f64::EPSILON);
    }

    #[test]
    fn json_roundtrip() {
        let net = simple_net();
        let json = serde_json::to_vec(&net.layers)
            .expect("serialize layers");
        let restored = FeedForwardNet::from_json(&json)
            .expect("deserialize");
        assert_eq!(restored.input_size(), 2);
        assert_eq!(restored.output_size(), 1);
        assert_eq!(restored.num_layers(), 2);
    }

    #[test]
    fn build_default_mlp_dimensions() {
        let net = build_default_mlp(&[10, 32, 16, 1]);
        assert_eq!(net.input_size(), 10);
        assert_eq!(net.output_size(), 1);
        assert_eq!(net.num_layers(), 3);
        let out =
            net.forward(&[1.0; 10]).expect("forward pass");
        assert_eq!(out.len(), 1);
        assert!(out[0].is_finite());
    }

    #[test]
    fn net_metadata() {
        let net = simple_net();
        assert_eq!(net.input_size(), 2);
        assert_eq!(net.output_size(), 1);
        assert_eq!(net.num_layers(), 2);
    }
}
