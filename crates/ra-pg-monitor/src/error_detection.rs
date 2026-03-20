//! Cardinality estimation error detection for the monitoring dashboard.
//!
//! Bridges ra-stats feedback (q-error tracking and recommendations)
//! with the ra-pg-monitor advisory system. Surfaces tables with the
//! worst estimation errors and translates feedback recommendations
//! into dashboard-compatible advice.

use ra_stats::feedback::{
    CardinalityErrorTracker, ErrorSeverity, OperatorKind, RecommendationEngine, RecommendationKind,
};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::recommendations::{Category, Recommendation, Severity};

/// Summary of cardinality estimation errors for a single table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableErrorSummary {
    /// Table name.
    pub table: String,
    /// Number of recorded estimation errors.
    pub error_count: u64,
    /// Average q-error across all observations.
    pub avg_q_error: f64,
    /// Maximum q-error observed.
    pub max_q_error: f64,
    /// Severity classification based on avg q-error.
    pub severity: ErrorSeverity,
}

impl fmt::Display for TableErrorSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} errors, avg q-error={:.1}, max={:.1} [{}]",
            self.table, self.error_count, self.avg_q_error, self.max_q_error, self.severity,
        )
    }
}

/// Monitors cardinality estimation errors and produces
/// recommendations for the dashboard.
pub struct ErrorDetector {
    tracker: CardinalityErrorTracker,
    engine: RecommendationEngine,
}

impl ErrorDetector {
    /// Create a new detector with default recommendation thresholds.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracker: CardinalityErrorTracker::new(),
            engine: RecommendationEngine::new(),
        }
    }

    /// Create a detector with a custom recommendation engine.
    #[must_use]
    pub fn with_engine(engine: RecommendationEngine) -> Self {
        Self {
            tracker: CardinalityErrorTracker::new(),
            engine,
        }
    }

    /// Record a cardinality estimation error observation.
    pub fn record(
        &mut self,
        table: &str,
        operator: OperatorKind,
        estimated: f64,
        actual: f64,
        context: Option<String>,
    ) {
        self.tracker
            .record(table, operator, estimated, actual, context);
    }

    /// Get a reference to the underlying tracker.
    #[must_use]
    pub fn tracker(&self) -> &CardinalityErrorTracker {
        &self.tracker
    }

    /// Per-table error summaries, worst first.
    #[must_use]
    pub fn table_summaries(&self) -> Vec<TableErrorSummary> {
        let worst = self.tracker.worst_tables(usize::MAX);
        let mut summaries: Vec<TableErrorSummary> = worst
            .into_iter()
            .map(|(table, avg_q)| {
                let max_q = self
                    .tracker
                    .errors()
                    .iter()
                    .filter(|e| e.table == table)
                    .map(|e| e.q_error)
                    .fold(0.0_f64, f64::max);
                let count = self
                    .tracker
                    .errors()
                    .iter()
                    .filter(|e| e.table == table)
                    .count() as u64;
                TableErrorSummary {
                    table,
                    error_count: count,
                    avg_q_error: avg_q,
                    max_q_error: max_q,
                    severity: ra_stats::feedback::classify_error(avg_q),
                }
            })
            .collect();
        summaries.sort_by(|a, b| {
            b.avg_q_error
                .partial_cmp(&a.avg_q_error)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        summaries
    }

    /// Tables that need ANALYZE (high q-error).
    #[must_use]
    pub fn tables_needing_analyze(&self) -> Vec<String> {
        self.engine
            .recommend(&self.tracker)
            .into_iter()
            .filter(|r| r.kind == RecommendationKind::Analyze)
            .map(|r| r.table)
            .collect()
    }

    /// Generate dashboard recommendations from error analysis.
    #[must_use]
    pub fn recommendations(&self) -> Vec<Recommendation> {
        self.engine
            .recommend(&self.tracker)
            .into_iter()
            .map(|r| {
                let severity = match r.severity {
                    ErrorSeverity::Low => Severity::Info,
                    ErrorSeverity::Medium => Severity::Warning,
                    ErrorSeverity::High => Severity::Error,
                };
                Recommendation {
                    severity,
                    category: Category::Statistics,
                    target: r.table,
                    message: r.message,
                    suggestion: r.suggestion,
                }
            })
            .collect()
    }

    /// Clear all recorded data.
    pub fn clear(&mut self) {
        self.tracker.clear();
    }
}

impl Default for ErrorDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detector_new_empty() {
        let d = ErrorDetector::new();
        assert!(d.tracker().is_empty());
        assert!(d.table_summaries().is_empty());
        assert!(d.recommendations().is_empty());
    }

    #[test]
    fn detector_default_empty() {
        let d = ErrorDetector::default();
        assert!(d.tracker().is_empty());
    }

    #[test]
    fn detector_record_and_summarize() {
        let mut d = ErrorDetector::new();
        d.record("orders", OperatorKind::Scan, 100.0, 2000.0, None);
        let summaries = d.table_summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].table, "orders");
        assert!((summaries[0].avg_q_error - 20.0).abs() < f64::EPSILON);
        assert_eq!(summaries[0].error_count, 1);
        assert_eq!(summaries[0].severity, ErrorSeverity::High);
    }

    #[test]
    fn detector_multiple_tables_sorted() {
        let mut d = ErrorDetector::new();
        d.record("a", OperatorKind::Scan, 100.0, 200.0, None); // q=2
        d.record("b", OperatorKind::Scan, 100.0, 5000.0, None); // q=50
        let summaries = d.table_summaries();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].table, "b");
    }

    #[test]
    fn detector_tables_needing_analyze() {
        let mut d = ErrorDetector::new();
        d.record("bad", OperatorKind::Scan, 100.0, 5000.0, None);
        d.record("ok", OperatorKind::Scan, 100.0, 110.0, None);
        let need_analyze = d.tables_needing_analyze();
        assert_eq!(need_analyze.len(), 1);
        assert_eq!(need_analyze[0], "bad");
    }

    #[test]
    fn detector_recommendations_severity_mapping() {
        let mut d = ErrorDetector::new();
        d.record("orders", OperatorKind::Scan, 100.0, 5000.0, None);
        let recs = d.recommendations();
        assert!(!recs.is_empty());
        // High error should map to Severity::Error
        let analyze_rec = recs
            .iter()
            .find(|r| r.suggestion.contains("ANALYZE"))
            .expect("should have ANALYZE rec");
        assert_eq!(analyze_rec.severity, Severity::Error);
        assert_eq!(analyze_rec.category, Category::Statistics);
    }

    #[test]
    fn detector_recommendations_for_joins() {
        let mut d = ErrorDetector::new();
        d.record(
            "orders",
            OperatorKind::Join,
            100.0,
            800.0,
            Some("orders.id = items.order_id".to_string()),
        );
        let recs = d.recommendations();
        let ext_rec = recs.iter().find(|r| r.suggestion.contains("STATISTICS"));
        assert!(ext_rec.is_some());
    }

    #[test]
    fn detector_clear() {
        let mut d = ErrorDetector::new();
        d.record("a", OperatorKind::Scan, 100.0, 500.0, None);
        assert!(!d.tracker().is_empty());
        d.clear();
        assert!(d.tracker().is_empty());
        assert!(d.table_summaries().is_empty());
    }

    #[test]
    fn detector_with_custom_engine() {
        let engine = RecommendationEngine::with_thresholds(5.0, 3.0, 3.0, 2.0);
        let mut d = ErrorDetector::with_engine(engine);
        // q=8, above custom analyze threshold of 5
        d.record("orders", OperatorKind::Scan, 100.0, 800.0, None);
        let need_analyze = d.tables_needing_analyze();
        assert_eq!(need_analyze.len(), 1);
    }

    #[test]
    fn table_error_summary_display() {
        let summary = TableErrorSummary {
            table: "orders".to_string(),
            error_count: 5,
            avg_q_error: 15.0,
            max_q_error: 50.0,
            severity: ErrorSeverity::High,
        };
        let display = summary.to_string();
        assert!(display.contains("orders"));
        assert!(display.contains("5 errors"));
        assert!(display.contains("15.0"));
        assert!(display.contains("High"));
    }
}
