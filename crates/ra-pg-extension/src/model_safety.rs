//! Model version management with automatic rollback on quality regression.
//!
//! When a new neural cost model is promoted (after distillation), the previous
//! version is retained. If the new model's prediction accuracy (MAPE) regresses
//! beyond a configurable threshold relative to the previous model, the manager
//! automatically rolls back to the previous version.

use std::sync::Arc;

use ra_engine::cost_model::feedback::MapeTracker;
use ra_engine::cost_model::FastCostModel;

/// Configuration for model safety rollback behavior.
pub struct ModelSafetyConfig {
    /// Rollback if current MAPE > this multiplier x previous MAPE.
    pub rollback_threshold: f64,
    /// Number of predictions before evaluating rollback.
    pub evaluation_window: u64,
    /// Whether automatic rollback is enabled.
    pub enabled: bool,
}

impl Default for ModelSafetyConfig {
    fn default() -> Self {
        Self {
            rollback_threshold: 2.0,
            evaluation_window: 100,
            enabled: true,
        }
    }
}

/// Manages model versions with automatic rollback on quality regression.
///
/// Held behind a `Mutex` in the extension state. The planner hook obtains
/// an `Arc<FastCostModel>` via `current_model()` and can use it without
/// holding the lock during optimization.
pub struct ModelVersionManager {
    /// Currently active model.
    current: Arc<FastCostModel>,
    /// Previous model version (for rollback).
    previous: Option<Arc<FastCostModel>>,
    /// MAPE tracker for current model.
    current_mape: MapeTracker,
    /// MAPE tracker for previous model (during A/B window).
    previous_mape: MapeTracker,
    /// Number of predictions since last model swap.
    predictions_since_swap: u64,
    /// Total rollbacks performed.
    rollback_count: u64,
    /// Current model version number.
    version: u64,
    /// Configuration.
    config: ModelSafetyConfig,
}

/// Status snapshot for monitoring.
pub struct ModelStatus {
    pub version: u64,
    pub current_mape: f32,
    pub previous_mape: Option<f32>,
    pub predictions_since_swap: u64,
    pub rollback_count: u64,
    pub has_previous: bool,
}

impl ModelVersionManager {
    /// Create a new manager with the initial model (version 1).
    #[must_use]
    pub fn new(initial_model: FastCostModel) -> Self {
        Self {
            current: Arc::new(initial_model),
            previous: None,
            current_mape: MapeTracker::new(),
            previous_mape: MapeTracker::new(),
            predictions_since_swap: 0,
            rollback_count: 0,
            version: 1,
            config: ModelSafetyConfig::default(),
        }
    }

    /// Create with custom configuration.
    #[must_use]
    pub fn with_config(initial_model: FastCostModel, config: ModelSafetyConfig) -> Self {
        Self {
            current: Arc::new(initial_model),
            previous: None,
            current_mape: MapeTracker::new(),
            previous_mape: MapeTracker::new(),
            predictions_since_swap: 0,
            rollback_count: 0,
            version: 1,
            config,
        }
    }

    /// Swap in a new model version (called after distillation).
    ///
    /// Current becomes previous, new becomes current. Resets prediction
    /// counters and MAPE trackers for the new evaluation window.
    pub fn promote_model(&mut self, new_model: FastCostModel) {
        self.previous = Some(Arc::clone(&self.current));
        self.current = Arc::new(new_model);
        self.previous_mape = std::mem::replace(&mut self.current_mape, MapeTracker::new());
        self.predictions_since_swap = 0;
        self.version += 1;
    }

    /// Record a prediction/actual pair for quality tracking.
    ///
    /// Returns `true` if a rollback was triggered (current model reverted
    /// to previous version due to MAPE regression).
    pub fn record_prediction(&mut self, predicted: f64, actual: f64) -> bool {
        self.current_mape.record(predicted, actual);
        self.predictions_since_swap += 1;

        if !self.config.enabled {
            return false;
        }

        if self.predictions_since_swap < self.config.evaluation_window {
            return false;
        }

        // Evaluate rollback at window boundary
        if let Some(ref prev) = self.previous {
            let current = f64::from(self.current_mape.current_mape());
            let previous = f64::from(self.previous_mape.current_mape());

            if previous > 0.0 && current > self.config.rollback_threshold * previous {
                // Rollback: swap previous back to current
                self.current = Arc::clone(prev);
                self.previous = None;
                self.current_mape = std::mem::replace(&mut self.previous_mape, MapeTracker::new());
                self.predictions_since_swap = 0;
                self.rollback_count += 1;
                self.version += 1;

                tracing::warn!(
                    rollback_count = self.rollback_count,
                    current_mape = current,
                    previous_mape = previous,
                    "Model rollback triggered: current MAPE {current:.4} \
                     exceeds {:.1}x previous MAPE {previous:.4}",
                    self.config.rollback_threshold,
                );

                return true;
            }
        }

        // Window passed without rollback; clear previous to free memory
        self.previous = None;
        self.predictions_since_swap = 0;
        false
    }

    /// Get the current active model for use in the planner hook.
    #[must_use]
    pub fn current_model(&self) -> Arc<FastCostModel> {
        Arc::clone(&self.current)
    }

    /// Force a manual rollback to previous version.
    ///
    /// Returns `true` if rollback succeeded, `false` if no previous
    /// version is available.
    pub fn force_rollback(&mut self) -> bool {
        if let Some(ref prev) = self.previous {
            self.current = Arc::clone(prev);
            self.previous = None;
            self.current_mape = std::mem::replace(&mut self.previous_mape, MapeTracker::new());
            self.predictions_since_swap = 0;
            self.rollback_count += 1;
            self.version += 1;

            tracing::info!(
                rollback_count = self.rollback_count,
                "Manual model rollback performed"
            );

            true
        } else {
            false
        }
    }

    /// Status snapshot for monitoring.
    #[must_use]
    pub fn status(&self) -> ModelStatus {
        ModelStatus {
            version: self.version,
            current_mape: self.current_mape.current_mape(),
            previous_mape: self
                .previous
                .as_ref()
                .map(|_| self.previous_mape.current_mape()),
            predictions_since_swap: self.predictions_since_swap,
            rollback_count: self.rollback_count,
            has_previous: self.previous.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> ModelVersionManager {
        ModelVersionManager::new(FastCostModel::new_random())
    }

    fn make_manager_with_window(window: u64) -> ModelVersionManager {
        ModelVersionManager::with_config(
            FastCostModel::new_random(),
            ModelSafetyConfig {
                rollback_threshold: 2.0,
                evaluation_window: window,
                enabled: true,
            },
        )
    }

    #[test]
    fn test_promote_model_saves_previous() {
        let mut mgr = make_manager();
        assert!(!mgr.status().has_previous);
        assert_eq!(mgr.status().version, 1);

        mgr.promote_model(FastCostModel::new_random());
        assert!(mgr.status().has_previous);
        assert_eq!(mgr.status().version, 2);
    }

    #[test]
    fn test_rollback_on_mape_regression() {
        let mut mgr = make_manager_with_window(10);

        // Feed good predictions so previous_mape is low
        for _ in 0..50 {
            mgr.record_prediction(100.0, 100.0);
        }

        // Promote a new model
        mgr.promote_model(FastCostModel::new_random());
        assert!(mgr.status().has_previous);
        let version_before = mgr.status().version;

        // Feed terrible predictions to trigger rollback (>2x previous MAPE)
        let mut rolled_back = false;
        for _ in 0..10 {
            if mgr.record_prediction(500.0, 100.0) {
                rolled_back = true;
                break;
            }
        }

        assert!(rolled_back, "Expected rollback to be triggered");
        assert_eq!(mgr.status().version, version_before + 1);
        assert_eq!(mgr.status().rollback_count, 1);
        assert!(!mgr.status().has_previous);
    }

    #[test]
    fn test_no_rollback_when_model_improves() {
        let mut mgr = make_manager_with_window(10);

        // Feed mediocre predictions to establish baseline
        for _ in 0..50 {
            mgr.record_prediction(150.0, 100.0); // 50% error
        }

        // Promote a new model
        mgr.promote_model(FastCostModel::new_random());
        let version_before = mgr.status().version;

        // Feed better predictions (lower error than before)
        let mut rolled_back = false;
        for _ in 0..10 {
            if mgr.record_prediction(105.0, 100.0) {
                rolled_back = true;
            }
        }

        assert!(!rolled_back, "Should not rollback when model improves");
        assert_eq!(mgr.status().version, version_before);
        assert_eq!(mgr.status().rollback_count, 0);
    }

    #[test]
    fn test_force_rollback() {
        let mut mgr = make_manager();

        // No previous version available
        assert!(!mgr.force_rollback());

        // Promote and then force rollback
        mgr.promote_model(FastCostModel::new_random());
        assert!(mgr.status().has_previous);
        assert!(mgr.force_rollback());
        assert!(!mgr.status().has_previous);
        assert_eq!(mgr.status().rollback_count, 1);
    }

    #[test]
    fn test_no_rollback_before_window_complete() {
        let mut mgr = make_manager_with_window(100);

        // Establish baseline
        for _ in 0..200 {
            mgr.record_prediction(100.0, 100.0);
        }

        mgr.promote_model(FastCostModel::new_random());

        // Feed terrible predictions but fewer than the evaluation window
        let mut rolled_back = false;
        for _ in 0..99 {
            if mgr.record_prediction(500.0, 100.0) {
                rolled_back = true;
            }
        }

        assert!(
            !rolled_back,
            "Should not rollback before evaluation window completes"
        );
        assert!(mgr.status().has_previous);
    }

    #[test]
    fn test_current_model_returns_arc() {
        let mgr = make_manager();
        let model = mgr.current_model();
        // Verify we can clone the Arc without issue
        let _model2 = Arc::clone(&model);
        assert!(Arc::strong_count(&model) >= 2);
    }

    #[test]
    fn test_disabled_config_prevents_rollback() {
        let mut mgr = ModelVersionManager::with_config(
            FastCostModel::new_random(),
            ModelSafetyConfig {
                rollback_threshold: 2.0,
                evaluation_window: 10,
                enabled: false,
            },
        );

        // Establish baseline
        for _ in 0..50 {
            mgr.record_prediction(100.0, 100.0);
        }

        mgr.promote_model(FastCostModel::new_random());

        // Terrible predictions but rollback is disabled
        let mut rolled_back = false;
        for _ in 0..20 {
            if mgr.record_prediction(500.0, 100.0) {
                rolled_back = true;
            }
        }

        assert!(!rolled_back, "Should not rollback when disabled");
    }
}
