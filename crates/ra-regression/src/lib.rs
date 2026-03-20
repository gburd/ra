//! Query plan regression detection system for RA optimizer.
//!
//! This crate provides functionality to detect regressions in query plans
//! when statistics or optimizer code changes. It tracks plan fingerprints
//! and costs over time to identify unexpected degradations.

pub mod detector;
pub mod fingerprint;
pub mod history;
pub mod storage;

pub use detector::{RegressionDetector, RegressionReport, RegressionSeverity};
pub use fingerprint::PlanFingerprint;
pub use history::{CostHistory, QueryEntry};
pub use storage::{Storage, StorageError, SqliteStorage, TomlStorage};

#[cfg(test)]
mod tests;

/// Configuration for regression detection thresholds.
#[derive(Debug, Clone)]
pub struct RegressionConfig {
    /// Threshold for warning-level regression (default: 1.25 = 25% increase).
    pub warn_threshold: f64,
    /// Threshold for error-level regression (default: 2.0 = 2x increase).
    pub error_threshold: f64,
    /// Whether to treat plan structure changes as regressions.
    pub detect_plan_changes: bool,
}

impl Default for RegressionConfig {
    fn default() -> Self {
        Self {
            warn_threshold: 1.25,
            error_threshold: 2.0,
            detect_plan_changes: true,
        }
    }
}