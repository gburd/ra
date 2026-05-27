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
        // Surface only the high-signal fields. The trainer's internal
        // weights and the trace buffer are intentionally elided to keep
        // log lines readable and avoid leaking floating-point arrays.
        f.debug_struct("TrainingCoordinator")
            .field("total_traces", &self.total_traces)
            .field("total_train_steps", &self.total_train_steps)
            .field("buffer_pending", &self.trace_buffer.len())
            .finish_non_exhaustive()
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
    /// Only dim 0 (CPU time) is observed, so we use masked training to
    /// avoid forcing the other 15 dimensions toward zero. Pre-A2, this
    /// function set the entire 16-dim target to zero except dim 0, which
    /// systematically destroyed the routing dims (12-15) every step.
    pub fn record_feedback(&mut self, features: &QueryFeatures, actual_time_ms: f64) {
        self.record_feedback_partial(features, &[(0, actual_time_ms as f32)]);
    }

    /// Record partial feedback supplying values for specific output dims.
    ///
    /// `observed` is `(dim_index, value)` pairs. Unspecified dimensions
    /// receive no gradient and are left untouched. Use this when more
    /// than CPU time is known (e.g. memory dim 1, I/O dim 3).
    ///
    /// When dim 0 (CPU time) is observed, the scalar head is also
    /// updated via SGD against the same target, so subsequent calls to
    /// [`crate::cost_model::BitNetCostModel::predict_scalar`] reflect
    /// the observed timing rather than the hand-tuned default formula.
    pub fn record_feedback_partial(
        &mut self,
        features: &QueryFeatures,
        observed: &[(usize, f32)],
    ) {
        let mut target = [0.0f32; 16];
        let mut mask = [false; 16];
        let mut cpu_target: Option<f32> = None;
        for &(dim, value) in observed {
            if dim < 16 {
                target[dim] = value;
                mask[dim] = true;
                if dim == 0 {
                    cpu_target = Some(value);
                }
            }
        }
        if !mask.iter().any(|m| *m) {
            return;
        }
        let feature_array = features.as_array();
        self.trainer
            .train_step_masked(&feature_array, &target, &mask);
        if let Some(cpu_ms) = cpu_target {
            // Conservative LR: 0.01 means it'd take ~100 same-magnitude
            // observations to noticeably move the head from defaults.
            // The hidden layers are also being trained on the same
            // sample so we don't want the scalar head to overfit
            // before they catch up.
            self.trainer
                .update_scalar_head(&feature_array, cpu_ms, 0.01);
        }
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
    /// Each pair is `([f32; 16], [f32; 16])` matching the model's input/output
    /// dimensions. Snapshots the model after training. Treats every target
    /// dimension as observed; for partial supervision use
    /// [`Self::train_on_samples_masked`].
    pub fn train_on_samples(&mut self, samples: &[([f32; 16], [f32; 16])]) {
        if !samples.is_empty() {
            self.trainer.train_batch(samples);
            self.total_train_steps += samples.len();
            self.snapshot_model();
        }
    }

    /// Train on `(features, target, mask)` triples — only mask-true dims
    /// receive gradient. Snapshots the model after training.
    pub fn train_on_samples_masked(
        &mut self,
        samples: &[([f32; 16], [f32; 16], [bool; 16])],
    ) {
        if !samples.is_empty() {
            self.trainer.train_batch_masked(samples);
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
            // Use masked batch training: each trace observes only specific
            // dims (see `traces_to_training_pairs`). Pre-A2 we used
            // `train_batch` here, which forced unobserved dims to zero.
            self.trainer.train_batch_masked(&batch);
            self.total_train_steps += batch.len();
        }

        // Snapshot model periodically
        if self.total_train_steps % SNAPSHOT_INTERVAL < TRAIN_BATCH_SIZE {
            self.snapshot_model();
        }
    }

    /// Convert optimization traces to masked training samples.
    ///
    /// Input: 12D query features.
    /// Output: `(features, target, mask)` triples where each sample's
    /// `mask[j] == true` for dims actually carried by the trace.
    /// Observed dims are:
    ///   - dim 0: optimization time in ms
    ///   - dim 1: final plan cost (log scale, when known)
    ///   - dim 12: difficulty score (`optimal_stop` / `iterations_run`)
    ///   - dim 13: normalized iterations needed
    ///   - dim 14: improvement percentage
    ///   - dim 15: confidence (1.0 for real data)
    ///
    /// Other dims (memory, I/O, locks, etc.) are unobserved and stay
    /// untouched by training to avoid the pre-A2 zero-collapse bug.
    fn traces_to_training_pairs(
        traces: &[OptimizationTrace],
    ) -> Vec<([f32; 16], [f32; 16], [bool; 16])> {
        traces
            .iter()
            .map(|trace| {
                let features = trace.features.as_array();
                let mut target = [0.0f32; 16];
                let mut mask = [false; 16];

                // dim 0: optimization wall-clock time (always observed).
                target[0] = trace.optimization_time_ms as f32;
                mask[0] = true;

                // dim 1: log-scale final plan cost (only if non-empty trace).
                if let Some(&final_cost) = trace.cost_per_iteration.last() {
                    target[1] = (final_cost as f32).log2().max(0.0);
                    mask[1] = true;
                }

                // Speculative router training signal (dims 12-15).
                let difficulty = if trace.iterations_run > 0 {
                    trace.optimal_stop_point as f32 / trace.iterations_run as f32
                } else {
                    0.0
                };
                target[12] = difficulty;
                target[13] = trace.optimal_stop_point as f32 / 20.0;
                target[14] = trace.final_improvement_pct as f32;
                target[15] = 1.0;
                mask[12] = true;
                mask[13] = true;
                mask[14] = true;
                mask[15] = true;

                (features, target, mask)
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
/// Trains a freshly-initialized [`BitNetTrainer`] over several epochs on
/// the [`generate_bootstrap_samples`] dataset, then exports the trained
/// weights as a [`BitNetCostModel`]. The result is a genuinely
/// "pre-trained" snapshot — `samples_trained` reflects the actual number
/// of training steps performed, not a marketing constant.
///
/// The synthetic data captures known heuristic relationships:
/// - Single tables are trivial (~10µs optimization, dim 0)
/// - 2–7 table equi-joins need ~0.01ms (left-deep)
/// - Complex queries with cross/theta joins need 5–200ms (e-graph)
/// - Difficulty (dim 12) and predicted iterations (dim 13) for the
///   speculative router
///
/// Pre-G9 this function bypassed the trainer entirely: it pseudo-randomly
/// initialised the latent weights, hand-tuned eight specific entries,
/// then declared `samples_trained = 10000` as a literal — the model had
/// never seen any of the bootstrap targets. Now we run real training
/// and report the real step count.
///
/// Trains via the masked path so dims 1, 11 (memory, vacuum) and similar
/// unspecified outputs aren't pushed toward zero.
#[must_use]
pub fn bootstrap_model() -> BitNetCostModel {
    // 30 epochs is enough for the bias-driven targets to converge
    // through ternary quantization without overfitting the noise from
    // randomly-initialised hidden-layer weights. Pre-A4 this was 10
    // epochs at F=12; the F=16 model has ~33% more first-layer weights
    // and needs more passes to settle.
    const EPOCHS: usize = 30;

    let samples = generate_bootstrap_samples();
    let masks = bootstrap_masks(&samples);

    // Higher learning rate than the online default since we're training
    // from scratch on a fixed synthetic distribution. weight_decay=0
    // keeps the bias-driven baseline targets reachable through ternary
    // quantization noise.
    let mut trainer = BitNetTrainer::new(TrainerConfig {
        lr: 0.02,
        weight_decay: 0.0,
        ..TrainerConfig::default()
    });

    // Fit feature normalization from the bootstrap distribution so
    // cardinality and density signals don't dominate raw magnitudes.
    let feature_array: Vec<[f32; 16]> = samples.iter().map(|(f, _)| *f).collect();
    let mut tmp_model = BitNetCostModel::new_zeros();
    tmp_model.fit_normalization(&feature_array);
    // BitNetTrainer holds its own copy; sync them.
    trainer.set_normalization(tmp_model.feature_mean(), tmp_model.feature_inv_std());

    let batch: Vec<([f32; 16], [f32; 16], [bool; 16])> = samples
        .iter()
        .zip(masks.iter())
        .map(|((f, t), m)| (*f, *t, *m))
        .collect();
    for _ in 0..EPOCHS {
        trainer.train_batch_masked(&batch);
    }

    trainer.to_model()
}

/// Per-sample observation masks for [`generate_bootstrap_samples`].
///
/// The synthetic targets only fill specific dimensions (0, 12, 13, 14, 15
/// in various combinations); marking unspecified dims as unobserved
/// avoids pushing those outputs toward zero during bootstrap training.
fn bootstrap_masks(samples: &[([f32; 16], [f32; 16])]) -> Vec<[bool; 16]> {
    samples
        .iter()
        .map(|(_features, target)| {
            let mut mask = [false; 16];
            // Always observe dim 0 (optimization time) — every sample sets it.
            mask[0] = true;
            // Mark a dim observed if the synthetic generator wrote a non-zero
            // value to it. The targets in `generate_bootstrap_samples` use
            // 0.0 as a sentinel for "unspecified", so this round-trips
            // cleanly even when zero is a legitimate prediction (we still
            // train on dim 0 unconditionally).
            for (i, &v) in target.iter().enumerate().skip(1) {
                if v.abs() > f32::EPSILON {
                    mask[i] = true;
                }
            }
            mask
        })
        .collect()
}

/// Generate synthetic training samples spanning the query space.
///
/// Used by the training harness to provide additional training signal
/// alongside real query optimization traces.
#[must_use]
pub fn generate_bootstrap_samples() -> Vec<([f32; 16], [f32; 16])> {
    let mut samples = Vec::with_capacity(200);

    // Trivial queries: 1 table, no joins → skip (0ms)
    for i in 0..20 {
        let features = [1.0, 0.0, (i % 3) as f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
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
                0.0, 0.0, 0.0, 0.0,
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
            0.0, 0.0, 0.0, 0.0,
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
                0.0, 0.0, 0.0, 0.0,
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
            0.0, 0.0, 0.0, 0.0,
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
            let trained = coord.record_trace(sample_trace(5, 10.0 + f64::from(i)));
            assert!(!trained, "Should not train before batch size");
        }
        assert_eq!(coord.stats().buffer_pending, 63);
    }

    #[test]
    fn coordinator_trains_on_batch() {
        let mut coord = TrainingCoordinator::new();
        for i in 0..64 {
            coord.record_trace(sample_trace(5, 10.0 + f64::from(i)));
        }
        assert_eq!(coord.stats().buffer_pending, 0);
        assert!(coord.stats().total_train_steps > 0);
    }

    #[test]
    fn coordinator_snapshots_model() {
        let mut coord = TrainingCoordinator::new();
        // Fill enough batches to trigger snapshot (256 steps)
        for i in 0..260 {
            coord.record_trace(sample_trace(3, 5.0 + f64::from(i % 50)));
        }
        let model = coord.current_model();
        assert!(model.samples_trained > 0);
    }

    #[test]
    fn coordinator_flush_trains_partial_batch() {
        let mut coord = TrainingCoordinator::new();
        for i in 0..10 {
            coord.record_trace(sample_trace(4, 8.0 + f64::from(i)));
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

    /// Regression for A2: repeated single-dim feedback must not collapse
    /// the routing dimensions toward zero. We compare a masked
    /// feedback stream (post-A2 behaviour) against an unmasked
    /// hard-zeroing stream (pre-A2 behaviour) and assert masked is
    /// strictly less destructive.
    #[test]
    fn record_feedback_preserves_routing_dims_better_than_unmasked() {
        // Build a model with a deliberate non-zero signal on dim 12
        // (router difficulty) by training a single-sample bias signal
        // through the trace path which sets mask[12]=true.
        let prep = || -> TrainingCoordinator {
            let mut coord = TrainingCoordinator::new();
            // Submit traces that deliberately encode dim-12 difficulty.
            for _ in 0..200 {
                coord.record_trace(sample_trace(8, 30.0));
            }
            coord.flush();
            // Snapshot.
            for _ in 0..2 { coord.record_trace(sample_trace(8, 30.0)); }
            coord.flush();
            coord
        };

        let features = QueryFeatures {
            table_count: 3.0, join_count: 2.0, filter_count: 1.0,
            aggregate_count: 0.0, subquery_count: 0.0, cte_count: 0.0,
            window_function_count: 0.0, order_by_count: 0.0,
            group_by_count: 0.0, distinct_flag: 0.0, limit_present: 0.0,
            max_join_cardinality: 3.0,
        };

        let mut coord_masked = prep();
        let dim12_pre = coord_masked.current_model().predict_all(&features.as_array())[12];

        // Stream 256 masked feedbacks (only dim 0 supervised). Forces
        // a snapshot at step 256.
        for _ in 0..256 {
            coord_masked.record_feedback(&features, 5.0);
        }
        let dim12_masked = coord_masked
            .current_model()
            .predict_all(&features.as_array())[12];

        // Now do the same on a fresh coordinator using the pre-A2 path
        // (full target with hard-zero on dim 12).
        let mut coord_zeroed = prep();
        for _ in 0..256 {
            // Direct call to the underlying trainer mimicking pre-A2:
            let mut target = [0.0f32; 16];
            target[0] = 5.0;
            coord_zeroed
                .trainer
                .train_step(&features.as_array(), &target);
            coord_zeroed.total_train_steps += 1;
        }
        coord_zeroed.snapshot_model();
        let dim12_zeroed = coord_zeroed
            .current_model()
            .predict_all(&features.as_array())[12];

        // Masked feedback should preserve dim 12 strictly closer to
        // pre-feedback than the zeroed (pre-A2) path.
        let drift_masked = (dim12_masked - dim12_pre).abs();
        let drift_zeroed = (dim12_zeroed - dim12_pre).abs();
        assert!(
            drift_masked < drift_zeroed,
            "masked feedback did not preserve dim 12 better than zeroed: \
             pre={dim12_pre:.3} masked={dim12_masked:.3} zeroed={dim12_zeroed:.3} \
             drift_masked={drift_masked:.3} drift_zeroed={drift_zeroed:.3}"
        );
    }

    /// Verify the masked partial-feedback API works as advertised.
    #[test]
    fn record_feedback_partial_only_trains_specified_dims() {
        let mut coord = TrainingCoordinator::new();
        let features = QueryFeatures {
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
            max_join_cardinality: 1000.0,
        };
        // Observe CPU and memory only.
        coord.record_feedback_partial(&features, &[(0, 1.5), (1, 200.0)]);
        assert_eq!(coord.stats().total_train_steps, 1);

        // Empty observations must be a no-op.
        coord.record_feedback_partial(&features, &[]);
        assert_eq!(coord.stats().total_train_steps, 1);
    }

    /// Regression for G9: `bootstrap_model()` must run real training,
    /// not just declare `samples_trained = 10000` while leaving weights
    /// untrained. A genuinely-trained model should:
    ///   1. Report a `samples_trained` count tied to the actual training
    ///      run (not the pre-G9 marketing constant 10000).
    ///   2. Produce different predictions than the all-zeros baseline.
    ///   3. Distinguish trivial queries (low predicted cost) from
    ///      complex queries (higher predicted cost).
    #[test]
    fn bootstrap_model_is_actually_trained() {
        let model = bootstrap_model();

        // (1) samples_trained reflects real training steps, not 10000.
        assert_ne!(
            model.samples_trained, 10_000,
            "bootstrap_model should not declare the pre-G9 marketing constant"
        );
        assert!(
            model.samples_trained > 0,
            "bootstrap_model should report a real training step count, got {}",
            model.samples_trained
        );

        // (2) Predictions differ from the all-zeros baseline. The
        // feature layout matches `OptimizationFeatures::as_array()`,
        // which is what the speculative router and `bootstrap_model`'s
        // training samples use. Position legend (post-A4):
        //   0-5 : table/join/filter/aggregate/subquery/window counts
        //   6-9 : density / max_fan_out / equi_join_fraction / cross_join_present
        //   10-12: selectivity / has_limit / has_distinct_or_group
        //   13-15: log_estimated_rows / total_table_pages / index_coverage
        let baseline = BitNetCostModel::new_zeros();
        let trivial = [
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, // 1 table, no other structure
            0.0, 0.0, 1.0, 0.0,           // density 0, equi-fraction 1
            1.0, 0.0, 0.0,                // selectivity full, no limit/distinct
            0.0, 0.0, 1.0,                // 0 rows estimated, full index coverage
        ];
        let complex = [
            6.0, 5.0, 3.0, 1.0, 0.0, 0.0, // 6 tables, 5 joins
            0.3, 4.0, 0.3, 1.0,           // sparse, high fan-out, cross joins
            0.05, 0.0, 1.0,               // tight predicates, distinct
            5.0, 1000.0, 0.0,             // 10^5 rows, no indexes
        ];
        assert!(
            (model.predict_cpu_ms(&trivial) - baseline.predict_cpu_ms(&trivial)).abs()
                + (model.predict_cpu_ms(&complex) - baseline.predict_cpu_ms(&complex)).abs()
                > 0.1,
            "bootstrap_model produces baseline predictions (training had no effect)"
        );

        // (3) Trivial queries should predict lower CPU cost than complex
        //     ones. The training set explicitly encodes this gradient
        //     (trivial → 0.01ms, complex → up to 165ms).
        let p_trivial = model.predict_cpu_ms(&trivial);
        let p_complex = model.predict_cpu_ms(&complex);
        assert!(
            p_complex >= p_trivial,
            "bootstrap-trained model should rank complex >= trivial: \
             trivial={p_trivial:.3}, complex={p_complex:.3}"
        );
    }
}
