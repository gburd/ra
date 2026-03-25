//! Adaptive cost model integration.
//!
//! Bridges the streaming statistics pipeline with cost model updates.
//! The [`AdaptiveCostDriver`] monitors smoothed resource metrics and
//! conditionally triggers cost model recalibration only when changes
//! exceed configurable thresholds, with a minimum update interval to
//! prevent excessive recomputation.
//!
//! This implements the "conditional cost model updates" pattern:
//! - Only trigger on >10% change (default) in any resource metric
//! - Enforce minimum 100ms between updates
//! - Track update history for observability

use std::time::{Duration, Instant};

/// Default fractional change threshold to trigger a cost update.
const DEFAULT_CHANGE_THRESHOLD: f64 = 0.10;

/// Default minimum interval between cost model updates.
const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(100);

/// Configuration for adaptive cost model updates.
#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    /// Fractional change required to trigger an update (default 0.10).
    pub change_threshold: f64,
    /// Minimum time between updates (default 100ms).
    pub min_interval: Duration,
    /// Maximum number of updates to retain in history.
    pub max_history: usize,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            change_threshold: DEFAULT_CHANGE_THRESHOLD,
            min_interval: DEFAULT_MIN_INTERVAL,
            max_history: 1000,
        }
    }
}

/// A snapshot of resource metrics used for cost model calibration.
#[derive(Debug, Clone, Copy)]
pub struct ResourceSnapshot {
    /// CPU utilization (0.0-1.0 or 0-100 depending on source).
    pub cpu: f64,
    /// Memory usage metric.
    pub memory: f64,
    /// I/O throughput or latency metric.
    pub io: f64,
}

/// Record of a cost model update event.
#[derive(Debug, Clone)]
pub struct UpdateRecord {
    /// Snapshot that triggered the update.
    pub snapshot: ResourceSnapshot,
    /// Which resource(s) triggered the update.
    pub trigger: UpdateTrigger,
    /// When the update occurred.
    pub timestamp: Instant,
}

/// Which resource metric(s) exceeded the change threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateTrigger {
    /// CPU metric changed beyond threshold.
    Cpu,
    /// Memory metric changed beyond threshold.
    Memory,
    /// I/O metric changed beyond threshold.
    Io,
    /// Multiple metrics changed simultaneously.
    Multiple,
    /// Update was forced (not threshold-based).
    Forced,
}

/// Drives conditional cost model updates based on resource changes.
///
/// Tracks the last accepted resource snapshot and only produces
/// update events when metrics shift by more than the configured
/// threshold and the minimum interval has elapsed.
pub struct AdaptiveCostDriver {
    config: AdaptiveConfig,
    last_snapshot: Option<ResourceSnapshot>,
    last_update_time: Instant,
    history: Vec<UpdateRecord>,
    update_count: u64,
    suppressed_count: u64,
    eval_count: u64,
}

impl AdaptiveCostDriver {
    /// Create a driver with default configuration.
    pub fn new() -> Self {
        Self::with_config(AdaptiveConfig::default())
    }

    /// Create a driver with custom configuration.
    pub fn with_config(config: AdaptiveConfig) -> Self {
        // Initialize last_update_time far enough in the past so the
        // first evaluation is never throttled by the min_interval.
        let past = Instant::now()
            .checked_sub(config.min_interval + Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        Self {
            config,
            last_snapshot: None,
            last_update_time: past,
            history: Vec::new(),
            update_count: 0,
            suppressed_count: 0,
            eval_count: 0,
        }
    }

    /// Evaluate a new resource snapshot against thresholds.
    ///
    /// Returns `Some(UpdateRecord)` if the change is large enough
    /// and the minimum interval has elapsed; `None` otherwise.
    pub fn evaluate(
        &mut self,
        snapshot: ResourceSnapshot,
    ) -> Option<UpdateRecord> {
        self.eval_count += 1;
        let now = Instant::now();

        // Skip the interval check on the very first evaluation
        // (no prior snapshot). Otherwise enforce the minimum interval.
        if self.last_snapshot.is_some()
            && now.duration_since(self.last_update_time)
                < self.config.min_interval
        {
            self.suppressed_count += 1;
            return None;
        }

        let trigger = match &self.last_snapshot {
            None => UpdateTrigger::Forced,
            Some(prev) => {
                let cpu_changed = exceeds_threshold(
                    prev.cpu,
                    snapshot.cpu,
                    self.config.change_threshold,
                );
                let mem_changed = exceeds_threshold(
                    prev.memory,
                    snapshot.memory,
                    self.config.change_threshold,
                );
                let io_changed = exceeds_threshold(
                    prev.io,
                    snapshot.io,
                    self.config.change_threshold,
                );

                match (cpu_changed, mem_changed, io_changed) {
                    (false, false, false) => {
                        self.suppressed_count += 1;
                        return None;
                    }
                    (true, false, false) => UpdateTrigger::Cpu,
                    (false, true, false) => UpdateTrigger::Memory,
                    (false, false, true) => UpdateTrigger::Io,
                    _ => UpdateTrigger::Multiple,
                }
            }
        };

        self.last_snapshot = Some(snapshot);
        self.last_update_time = now;
        self.update_count += 1;

        let record = UpdateRecord {
            snapshot,
            trigger,
            timestamp: now,
        };

        self.history.push(record.clone());
        if self.history.len() > self.config.max_history {
            self.history.remove(0);
        }

        Some(record)
    }

    /// Force an update regardless of thresholds or interval.
    pub fn force_update(
        &mut self,
        snapshot: ResourceSnapshot,
    ) -> UpdateRecord {
        let now = Instant::now();
        self.last_snapshot = Some(snapshot);
        self.last_update_time = now;
        self.update_count += 1;

        let record = UpdateRecord {
            snapshot,
            trigger: UpdateTrigger::Forced,
            timestamp: now,
        };

        self.history.push(record.clone());
        if self.history.len() > self.config.max_history {
            self.history.remove(0);
        }

        record
    }

    /// Total evaluations performed.
    pub fn eval_count(&self) -> u64 {
        self.eval_count
    }

    /// Updates that were produced (not suppressed).
    pub fn update_count(&self) -> u64 {
        self.update_count
    }

    /// Evaluations that were suppressed (under threshold or interval).
    pub fn suppressed_count(&self) -> u64 {
        self.suppressed_count
    }

    /// Suppression ratio: fraction of evaluations that were filtered.
    pub fn suppression_ratio(&self) -> f64 {
        if self.eval_count == 0 {
            return 0.0;
        }
        self.suppressed_count as f64 / self.eval_count as f64
    }

    /// The last accepted resource snapshot.
    pub fn last_snapshot(&self) -> Option<&ResourceSnapshot> {
        self.last_snapshot.as_ref()
    }

    /// Update history (bounded by `max_history`).
    pub fn history(&self) -> &[UpdateRecord] {
        &self.history
    }

    /// Active configuration.
    pub fn config(&self) -> &AdaptiveConfig {
        &self.config
    }

    /// Reset all state (keeps configuration).
    pub fn reset(&mut self) {
        self.last_snapshot = None;
        self.last_update_time = Instant::now()
            .checked_sub(
                self.config.min_interval + Duration::from_secs(1),
            )
            .unwrap_or_else(Instant::now);
        self.history.clear();
        self.update_count = 0;
        self.suppressed_count = 0;
        self.eval_count = 0;
    }
}

impl Default for AdaptiveCostDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AdaptiveCostDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdaptiveCostDriver")
            .field("update_count", &self.update_count)
            .field("suppressed_count", &self.suppressed_count)
            .field("eval_count", &self.eval_count)
            .field("threshold", &self.config.change_threshold)
            .finish_non_exhaustive()
    }
}

/// Check whether the relative change between `old` and `new` exceeds
/// the given threshold fraction.
fn exceeds_threshold(old: f64, new: f64, threshold: f64) -> bool {
    if old.abs() < f64::EPSILON {
        return new.abs() > f64::EPSILON;
    }
    ((new - old) / old).abs() > threshold
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::cast_lossless
)]
mod tests {
    use super::*;

    fn snap(cpu: f64, mem: f64, io: f64) -> ResourceSnapshot {
        ResourceSnapshot {
            cpu,
            memory: mem,
            io,
        }
    }

    #[test]
    fn first_evaluation_always_triggers() {
        let mut driver = AdaptiveCostDriver::new();
        let result = driver.evaluate(snap(50.0, 1024.0, 100.0));
        assert!(result.is_some());
        assert_eq!(result.unwrap().trigger, UpdateTrigger::Forced);
        assert_eq!(driver.update_count(), 1);
    }

    #[test]
    fn small_change_suppressed() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(50.0, 1024.0, 100.0));

        // 5% change, below 10% threshold
        let result = driver.evaluate(snap(52.5, 1024.0, 100.0));
        assert!(result.is_none());
        assert_eq!(driver.suppressed_count(), 1);
    }

    #[test]
    fn large_cpu_change_triggers() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(50.0, 1024.0, 100.0));

        // 20% CPU change, above 10% threshold
        let result = driver.evaluate(snap(60.0, 1024.0, 100.0));
        assert!(result.is_some());
        assert_eq!(result.unwrap().trigger, UpdateTrigger::Cpu);
    }

    #[test]
    fn large_memory_change_triggers() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(50.0, 1000.0, 100.0));

        let result = driver.evaluate(snap(50.0, 1200.0, 100.0));
        assert!(result.is_some());
        assert_eq!(result.unwrap().trigger, UpdateTrigger::Memory);
    }

    #[test]
    fn large_io_change_triggers() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(50.0, 1024.0, 100.0));

        let result = driver.evaluate(snap(50.0, 1024.0, 150.0));
        assert!(result.is_some());
        assert_eq!(result.unwrap().trigger, UpdateTrigger::Io);
    }

    #[test]
    fn multiple_changes_flagged() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(50.0, 1000.0, 100.0));

        let result = driver.evaluate(snap(60.0, 1200.0, 100.0));
        assert!(result.is_some());
        assert_eq!(result.unwrap().trigger, UpdateTrigger::Multiple);
    }

    #[test]
    fn min_interval_enforced() {
        let mut driver = AdaptiveCostDriver::new();
        driver.evaluate(snap(50.0, 1024.0, 100.0));

        // Even a large change should be suppressed by interval
        let result = driver.evaluate(snap(100.0, 2048.0, 200.0));
        assert!(result.is_none());
    }

    #[test]
    fn force_update_bypasses_checks() {
        let mut driver = AdaptiveCostDriver::new();
        driver.evaluate(snap(50.0, 1024.0, 100.0));

        // Force update bypasses interval and threshold
        let record = driver.force_update(snap(51.0, 1024.0, 100.0));
        assert_eq!(record.trigger, UpdateTrigger::Forced);
        assert_eq!(driver.update_count(), 2);
    }

    #[test]
    fn history_tracks_updates() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            max_history: 5,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);

        for i in 0..10 {
            let base = (i + 1) as f64 * 100.0;
            driver.force_update(snap(base, base, base));
        }

        assert_eq!(driver.history().len(), 5);
        assert_eq!(driver.update_count(), 10);
    }

    #[test]
    fn suppression_ratio() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(50.0, 1024.0, 100.0));
        driver.evaluate(snap(51.0, 1024.0, 100.0));
        driver.evaluate(snap(52.0, 1024.0, 100.0));
        driver.evaluate(snap(53.0, 1024.0, 100.0));

        // 1 trigger (first) + 3 suppressed = 3/4 ratio
        assert!((driver.suppression_ratio() - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_clears_state() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(50.0, 1024.0, 100.0));
        driver.reset();

        assert_eq!(driver.update_count(), 0);
        assert_eq!(driver.eval_count(), 0);
        assert!(driver.last_snapshot().is_none());
        assert!(driver.history().is_empty());
    }

    #[test]
    fn zero_to_nonzero_triggers() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(0.0, 0.0, 0.0));

        let result = driver.evaluate(snap(1.0, 0.0, 0.0));
        assert!(result.is_some());
    }

    #[test]
    fn zero_to_zero_suppressed() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(0.0, 0.0, 0.0));

        let result = driver.evaluate(snap(0.0, 0.0, 0.0));
        assert!(result.is_none());
    }

    #[test]
    fn custom_threshold() {
        let config = AdaptiveConfig {
            change_threshold: 0.50,
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        driver.evaluate(snap(100.0, 1024.0, 100.0));

        // 20% change should be suppressed with 50% threshold
        let result = driver.evaluate(snap(120.0, 1024.0, 100.0));
        assert!(result.is_none());

        // 60% change should trigger
        let result = driver.evaluate(snap(160.0, 1024.0, 100.0));
        assert!(result.is_some());
    }

    #[test]
    fn default_config_values() {
        let config = AdaptiveConfig::default();
        assert!((config.change_threshold - 0.10).abs() < f64::EPSILON);
        assert_eq!(config.min_interval, Duration::from_millis(100));
        assert_eq!(config.max_history, 1000);
    }

    #[test]
    fn debug_format() {
        let driver = AdaptiveCostDriver::new();
        let dbg = format!("{driver:?}");
        assert!(dbg.contains("AdaptiveCostDriver"));
    }

    #[test]
    fn default_trait() {
        let driver = AdaptiveCostDriver::default();
        assert_eq!(driver.update_count(), 0);
    }

    #[test]
    fn last_snapshot_updated() {
        let config = AdaptiveConfig {
            min_interval: Duration::ZERO,
            ..AdaptiveConfig::default()
        };
        let mut driver = AdaptiveCostDriver::with_config(config);
        assert!(driver.last_snapshot().is_none());
        driver.evaluate(snap(50.0, 1024.0, 100.0));
        let s = driver.last_snapshot().unwrap();
        assert!((s.cpu - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn exceeds_threshold_fn() {
        assert!(exceeds_threshold(100.0, 115.0, 0.10));
        assert!(!exceeds_threshold(100.0, 105.0, 0.10));
        assert!(exceeds_threshold(0.0, 1.0, 0.10));
        assert!(!exceeds_threshold(0.0, 0.0, 0.10));
        assert!(exceeds_threshold(100.0, 80.0, 0.10));
    }
}
