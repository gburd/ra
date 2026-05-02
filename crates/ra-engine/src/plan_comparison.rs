//! Plan comparison framework.
//!
//! Provides structured metrics for comparing RA's optimized plans
//! against database-native plans (via EXPLAIN ANALYZE). Used by
//! `ra-cli compare` to produce quantitative reports.

use std::time::Duration;

use ra_core::algebra::RelExpr;

/// Full comparison result between RA and a database plan.
#[derive(Debug, Clone)]
pub struct PlanComparisonResult {
    /// RA's optimized plan.
    pub ra_plan: RelExpr,
    /// Database's native plan text (from EXPLAIN).
    pub db_plan_text: Option<String>,
    /// Comparison metrics.
    pub metrics: ComparisonMetrics,
}

/// Quantitative comparison metrics between RA and a database planner.
#[derive(Debug, Clone, Default)]
pub struct ComparisonMetrics {
    /// Time RA spent parsing SQL to `RelExpr`.
    pub ra_parse_time: Duration,
    /// Time RA spent on equality-saturation optimization.
    pub ra_optimize_time: Duration,
    /// Peak memory used by the e-graph (bytes, estimated).
    pub ra_peak_memory: usize,
    /// Number of e-graph iterations completed.
    pub ra_iterations: u32,
    /// Number of rules considered (after advisor filtering).
    pub ra_rules_considered: u32,
    /// Number of rules that actually fired (matched).
    pub ra_rules_applied: u32,
    /// Database planning time (from EXPLAIN ANALYZE).
    pub db_plan_time: Option<Duration>,
    /// Database execution time (from EXPLAIN ANALYZE with --execute).
    pub db_execution_time: Option<Duration>,
    /// Whether RA and DB produce identical results (`--execute` only).
    pub results_match: Option<bool>,
}

impl ComparisonMetrics {
    /// Format a human-readable summary of the metrics.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("RA parse:     {:?}", self.ra_parse_time));
        lines.push(format!(
            "RA optimize:  {:?} ({} iterations)",
            self.ra_optimize_time, self.ra_iterations,
        ));
        lines.push(format!(
            "RA rules:     {} considered, {} applied",
            self.ra_rules_considered, self.ra_rules_applied,
        ));
        if self.ra_peak_memory > 0 {
            lines.push(format!("RA memory:    {} KB", self.ra_peak_memory / 1024));
        }
        if let Some(dt) = self.db_plan_time {
            lines.push(format!("DB plan time: {dt:?}"));
        }
        if let Some(dt) = self.db_execution_time {
            lines.push(format!("DB exec time: {dt:?}"));
        }
        if let Some(ok) = self.results_match {
            lines.push(format!(
                "Results:      {}",
                if ok { "MATCH" } else { "DIVERGE" }
            ));
        }
        lines.join("\n")
    }

    /// Compute the optimization time ratio (RA / DB).
    ///
    /// Returns `None` if database plan time is unavailable.
    #[must_use]
    pub fn optimization_time_ratio(&self) -> Option<f64> {
        self.db_plan_time.map(|db| {
            let ra = self.ra_optimize_time.as_secs_f64();
            let db_secs = db.as_secs_f64();
            if db_secs > 0.0 {
                ra / db_secs
            } else {
                f64::INFINITY
            }
        })
    }

    /// Compute rule utilization (applied / considered).
    #[must_use]
    pub fn rule_utilization(&self) -> f64 {
        if self.ra_rules_considered == 0 {
            return 0.0;
        }
        f64::from(self.ra_rules_applied) / f64::from(self.ra_rules_considered)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn metrics_summary_format() {
        let m = ComparisonMetrics {
            ra_parse_time: Duration::from_micros(500),
            ra_optimize_time: Duration::from_millis(15),
            ra_iterations: 8,
            ra_rules_considered: 120,
            ra_rules_applied: 45,
            ra_peak_memory: 512 * 1024,
            ..ComparisonMetrics::default()
        };
        let summary = m.summary();
        assert!(summary.contains("RA optimize:"));
        assert!(summary.contains("120 considered"));
        assert!(summary.contains("512 KB"));
    }

    #[test]
    fn rule_utilization_calculation() {
        let m = ComparisonMetrics {
            ra_rules_considered: 200,
            ra_rules_applied: 50,
            ..ComparisonMetrics::default()
        };
        assert!((m.rule_utilization() - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn optimization_time_ratio_none_without_db() {
        let m = ComparisonMetrics::default();
        assert!(m.optimization_time_ratio().is_none());
    }

    #[test]
    fn optimization_time_ratio_with_db() {
        let m = ComparisonMetrics {
            ra_optimize_time: Duration::from_millis(10),
            db_plan_time: Some(Duration::from_millis(5)),
            ..ComparisonMetrics::default()
        };
        let ratio = m.optimization_time_ratio().unwrap();
        assert!((ratio - 2.0).abs() < 0.01);
    }
}
