//! Regression detection logic.

use crate::{
    fingerprint::PlanFingerprint,
    history::CostHistory,
    RegressionConfig,
};
use std::fmt;

/// Severity levels for detected regressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegressionSeverity {
    /// No regression detected.
    None,
    /// Minor regression or improvement.
    Info,
    /// Cost increased beyond warning threshold.
    Warning,
    /// Cost increased beyond error threshold.
    Error,
}

impl fmt::Display for RegressionSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "NONE"),
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// Report of a detected regression.
#[derive(Debug, Clone)]
pub struct RegressionReport {
    /// Query identifier.
    pub query_id: String,
    /// Severity of the regression.
    pub severity: RegressionSeverity,
    /// Current cost.
    pub current_cost: f64,
    /// Baseline cost (if available).
    pub baseline_cost: Option<f64>,
    /// Historical average cost (if available).
    pub historical_avg: Option<f64>,
    /// Cost increase ratio (current/baseline).
    pub cost_ratio: Option<f64>,
    /// Whether the plan structure changed.
    pub plan_changed: bool,
    /// Description of the regression.
    pub description: String,
}

impl RegressionReport {
    /// Check if this represents an actual regression (not None or Info).
    pub fn is_regression(&self) -> bool {
        matches!(self.severity, RegressionSeverity::Warning | RegressionSeverity::Error)
    }

    /// Check if this represents an improvement.
    pub fn is_improvement(&self) -> bool {
        self.cost_ratio.map_or(false, |r| r < 0.9)
    }
}

impl fmt::Display for RegressionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] Query {}: ", self.severity, self.query_id)?;
        write!(f, "{}", self.description)?;

        if let Some(baseline) = self.baseline_cost {
            write!(f, " (baseline: {:.2}, current: {:.2}", baseline, self.current_cost)?;
            if let Some(ratio) = self.cost_ratio {
                write!(f, ", ratio: {:.2}x", ratio)?;
            }
            write!(f, ")")?;
        }

        if self.plan_changed {
            write!(f, " [PLAN CHANGED]")?;
        }

        Ok(())
    }
}

/// Detector for query plan regressions.
pub struct RegressionDetector {
    config: RegressionConfig,
}

impl RegressionDetector {
    /// Create a new regression detector with default configuration.
    pub fn new() -> Self {
        Self {
            config: RegressionConfig::default(),
        }
    }

    /// Create a new regression detector with custom configuration.
    pub fn with_config(config: RegressionConfig) -> Self {
        Self { config }
    }

    /// Detect regression for a query.
    pub fn detect(
        &self,
        query_id: &str,
        current_cost: f64,
        current_fingerprint: &PlanFingerprint,
        history: &CostHistory,
    ) -> RegressionReport {
        // Get historical data
        let baseline = history.get_baseline(query_id);
        let historical_avg = history.get_average_cost(query_id, 10);

        // Check for plan changes
        let plan_changed = baseline.map_or(false, |b| {
            b.plan_hash != current_fingerprint.as_str()
        });

        // Calculate cost ratio
        let (baseline_cost, cost_ratio) = if let Some(baseline) = baseline {
            let ratio = current_cost / baseline.cost;
            (Some(baseline.cost), Some(ratio))
        } else {
            (None, None)
        };

        // Determine severity and description
        let (severity, description) = self.analyze_regression(
            current_cost,
            baseline_cost,
            historical_avg,
            cost_ratio,
            plan_changed,
        );

        RegressionReport {
            query_id: query_id.to_string(),
            severity,
            current_cost,
            baseline_cost,
            historical_avg,
            cost_ratio,
            plan_changed,
            description,
        }
    }

    /// Compare current cost against historical cost.
    pub fn compare(
        &self,
        current_cost: f64,
        historical_cost: f64,
    ) -> RegressionReport {
        let cost_ratio = current_cost / historical_cost;

        let (severity, description) = if cost_ratio >= self.config.error_threshold {
            (
                RegressionSeverity::Error,
                format!("Cost increased by {:.1}x", cost_ratio),
            )
        } else if cost_ratio >= self.config.warn_threshold {
            (
                RegressionSeverity::Warning,
                format!("Cost increased by {:.1}x", cost_ratio),
            )
        } else if cost_ratio < 0.9 {
            (
                RegressionSeverity::Info,
                format!("Cost improved by {:.1}%", (1.0 - cost_ratio) * 100.0),
            )
        } else {
            (
                RegressionSeverity::None,
                "No significant change".to_string(),
            )
        };

        RegressionReport {
            query_id: String::new(),
            severity,
            current_cost,
            baseline_cost: Some(historical_cost),
            historical_avg: None,
            cost_ratio: Some(cost_ratio),
            plan_changed: false,
            description,
        }
    }

    /// Analyze regression and determine severity.
    fn analyze_regression(
        &self,
        current_cost: f64,
        baseline_cost: Option<f64>,
        historical_avg: Option<f64>,
        cost_ratio: Option<f64>,
        plan_changed: bool,
    ) -> (RegressionSeverity, String) {
        // Check for plan changes first
        if plan_changed && self.config.detect_plan_changes {
            if let Some(ratio) = cost_ratio {
                if ratio >= self.config.error_threshold {
                    return (
                        RegressionSeverity::Error,
                        format!("Plan changed with {:.1}x cost increase", ratio),
                    );
                } else if ratio >= self.config.warn_threshold {
                    return (
                        RegressionSeverity::Warning,
                        format!("Plan changed with {:.1}x cost increase", ratio),
                    );
                } else if ratio < 0.9 {
                    return (
                        RegressionSeverity::Info,
                        format!("Plan changed with {:.1}% cost improvement", (1.0 - ratio) * 100.0),
                    );
                } else {
                    return (
                        RegressionSeverity::Info,
                        "Plan changed with similar cost".to_string(),
                    );
                }
            } else {
                return (
                    RegressionSeverity::Info,
                    "Plan changed (no baseline for comparison)".to_string(),
                );
            }
        }

        // Check cost ratio against thresholds
        if let Some(ratio) = cost_ratio {
            if ratio >= self.config.error_threshold {
                return (
                    RegressionSeverity::Error,
                    format!("Cost regression: {:.1}x increase", ratio),
                );
            } else if ratio >= self.config.warn_threshold {
                return (
                    RegressionSeverity::Warning,
                    format!("Cost regression: {:.1}x increase", ratio),
                );
            } else if ratio < 0.9 {
                return (
                    RegressionSeverity::Info,
                    format!("Cost improvement: {:.1}% reduction", (1.0 - ratio) * 100.0),
                );
            }
        }

        // Check against historical average
        if let Some(avg) = historical_avg {
            let avg_ratio = current_cost / avg;
            if avg_ratio >= 1.5 {
                return (
                    RegressionSeverity::Warning,
                    format!("Cost {:.1}x above historical average", avg_ratio),
                );
            }
        }

        // No regression detected
        if baseline_cost.is_none() {
            (
                RegressionSeverity::None,
                "No baseline for comparison".to_string(),
            )
        } else {
            (
                RegressionSeverity::None,
                "No regression detected".to_string(),
            )
        }
    }
}

impl Default for RegressionDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::QueryEntry;

    #[test]
    fn test_detect_error_regression() {
        let detector = RegressionDetector::new();
        let report = detector.compare(200.0, 100.0);

        assert_eq!(report.severity, RegressionSeverity::Error);
        assert!(report.is_regression());
        assert_eq!(report.cost_ratio, Some(2.0));
    }

    #[test]
    fn test_detect_warning_regression() {
        let detector = RegressionDetector::new();
        let report = detector.compare(130.0, 100.0);

        assert_eq!(report.severity, RegressionSeverity::Warning);
        assert!(report.is_regression());
        assert_eq!(report.cost_ratio, Some(1.3));
    }

    #[test]
    fn test_detect_improvement() {
        let detector = RegressionDetector::new();
        let report = detector.compare(80.0, 100.0);

        assert_eq!(report.severity, RegressionSeverity::Info);
        assert!(!report.is_regression());
        assert!(report.is_improvement());
        assert_eq!(report.cost_ratio, Some(0.8));
    }

    #[test]
    fn test_detect_no_change() {
        let detector = RegressionDetector::new();
        let report = detector.compare(105.0, 100.0);

        assert_eq!(report.severity, RegressionSeverity::None);
        assert!(!report.is_regression());
        assert!(!report.is_improvement());
    }

    #[test]
    fn test_detect_with_history() {
        let detector = RegressionDetector::new();
        let mut history = CostHistory::new();

        history.add_entry(QueryEntry::new(
            "q1".to_string(),
            "SELECT * FROM t".to_string(),
            "hash1".to_string(),
            100.0,
        ));

        let fingerprint = PlanFingerprint::from_plan(
            &datafusion::prelude::SessionContext::new()
                .sql("SELECT * FROM t")
                .await
                .unwrap()
                .into_unoptimized_plan(),
        );

        let report = detector.detect("q1", 250.0, &fingerprint, &history);

        assert_eq!(report.severity, RegressionSeverity::Error);
        assert_eq!(report.baseline_cost, Some(100.0));
        assert_eq!(report.cost_ratio, Some(2.5));
    }
}