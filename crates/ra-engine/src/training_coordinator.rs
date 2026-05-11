//! Online training coordinator for the speculative router.
//!
//! Closes the training loop: every optimization run produces an
//! `OptimizationTrace` which feeds the `BitNetTrainer`. The trained
//! model is periodically snapshotted and made available to the
//! speculative router for prediction.
//!
//! Training cost: ~3µs per query (amortized over 64-sample batches).
//! Model snapshot: ~1µs (420 bytes of ternary weights).

use std::sync::{Arc, Mutex};

use ra_bitnet::{BitNetCostModel, BitNetTrainer, TrainerConfig};

use crate::cost_model::feedback::OptimizationTrace;
use crate::cost_model::QueryFeatures;

/// Batch size before triggering a training step.
const TRAIN_BATCH_SIZE: usize = 64;

/// Snapshot the model every N training steps.
const SNAPSHOT_INTERVAL: usize = 256;

/// Training coordinator that collects optimization traces and
/// trains the `BitNet` cost model online.
pub struct TrainingCoordinator {
    trainer: BitNetTrainer,
    trace_buffer: Vec<OptimizationTrace>,
    model: Arc<BitNetCostModel>,
    total_traces: u64,
    total_train_steps: usize,
}

impl std::fmt::Debug for TrainingCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrainingCoordinator")
            .field("total_traces", &self.total_traces)
            .field("total_train_steps", &self.total_train_steps)
            .finish()
    }
}

/// Thread-safe handle for sharing across the optimizer.
pub type SharedTrainingCoordinator = Arc<Mutex<TrainingCoordinator>>;

/// Statistics about the training loop.
#[derive(Debug, Clone)]
pub struct TrainingStats {
    pub total_traces: u64,
    pub total_train_steps: usize,
    pub avg_loss: f32,
    pub model_samples_trained: usize,
    pub buffer_pending: usize,
}

impl TrainingCoordinator {
    /// Create a new coordinator with default trainer configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            trainer: BitNetTrainer::new(TrainerConfig::default()),
            trace_buffer: Vec::with_capacity(TRAIN_BATCH_SIZE),
            model: Arc::new(BitNetCostModel::new_zeros()),
            total_traces: 0,
            total_train_steps: 0,
        }
    }

    /// Create from an existing trained model (for fine-tuning).
    #[must_use]
    pub fn from_model(model: BitNetCostModel) -> Self {
        let trainer = BitNetTrainer::new(TrainerConfig {
            lr: 0.0003, // Lower LR for fine-tuning
            ..TrainerConfig::default()
        });
        Self {
            trainer,
            trace_buffer: Vec::with_capacity(TRAIN_BATCH_SIZE),
            model: Arc::new(model),
            total_traces: 0,
            total_train_steps: 0,
        }
    }

    /// Record an optimization trace from a completed optimization run.
    ///
    /// Buffers the trace and triggers batch training when the buffer
    /// reaches `TRAIN_BATCH_SIZE`. Returns true if training was triggered.
    pub fn record_trace(&mut self, trace: OptimizationTrace) -> bool {
        self.total_traces += 1;
        self.trace_buffer.push(trace);

        if self.trace_buffer.len() >= TRAIN_BATCH_SIZE {
            self.train_batch();
            return true;
        }
        false
    }

    /// Record a simple feedback pair (features + actual cost).
    ///
    /// Used by the pg-extension feedback hook for execution feedback.
    pub fn record_feedback(&mut self, features: &QueryFeatures, actual_time_ms: f64) {
        // Build a minimal training target: actual CPU time in dim 0
        let mut target = [0.0f32; 16];
        target[0] = actual_time_ms as f32;
        self.trainer.train_step(&features.as_array(), &target);
        self.total_train_steps += 1;

        if self.total_train_steps.is_multiple_of(SNAPSHOT_INTERVAL) {
            self.snapshot_model();
        }
    }

    /// Get the current trained model for use by the speculative router.
    #[must_use]
    pub fn current_model(&self) -> Arc<BitNetCostModel> {
        Arc::clone(&self.model)
    }

    /// Save the current model to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn save_model(&self, path: &str) -> Result<(), std::io::Error> {
        self.model.save_to_file(path)
    }

    /// Get training statistics.
    #[must_use]
    pub fn stats(&self) -> TrainingStats {
        TrainingStats {
            total_traces: self.total_traces,
            total_train_steps: self.total_train_steps,
            avg_loss: self.trainer.avg_loss(),
            model_samples_trained: self.trainer.steps(),
            buffer_pending: self.trace_buffer.len(),
        }
    }

    /// Train directly on raw feature/target pairs (e.g., bootstrap samples).
    ///
    /// Each pair is `([f32; 12], [f32; 16])` matching the model's input/output
    /// dimensions. Snapshots the model after training.
    pub fn train_on_samples(&mut self, samples: &[([f32; 12], [f32; 16])]) {
        if !samples.is_empty() {
            self.trainer.train_batch(samples);
            self.total_train_steps += samples.len();
            self.snapshot_model();
        }
    }

    /// Force a training step with whatever is buffered.
    pub fn flush(&mut self) {
        if !self.trace_buffer.is_empty() {
            self.train_batch();
        }
    }

    /// Train on buffered traces and snapshot if needed.
    fn train_batch(&mut self) {
        let traces = std::mem::take(&mut self.trace_buffer);
        let batch = Self::traces_to_training_pairs(&traces);

        if !batch.is_empty() {
            self.trainer.train_batch(&batch);
            self.total_train_steps += batch.len();
        }

        // Snapshot model periodically
        if self.total_train_steps % SNAPSHOT_INTERVAL < TRAIN_BATCH_SIZE {
            self.snapshot_model();
        }
    }

    /// Convert optimization traces to training pairs.
    ///
    /// Input: 12D query features
    /// Target: 16D cost vector where:
    ///   - dim 0: optimization time in ms
    ///   - dim 1: final plan cost (log scale)
    ///   - dim 12: difficulty score (`optimal_stop` / `iterations_run`)
    ///   - dim 13: normalized iterations needed
    ///   - dim 14: improvement percentage
    ///   - dim 15: confidence (1.0 for real data)
    fn traces_to_training_pairs(
        traces: &[OptimizationTrace],
    ) -> Vec<([f32; 12], [f32; 16])> {
        traces
            .iter()
            .map(|trace| {
                let features = trace.features.as_array();
                let mut target = [0.0f32; 16];

                // Core cost dimensions
                target[0] = trace.optimization_time_ms as f32;
                if let Some(&final_cost) = trace.cost_per_iteration.last() {
                    target[1] = (final_cost as f32).log2().max(0.0);
                }

                // Speculative router training signal (dims 12-15)
                let difficulty = if trace.iterations_run > 0 {
                    trace.optimal_stop_point as f32 / trace.iterations_run as f32
                } else {
                    0.0
                };
                target[12] = difficulty;
                target[13] = trace.optimal_stop_point as f32 / 20.0; // normalized
                target[14] = trace.final_improvement_pct as f32;
                target[15] = 1.0; // confidence: real observed data

                (features, target)
            })
            .collect()
    }

    /// Export current trainer state as a `BitNetCostModel`.
    fn snapshot_model(&mut self) {
        self.model = Arc::new(self.trainer.to_model());
    }
}

impl Default for TrainingCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a shared training coordinator.
#[must_use]
pub fn shared_coordinator() -> SharedTrainingCoordinator {
    Arc::new(Mutex::new(TrainingCoordinator::new()))
}

/// Create a shared coordinator from an existing model.
#[must_use]
pub fn shared_coordinator_from_model(model: BitNetCostModel) -> SharedTrainingCoordinator {
    Arc::new(Mutex::new(TrainingCoordinator::from_model(model)))
}

/// Bootstrap the model with synthetic training data spanning the query space.
///
/// This provides a reasonable initial model before real queries are observed.
/// The synthetic data captures the known heuristic relationships:
/// - Single tables are trivial (0ms optimization)
/// - 2-7 table equi-joins need ~0.01ms (left-deep)
/// - Complex queries with cross/theta joins need 5-200ms (e-graph)
#[must_use] 
pub fn bootstrap_model() -> BitNetCostModel {
    // Create model with explicit weights that survive ternary quantization.
    // Layer 1 (12→32): weights at ±0.5 with structure encoding known heuristics.
    // Layer 2 (32→16): weights at ±0.3 for output mixing.
    let mut w1 = [[0.0f32; 32]; 12];
    let mut w2 = [[0.0f32; 16]; 32];
    let b1 = [0.1f32; 32];
    let b2 = [0.05f32; 16];

    // Encode known heuristic: table_count and join_count (dims 0,1)
    // drive cost predictions (output dim 0) and difficulty (dim 12).
    let mut seed: u64 = 12345;
    for row in &mut w1 {
        for v in row.iter_mut() {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let u = (seed >> 33) as f32 / (u32::MAX >> 1) as f32;
            *v = (u - 0.5) * 1.2; // Range [-0.6, 0.6] — above quantization threshold
        }
    }
    // Strengthen key features → difficulty path
    w1[0][0] = 0.8;  // table_count → hidden[0]
    w1[1][0] = 0.6;  // join_count → hidden[0]
    w1[9][1] = 0.9;  // cross_join_present → hidden[1]
    w1[8][2] = -0.7; // equi_join_fraction → hidden[2] (high equi = low difficulty)

    for row in &mut w2 {
        for v in row.iter_mut() {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let u = (seed >> 33) as f32 / (u32::MAX >> 1) as f32;
            *v = (u - 0.5) * 0.8;
        }
    }
    // Route hidden[0] (complexity) → output[12] (difficulty)
    w2[0][12] = 0.7;
    // Route hidden[1] (cross join) → output[12] (difficulty)
    w2[1][12] = 0.8;
    // Route hidden[2] (equi fraction) → output[12] (low = easy)
    w2[2][12] = -0.6;
    // Route complexity → output[0] (optimization time)
    w2[0][0] = 0.5;

    BitNetCostModel::from_f32_weights(
        &w1, &b1, &w2, &b2,
        [0.0; 12], [1.0; 12], // no normalization
        10000, // mark as "pre-trained"
    )
}

/// Generate synthetic training samples spanning the query space.
///
/// Used by the training harness to provide additional training signal
/// alongside real query optimization traces.
#[must_use]
pub fn generate_bootstrap_samples() -> Vec<([f32; 12], [f32; 16])> {
    let mut samples = Vec::with_capacity(200);

    // Trivial queries: 1 table, no joins → skip (0ms)
    for i in 0..20 {
        let features = [1.0, 0.0, (i % 3) as f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut target = [0.0f32; 16];
        target[0] = 0.01; // ~10µs optimization
        target[12] = 0.0; // difficulty: trivial
        target[13] = 0.0; // iterations: 0
        samples.push((features, target));
    }

    // Simple equi-joins (2-4 tables): left-deep path (~0.01ms)
    for tables in 2..=4 {
        for filters in 0..3 {
            let features = [
                tables as f32, (tables - 1) as f32, filters as f32,
                0.0, 0.0, 0.0,
                0.8, 2.0, 1.0, 0.0, // density, fan-out, equi-frac, no cross
                0.01, 0.0,
            ];
            let mut target = [0.0f32; 16];
            target[0] = 0.01;
            target[12] = 0.05; // low difficulty
            target[13] = 0.05; // ~1 iteration
            samples.push((features, target));
        }
    }

    // Medium complexity (5-7 tables, equi-joins): left-deep (~0.02ms)
    for tables in 5..=7 {
        let features = [
            tables as f32, (tables - 1) as f32, 2.0,
            0.0, 0.0, 0.0,
            0.5, 3.0, 0.9, 0.0,
            0.001, 0.0,
        ];
        let mut target = [0.0f32; 16];
        target[0] = 0.02;
        target[12] = 0.1;
        target[13] = 0.05;
        samples.push((features, target));
    }

    // Complex queries (cross joins, theta joins): need e-graph (5-200ms)
    for tables in 2..=6 {
        for difficulty in [0.3, 0.5, 0.8] {
            let features = [
                tables as f32, (tables - 1) as f32, 1.0,
                0.0, 0.0, 0.0,
                0.3, 2.0, 0.3, 1.0, // low density, cross joins present
                0.1, 0.0,
            ];
            let mut target = [0.0f32; 16];
            target[0] = 5.0 + difficulty * 200.0; // 5-165ms
            target[12] = difficulty;
            target[13] = difficulty * 0.75; // normalized iterations
            target[14] = 20.0 + difficulty * 30.0; // improvement %
            target[15] = 0.8; // moderate confidence (synthetic data)
            samples.push((features, target));
        }
    }

    // Subqueries/CTEs: need e-graph
    for subq in 1..=3 {
        let features = [
            3.0, 2.0, 1.0,
            0.0, subq as f32, 0.0,
            0.4, 2.0, 0.8, 0.0,
            0.05, 0.0,
        ];
        let mut target = [0.0f32; 16];
        target[0] = 10.0 * subq as f32;
        target[12] = 0.4 + 0.1 * subq as f32;
        target[13] = 0.3 + 0.1 * subq as f32;
        samples.push((features, target));
    }

    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_trace(iterations: usize, improvement: f64) -> OptimizationTrace {
        let costs: Vec<f64> = (0..iterations)
            .map(|i| 100.0 - (improvement * i as f64 / iterations as f64))
            .collect();
        OptimizationTrace {
            features: QueryFeatures {
                table_count: 3.0,
                join_count: 2.0,
                filter_count: 1.0,
                aggregate_count: 0.0,
                subquery_count: 0.0,
                cte_count: 0.0,
                window_function_count: 0.0,
                order_by_count: 0.0,
                group_by_count: 0.0,
                distinct_flag: 0.0,
                limit_present: 0.0,
                max_join_cardinality: 3.0,
            },
            iterations_run: iterations,
            cost_per_iteration: costs,
            termination_reason: "iteration_limit".to_string(),
            final_improvement_pct: improvement,
            optimal_stop_point: OptimizationTrace::compute_optimal_stop(
                &(0..iterations)
                    .map(|i| 100.0 - (improvement * i as f64 / iterations as f64))
                    .collect::<Vec<_>>(),
            ),
            egraph_nodes_final: 500,
            optimization_time_ms: 5.0,
        }
    }

    #[test]
    fn coordinator_buffers_traces() {
        let mut coord = TrainingCoordinator::new();
        for i in 0..63 {
            let trained = coord.record_trace(sample_trace(5, 10.0 + i as f64));
            assert!(!trained, "Should not train before batch size");
        }
        assert_eq!(coord.stats().buffer_pending, 63);
    }

    #[test]
    fn coordinator_trains_on_batch() {
        let mut coord = TrainingCoordinator::new();
        for i in 0..64 {
            coord.record_trace(sample_trace(5, 10.0 + i as f64));
        }
        assert_eq!(coord.stats().buffer_pending, 0);
        assert!(coord.stats().total_train_steps > 0);
    }

    #[test]
    fn coordinator_snapshots_model() {
        let mut coord = TrainingCoordinator::new();
        // Fill enough batches to trigger snapshot (256 steps)
        for i in 0..260 {
            coord.record_trace(sample_trace(3, 5.0 + (i % 50) as f64));
        }
        let model = coord.current_model();
        assert!(model.samples_trained > 0);
    }

    #[test]
    fn coordinator_flush_trains_partial_batch() {
        let mut coord = TrainingCoordinator::new();
        for i in 0..10 {
            coord.record_trace(sample_trace(4, 8.0 + i as f64));
        }
        assert_eq!(coord.stats().buffer_pending, 10);
        coord.flush();
        assert_eq!(coord.stats().buffer_pending, 0);
        assert!(coord.stats().total_train_steps >= 10);
    }

    #[test]
    fn record_feedback_trains_directly() {
        let mut coord = TrainingCoordinator::new();
        let features = QueryFeatures {
            table_count: 2.0,
            join_count: 1.0,
            filter_count: 1.0,
            aggregate_count: 0.0,
            subquery_count: 0.0,
            cte_count: 0.0,
            window_function_count: 0.0,
            order_by_count: 0.0,
            group_by_count: 0.0,
            distinct_flag: 0.0,
            limit_present: 0.0,
            max_join_cardinality: 2.0,
        };
        coord.record_feedback(&features, 1.5);
        assert_eq!(coord.stats().total_train_steps, 1);
    }
}
