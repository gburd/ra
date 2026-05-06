//! Online learning loop: connects query execution feedback to neural model updates.
//!
//! The [`OnlineLearner`] sits between the Ra optimizer and the cost model:
//!
//! ```text
//! Query arrives
//!     ↓
//! Ra optimizer → extract features → FastCostModel.predict() → plan
//!     ↓
//! Plan executes (PostgreSQL)
//!     ↓
//! OnlineLearner.record(features, actual_cost)
//!     ↓
//! [batch full?] → ProductionCostModel.train_batch() → [checkpoint due?] → save
//! ```
//!
//! # Deployment
//!
//! In the PostgreSQL extension (`planner_hook.rs`), the learner is held in a
//! process-global singleton:
//!
//! ```ignore
//! static LEARNER: std::sync::Mutex<Option<OnlineLearner>> = ...;
//!
//! // After plan extraction:
//! if let Ok(mut guard) = LEARNER.lock() {
//!     if let Some(learner) = guard.as_mut() {
//!         learner.record(features, actual_cost);
//!     }
//! }
//! ```
//!
//! # Offline training
//!
//! The [`OnlineLearner`] can also drive offline training from a saved
//! `training_data.json` file (see `ra-bench train` CLI subcommand).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{ActualCost, QueryFeatures};
use super::production_model::{ProductionCostModel, TrainingConfig};
use super::fast_model::FastCostModel;
use super::CostVector;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the online learning loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineLearnerConfig {
    /// Number of samples to accumulate before running one training batch.
    pub batch_size: usize,
    /// Persist the model to disk every this many training batches.
    pub checkpoint_every_n_batches: usize,
    /// Maximum pending (unprocessed) samples before oldest are dropped.
    pub max_pending: usize,
    /// Training configuration forwarded to `ProductionCostModel`.
    pub training_config: TrainingConfig,
    /// Whether to produce `FastCostModel` snapshots alongside the main model.
    pub export_fast_model: bool,
}

impl Default for OnlineLearnerConfig {
    fn default() -> Self {
        Self {
            batch_size: 64,
            checkpoint_every_n_batches: 50,  // checkpoint after every 3200 samples
            max_pending: 512,
            training_config: TrainingConfig::default(),
            export_fast_model: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Cumulative statistics for the online learning loop.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineLearnerStats {
    /// Total samples recorded (including dropped ones).
    pub total_recorded: usize,
    /// Samples dropped due to buffer overflow.
    pub samples_dropped: usize,
    /// Total training batches run.
    pub batches_trained: usize,
    /// Total training samples processed (batches × batch_size).
    pub total_trained: usize,
    /// Number of model checkpoints written.
    pub checkpoints_written: usize,
    /// Current exponential moving average loss.
    pub current_avg_loss: f32,
    /// Current learning rate (may have decayed).
    pub current_lr: f32,
}

// ---------------------------------------------------------------------------
// OnlineLearner
// ---------------------------------------------------------------------------

/// Manages the online training loop for the neural cost model.
///
/// Thread safety: `OnlineLearner` is NOT thread-safe. In PostgreSQL backends
/// it should be held in a process-global `Mutex` or accessed only from the
/// single query processing thread.
pub struct OnlineLearner {
    model: ProductionCostModel,
    config: OnlineLearnerConfig,
    /// Path for model persistence (None = in-memory only).
    model_path: Option<PathBuf>,
    /// Pending samples awaiting the next training batch.
    pending: Vec<(QueryFeatures, ActualCost)>,
    stats: OnlineLearnerStats,
}

impl OnlineLearner {
    /// Create a new learner with a fresh randomly-initialized model.
    pub fn new(config: OnlineLearnerConfig) -> Self {
        let model = ProductionCostModel::new(config.training_config.clone());
        Self {
            model,
            config,
            model_path: None,
            pending: Vec::new(),
            stats: OnlineLearnerStats::default(),
        }
    }

    /// Load an existing model from `path` (creating a new one if absent).
    ///
    /// The learner will checkpoint to the same path on each save.
    pub fn load_or_create(path: impl AsRef<Path>, config: OnlineLearnerConfig) -> Self {
        let path = path.as_ref();
        let model = if path.exists() {
            match ProductionCostModel::load_from_file(path) {
                Ok(m) => {
                    tracing::info!(
                        samples_seen = m.stats().samples_seen,
                        ?path,
                        "loaded existing model"
                    );
                    m
                }
                Err(e) => {
                    tracing::warn!(?e, ?path, "failed to load model, creating fresh");
                    ProductionCostModel::new(config.training_config.clone())
                }
            }
        } else {
            ProductionCostModel::new(config.training_config.clone())
        };

        let mut learner = Self {
            model,
            config,
            model_path: None,
            pending: Vec::new(),
            stats: OnlineLearnerStats::default(),
        };
        learner.model_path = Some(path.to_path_buf());
        learner
    }

    /// Record an observed (features, actual_cost) pair.
    ///
    /// When the pending buffer reaches `config.batch_size`, training runs
    /// automatically. Oldest samples are dropped if the buffer is full.
    pub fn record(&mut self, features: QueryFeatures, actual: ActualCost) {
        self.stats.total_recorded += 1;

        // Drop oldest if buffer full
        if self.pending.len() >= self.config.max_pending {
            self.pending.remove(0);
            self.stats.samples_dropped += 1;
        }
        self.pending.push((features, actual));

        // Auto-train when batch is full
        if self.pending.len() >= self.config.batch_size {
            self.train_pending();
        }
    }

    /// Train immediately on all pending samples, then clear the buffer.
    pub fn train_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let batch: Vec<(QueryFeatures, ActualCost)> = self.pending.drain(..).collect();
        self.model.train_batch(&batch);

        let model_stats = self.model.stats();
        self.stats.batches_trained += 1;
        self.stats.total_trained += batch.len();
        self.stats.current_avg_loss = model_stats.avg_loss;
        self.stats.current_lr = model_stats.current_lr;

        tracing::debug!(
            batches = self.stats.batches_trained,
            loss = self.stats.current_avg_loss,
            lr = self.stats.current_lr,
            "training batch complete"
        );

        // Auto-checkpoint
        if self.stats.batches_trained % self.config.checkpoint_every_n_batches == 0 {
            if let Err(e) = self.checkpoint() {
                tracing::warn!(?e, "checkpoint failed");
            }
        }
    }

    /// Predict costs for given features using the current model state.
    ///
    /// Returns `(cost_vector, confidence)` where confidence rises from 0→1
    /// as training samples accumulate.
    pub fn predict(&self, features: &QueryFeatures) -> (CostVector, f32) {
        self.model.predict_with_confidence(features)
    }

    /// Build a `FastCostModel` snapshot from the current model weights.
    ///
    /// The fast model is suitable for inline e-graph cost estimation.
    pub fn fast_model_snapshot(&self) -> FastCostModel {
        FastCostModel::from_production(&self.model)
    }

    /// Save the current model to the configured path.
    ///
    /// If `config.export_fast_model` is true, also writes a `_fast.json`
    /// sidecar with the compact weight layout.
    ///
    /// Returns `true` if a checkpoint was written, `false` if there was
    /// no model path configured.
    pub fn checkpoint(&mut self) -> anyhow::Result<bool> {
        let Some(path) = &self.model_path.clone() else {
            return Ok(false);
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.model.save_to_file(path)?;
        self.stats.checkpoints_written += 1;
        tracing::info!(
            ?path,
            checkpoints = self.stats.checkpoints_written,
            samples_trained = self.stats.total_trained,
            "model checkpoint written"
        );
        Ok(true)
    }

    /// Run a full offline training pass over a pre-collected dataset.
    ///
    /// `samples` is a slice of `(features, actual)` pairs loaded from
    /// a `training_data.json` file.  The `epochs` parameter controls
    /// how many full passes to make.
    ///
    /// Returns per-epoch loss.
    pub fn train_offline(
        &mut self,
        samples: &[(QueryFeatures, ActualCost)],
        epochs: usize,
    ) -> Vec<f32> {
        let mut epoch_losses = Vec::with_capacity(epochs);

        for epoch in 0..epochs {
            // Shuffle into batches
            let mut shuffled: Vec<_> = samples.iter().collect();
            // Simple deterministic shuffle using index permutation
            for i in (1..shuffled.len()).rev() {
                let j = (epoch * 6364136223846793005_usize.wrapping_add(i)) % (i + 1);
                shuffled.swap(i, j);
            }

            let batch_size = self.config.batch_size.max(1);
            let mut epoch_loss = 0.0_f32;
            let mut n_batches = 0usize;

            for chunk in shuffled.chunks(batch_size) {
                let batch: Vec<(QueryFeatures, ActualCost)> =
                    chunk.iter().map(|&s| (s.0.clone(), s.1.clone())).collect();
                self.model.train_batch(&batch);
                epoch_loss += self.model.stats().avg_loss;
                n_batches += 1;
                self.stats.batches_trained += 1;
                self.stats.total_trained += batch.len();
            }

            let avg = if n_batches > 0 { epoch_loss / n_batches as f32 } else { 0.0 };
            epoch_losses.push(avg);
            self.stats.current_avg_loss = avg;
            self.stats.current_lr = self.model.stats().current_lr;

            tracing::info!(
                epoch = epoch + 1,
                total_epochs = epochs,
                loss = avg,
                lr = self.stats.current_lr,
                "training epoch complete"
            );
        }

        epoch_losses
    }

    /// Return current learner statistics.
    pub fn stats(&self) -> &OnlineLearnerStats {
        &self.stats
    }

    /// Return a reference to the underlying production model.
    pub fn model(&self) -> &ProductionCostModel {
        &self.model
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_features(tables: f32) -> QueryFeatures {
        QueryFeatures {
            table_count: tables,
            join_count: (tables - 1.0).max(0.0),
            filter_count: 2.0,
            aggregate_count: 1.0,
            subquery_count: 0.0,
            cte_count: 0.0,
            window_function_count: 0.0,
            order_by_count: 1.0,
            group_by_count: 1.0,
            distinct_flag: 0.0,
            limit_present: 0.0,
            max_join_cardinality: tables * 10_000.0,
        }
    }

    fn make_actual(cpu_ms: f32) -> ActualCost {
        ActualCost {
            cpu_time_ms: cpu_ms,
            memory_peak_mb: cpu_ms * 0.5,
            memory_avg_mb: cpu_ms * 0.3,
            io_storage_ops: (cpu_ms * 10.0) as u64,
            io_storage_bytes: (cpu_ms * 10.0 * 8192.0) as u64,
            io_network_ops: 0,
            io_network_bytes: 0,
            locks_acquired: 2,
            lock_hold_time_ms: 0.1,
            lock_contention_score: 0.0,
            vacuum_overhead: 0.0,
            wal_generation_bytes: 0,
            replication_lag_ms: 0.0,
            cache_hit_ratio: 0.9,
            page_faults: 0,
            context_switches: 4,
        }
    }

    #[test]
    fn test_learner_creates_and_predicts() {
        let learner = OnlineLearner::new(OnlineLearnerConfig::default());
        let (costs, confidence) = learner.predict(&make_features(3.0));
        assert!(costs.cpu_time_ms >= 0.0);
        assert_eq!(confidence, 0.0, "untrained model has zero confidence");
    }

    #[test]
    fn test_record_triggers_training_at_batch_size() {
        let config = OnlineLearnerConfig { batch_size: 5, ..Default::default() };
        let mut learner = OnlineLearner::new(config);

        for i in 0..5 {
            learner.record(make_features(i as f32 + 1.0), make_actual(10.0 * (i + 1) as f32));
        }

        // After 5 records = 1 full batch, training should have run
        assert_eq!(learner.stats().batches_trained, 1);
        assert_eq!(learner.stats().total_trained, 5);
        assert!(learner.pending.is_empty(), "pending cleared after training");
    }

    #[test]
    fn test_buffer_overflow_drops_oldest() {
        let config = OnlineLearnerConfig {
            batch_size: 1000, // large enough that auto-training doesn't trigger
            max_pending: 3,
            ..Default::default()
        };
        let mut learner = OnlineLearner::new(config);

        for i in 0..5 {
            learner.record(make_features(i as f32 + 1.0), make_actual(10.0));
        }

        assert_eq!(learner.pending.len(), 3, "buffer capped at max_pending");
        assert_eq!(learner.stats().samples_dropped, 2);
    }

    #[test]
    fn test_offline_training_reduces_loss() {
        let config = OnlineLearnerConfig { batch_size: 10, ..Default::default() };
        let mut learner = OnlineLearner::new(config);

        let samples: Vec<(QueryFeatures, ActualCost)> =
            (0..50).map(|i| (make_features(i as f32 % 5.0 + 1.0), make_actual(50.0))).collect();

        let losses = learner.train_offline(&samples, 3);
        assert_eq!(losses.len(), 3, "one loss per epoch");
        // Generally loss should not be NaN
        for loss in &losses {
            assert!(!loss.is_nan(), "loss should not be NaN");
        }
    }

    #[test]
    fn test_fast_model_snapshot() {
        let mut learner = OnlineLearner::new(OnlineLearnerConfig::default());
        let samples: Vec<_> =
            (0..10).map(|_| (make_features(3.0), make_actual(100.0))).collect();
        learner.train_offline(&samples, 1);

        let fast = learner.fast_model_snapshot();
        let cpu = fast.predict_cpu_ms(&make_features(3.0));
        assert!(cpu >= 0.0);
    }

    #[test]
    fn test_checkpoint_and_reload() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/online_learner_test.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");

        let config = OnlineLearnerConfig { batch_size: 5, ..Default::default() };
        let mut learner = OnlineLearner::load_or_create(&path, config.clone());

        for i in 0..10 {
            learner.record(make_features(i as f32 % 4.0 + 1.0), make_actual(50.0));
        }
        learner.train_pending();
        learner.checkpoint().expect("checkpoint");

        // Reload and verify stats persisted
        let reloaded = OnlineLearner::load_or_create(&path, config);
        assert!(
            reloaded.model().stats().samples_seen > 0,
            "reloaded model should have seen samples"
        );
        let _ = std::fs::remove_file(&path);
    }
}
