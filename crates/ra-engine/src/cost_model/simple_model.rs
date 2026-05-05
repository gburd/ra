//! Simple linear cost model for query optimization.
//!
//! This is a lightweight implementation that uses basic linear algebra
//! instead of a full transformer. It can be trained online and used for
//! cost estimation and rule ranking.
//!
//! Architecture:
//! - Input: Query features (table count, join count, filter count, etc.)
//! - Hidden layer: 32 neurons with ReLU activation
//! - Output: 16 cost dimensions
//!
//! This model is designed to be:
//! - Fast (<0.1ms inference)
//! - Small (~10KB)
//! - Trainable online
//! - Measurable

/// Simple neural network for cost prediction.
#[derive(Debug, Clone)]
pub struct SimpleCostModel {
    /// Input layer weights (feature_dim × hidden_dim)
    w1: Vec<Vec<f32>>,
    /// Input layer bias
    b1: Vec<f32>,
    /// Output layer weights (hidden_dim × output_dim)
    w2: Vec<Vec<f32>>,
    /// Output layer bias
    b2: Vec<f32>,
    /// Number of training samples seen
    samples_seen: usize,
    /// Running average error per dimension
    avg_errors: Vec<f32>,
}

/// Query features extracted for cost prediction.
#[derive(Debug, Clone)]
pub struct QueryFeatures {
    pub table_count: f32,
    pub join_count: f32,
    pub filter_count: f32,
    pub aggregate_count: f32,
    pub subquery_count: f32,
    pub cte_count: f32,
    pub window_function_count: f32,
    pub order_by_count: f32,
    pub group_by_count: f32,
    pub distinct_flag: f32,
    pub limit_present: f32,
    pub max_join_cardinality: f32,
}

impl QueryFeatures {
    /// Convert features to vector for neural network input.
    pub fn to_vec(&self) -> Vec<f32> {
        vec![
            self.table_count,
            self.join_count,
            self.filter_count,
            self.aggregate_count,
            self.subquery_count,
            self.cte_count,
            self.window_function_count,
            self.order_by_count,
            self.group_by_count,
            self.distinct_flag,
            self.limit_present,
            self.max_join_cardinality,
        ]
    }

    /// Number of features.
    pub const FEATURE_DIM: usize = 12;
}

/// Softplus activation: ln(1 + exp(x))
/// Ensures positive outputs with smooth gradients.
fn softplus(x: f32) -> f32 {
    if x > 20.0 {
        x // Avoid overflow for large x
    } else {
        (1.0 + x.exp()).ln()
    }
}

/// Derivative of softplus: exp(x) / (1 + exp(x)) = sigmoid(x)
fn softplus_derivative(x: f32) -> f32 {
    if x > 20.0 {
        1.0 // Sigmoid saturates to 1.0 for large x
    } else {
        let exp_x = x.exp();
        exp_x / (1.0 + exp_x)
    }
}

impl SimpleCostModel {
    const HIDDEN_DIM: usize = 32;
    const OUTPUT_DIM: usize = 16;
    const LEARNING_RATE: f32 = 0.01;

    /// Create a new model with random weights.
    pub fn new() -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        // Xavier initialization for weights
        let w1_scale = (2.0 / QueryFeatures::FEATURE_DIM as f32).sqrt();
        let w2_scale = (2.0 / Self::HIDDEN_DIM as f32).sqrt();

        let w1 = (0..QueryFeatures::FEATURE_DIM)
            .map(|_| {
                (0..Self::HIDDEN_DIM)
                    .map(|_| rng.gen::<f32>() * w1_scale - w1_scale / 2.0)
                    .collect()
            })
            .collect();

        let b1 = vec![0.0; Self::HIDDEN_DIM];

        let w2 = (0..Self::HIDDEN_DIM)
            .map(|_| {
                (0..Self::OUTPUT_DIM)
                    .map(|_| rng.gen::<f32>() * w2_scale - w2_scale / 2.0)
                    .collect()
            })
            .collect();

        let b2 = vec![0.0; Self::OUTPUT_DIM];

        Self {
            w1,
            b1,
            w2,
            b2,
            samples_seen: 0,
            avg_errors: vec![0.0; Self::OUTPUT_DIM],
        }
    }

    /// Forward pass: predict costs from features.
    pub fn predict(&self, features: &QueryFeatures) -> super::CostVector {
        let x = features.to_vec();

        // Hidden layer: h = ReLU(W1 * x + b1)
        let mut hidden = vec![0.0; Self::HIDDEN_DIM];
        for i in 0..Self::HIDDEN_DIM {
            let mut sum = self.b1[i];
            for j in 0..QueryFeatures::FEATURE_DIM {
                sum += self.w1[j][i] * x[j];
            }
            hidden[i] = sum.max(0.0); // ReLU
        }

        // Output layer: y = softplus(W2 * h + b2)
        // Softplus(x) = ln(1 + exp(x)) ensures positive outputs with smooth gradients
        let mut output = vec![0.0; Self::OUTPUT_DIM];
        for i in 0..Self::OUTPUT_DIM {
            let mut sum = self.b2[i];
            for j in 0..Self::HIDDEN_DIM {
                sum += self.w2[j][i] * hidden[j];
            }
            // Softplus activation for non-negative costs with gradient flow
            output[i] = softplus(sum);
        }

        // Convert to CostVector
        super::CostVector {
            cpu_time_ms: output[0],
            memory_peak_mb: output[1],
            memory_avg_mb: output[2],
            io_storage_ops: output[3] as u64,
            io_storage_bytes: output[4] as u64,
            io_network_ops: output[5] as u64,
            io_network_bytes: output[6] as u64,
            locks_acquired: output[7] as u32,
            lock_hold_time_ms: output[8],
            lock_contention_score: output[9],
            vacuum_overhead: output[10],
            wal_generation_bytes: output[11] as u64,
            replication_lag_ms: output[12],
            cache_hit_ratio: output[13].min(1.0).max(0.0),
            page_faults: output[14] as u32,
            context_switches: output[15] as u32,
        }
    }

    /// Train on a single example using gradient descent.
    pub fn train(&mut self, features: &QueryFeatures, actual: &super::ActualCost) {
        // Forward pass (save intermediates for backprop)
        let x = features.to_vec();

        // Hidden layer
        let mut hidden_pre = vec![0.0; Self::HIDDEN_DIM];
        let mut hidden = vec![0.0; Self::HIDDEN_DIM];
        for i in 0..Self::HIDDEN_DIM {
            let mut sum = self.b1[i];
            for j in 0..QueryFeatures::FEATURE_DIM {
                sum += self.w1[j][i] * x[j];
            }
            hidden_pre[i] = sum;
            hidden[i] = sum.max(0.0); // ReLU
        }

        // Output layer
        let mut output_pre = vec![0.0; Self::OUTPUT_DIM];
        let mut output = vec![0.0; Self::OUTPUT_DIM];
        for i in 0..Self::OUTPUT_DIM {
            let mut sum = self.b2[i];
            for j in 0..Self::HIDDEN_DIM {
                sum += self.w2[j][i] * hidden[j];
            }
            output_pre[i] = sum;
            output[i] = softplus(sum);
        }

        // Convert actual to vector
        let actual_vec = vec![
            actual.cpu_time_ms,
            actual.memory_peak_mb,
            actual.memory_avg_mb,
            actual.io_storage_ops as f32,
            actual.io_storage_bytes as f32,
            actual.io_network_ops as f32,
            actual.io_network_bytes as f32,
            actual.locks_acquired as f32,
            actual.lock_hold_time_ms,
            actual.lock_contention_score,
            actual.vacuum_overhead,
            actual.wal_generation_bytes as f32,
            actual.replication_lag_ms,
            actual.cache_hit_ratio,
            actual.page_faults as f32,
            actual.context_switches as f32,
        ];

        // Compute gradients (MSE loss)
        let mut output_grad = vec![0.0; Self::OUTPUT_DIM];
        for i in 0..Self::OUTPUT_DIM {
            // Chain rule: d(Loss)/d(output_pre) = d(Loss)/d(output) * d(output)/d(output_pre)
            let loss_grad = 2.0 * (output[i] - actual_vec[i]);
            output_grad[i] = loss_grad * softplus_derivative(output_pre[i]);

            // Update running average error
            let error = (output[i] - actual_vec[i]).abs();
            self.avg_errors[i] = if self.samples_seen == 0 {
                error
            } else {
                0.99 * self.avg_errors[i] + 0.01 * error
            };
        }

        // Backprop to hidden layer
        let mut hidden_grad = vec![0.0; Self::HIDDEN_DIM];
        for i in 0..Self::HIDDEN_DIM {
            for j in 0..Self::OUTPUT_DIM {
                hidden_grad[i] += output_grad[j] * self.w2[i][j];
            }
            // ReLU derivative
            if hidden_pre[i] <= 0.0 {
                hidden_grad[i] = 0.0;
            }
        }

        // Update weights and biases
        for i in 0..Self::HIDDEN_DIM {
            for j in 0..Self::OUTPUT_DIM {
                self.w2[i][j] -= Self::LEARNING_RATE * output_grad[j] * hidden[i];
            }
        }

        for i in 0..Self::OUTPUT_DIM {
            self.b2[i] -= Self::LEARNING_RATE * output_grad[i];
        }

        for i in 0..QueryFeatures::FEATURE_DIM {
            for j in 0..Self::HIDDEN_DIM {
                self.w1[i][j] -= Self::LEARNING_RATE * hidden_grad[j] * x[i];
            }
        }

        for i in 0..Self::HIDDEN_DIM {
            self.b1[i] -= Self::LEARNING_RATE * hidden_grad[i];
        }

        self.samples_seen += 1;
    }

    /// Get model statistics.
    pub fn stats(&self) -> ModelStats {
        ModelStats {
            samples_seen: self.samples_seen,
            avg_errors: self.avg_errors.clone(),
            model_size_bytes: self.size_bytes(),
        }
    }

    /// Calculate model size in bytes.
    pub fn size_bytes(&self) -> usize {
        let w1_size = QueryFeatures::FEATURE_DIM * Self::HIDDEN_DIM * std::mem::size_of::<f32>();
        let b1_size = Self::HIDDEN_DIM * std::mem::size_of::<f32>();
        let w2_size = Self::HIDDEN_DIM * Self::OUTPUT_DIM * std::mem::size_of::<f32>();
        let b2_size = Self::OUTPUT_DIM * std::mem::size_of::<f32>();
        w1_size + b1_size + w2_size + b2_size
    }
}

impl Default for SimpleCostModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Model statistics.
#[derive(Debug, Clone)]
pub struct ModelStats {
    pub samples_seen: usize,
    pub avg_errors: Vec<f32>,
    pub model_size_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_creation() {
        let model = SimpleCostModel::new();
        assert_eq!(model.samples_seen, 0);
        assert_eq!(model.avg_errors.len(), SimpleCostModel::OUTPUT_DIM);
    }

    #[test]
    fn test_prediction() {
        let model = SimpleCostModel::new();
        let features = QueryFeatures {
            table_count: 2.0,
            join_count: 1.0,
            filter_count: 2.0,
            aggregate_count: 0.0,
            subquery_count: 0.0,
            cte_count: 0.0,
            window_function_count: 0.0,
            order_by_count: 0.0,
            group_by_count: 0.0,
            distinct_flag: 0.0,
            limit_present: 0.0,
            max_join_cardinality: 1000.0,
        };

        let prediction = model.predict(&features);
        // Should produce non-negative predictions
        assert!(prediction.cpu_time_ms >= 0.0);
        assert!(prediction.memory_peak_mb >= 0.0);
    }

    #[test]
    fn test_model_size() {
        let model = SimpleCostModel::new();
        let stats = model.stats();
        // Model should be small (<100KB)
        assert!(stats.model_size_bytes < 100_000);
        // With 12 features, 32 hidden, 16 output:
        // (12*32 + 32) + (32*16 + 16) = 384 + 32 + 512 + 16 = 944 floats
        // 944 * 4 bytes = 3776 bytes
        assert_eq!(stats.model_size_bytes, 3776);
    }
}
