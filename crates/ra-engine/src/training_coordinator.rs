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
/// trains the BitNet cost model online.
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

        if self.total_train_steps % SNAPSHOT_INTERVAL == 0 {
            self.snapshot_model();
        }
    }

    /// Get the current trained model for use by the speculative router.
    #[must_use]
    pub fn current_model(&self) -> Arc<BitNetCostModel> {
        Arc::clone(&self.model)
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
    ///   - dim 12: difficulty score (optimal_stop / iterations_run)
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
