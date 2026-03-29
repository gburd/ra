//! Cardinality estimation error detection and feedback.
//!
//! Detects when the optimizer's cardinality estimates diverge from
//! actual row counts observed during execution. Uses q-error
//! (max(actual/estimated, estimated/actual)) as the standard metric.
//!
//! High q-errors indicate stale statistics, missing correlations,
//! or inadequate estimation techniques. The recommendations engine
//! maps detected errors to actionable fixes (ANALYZE, extended
//! statistics, missing indexes).

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Operator types tracked for cardinality errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperatorKind {
    /// Table/index scan.
    Scan,
    /// Join (any algorithm).
    Join,
    /// Aggregation (GROUP BY, DISTINCT).
    Aggregate,
    /// Filter/selection.
    Filter,
    /// Sort.
    Sort,
    /// Other operator.
    Other,
}

impl fmt::Display for OperatorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan => write!(f, "Scan"),
            Self::Join => write!(f, "Join"),
            Self::Aggregate => write!(f, "Aggregate"),
            Self::Filter => write!(f, "Filter"),
            Self::Sort => write!(f, "Sort"),
            Self::Other => write!(f, "Other"),
        }
    }
}

/// Error severity classification based on q-error magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// q-error < 2: acceptable estimation.
    Low,
    /// 2 <= q-error < 10: correlation or skew issues.
    Medium,
    /// q-error >= 10: likely stale statistics.
    High,
}

impl fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Medium => write!(f, "Medium"),
            Self::High => write!(f, "High"),
        }
    }
}

/// A single cardinality estimation error observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardinalityError {
    /// Table name involved (if applicable).
    pub table: String,
    /// Operator that produced the error.
    pub operator: OperatorKind,
    /// Optimizer's estimated row count.
    pub estimated: f64,
    /// Actual row count observed during execution.
    pub actual: f64,
    /// Computed q-error: max(actual/estimated, estimated/actual).
    pub q_error: f64,
    /// Severity classification derived from q-error.
    pub severity: ErrorSeverity,
    /// Optional description of the context (e.g., join columns).
    pub context: Option<String>,
}

impl fmt::Display for CardinalityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}.{}: q-error={:.1} \
             (estimated={:.0}, actual={:.0})",
            self.severity, self.table, self.operator, self.q_error, self.estimated, self.actual,
        )
    }
}

/// Compute q-error between estimated and actual cardinalities.
///
/// q-error = max(actual/estimated, estimated/actual).
/// Both values are clamped to a minimum of 1.0 to avoid
/// division by zero. A q-error of 1.0 means perfect estimation.
pub fn q_error(estimated: f64, actual: f64) -> f64 {
    let est = estimated.max(1.0);
    let act = actual.max(1.0);
    (act / est).max(est / act)
}

/// Classify a q-error value into a severity level.
pub fn classify_error(q: f64) -> ErrorSeverity {
    if q >= 10.0 {
        ErrorSeverity::High
    } else if q >= 2.0 {
        ErrorSeverity::Medium
    } else {
        ErrorSeverity::Low
    }
}

/// Tracks cardinality estimation errors across query executions.
///
/// Collects per-operator observations, computes aggregate statistics,
/// and identifies systematic patterns (specific tables or operator
/// types with consistently high errors).
#[derive(Debug, Clone)]
pub struct CardinalityErrorTracker {
    errors: Vec<CardinalityError>,
    /// Per-table aggregate: (sum of q-errors, count).
    table_agg: HashMap<String, (f64, u64)>,
    /// Per-operator aggregate: (sum of q-errors, count).
    operator_agg: HashMap<OperatorKind, (f64, u64)>,
}

impl CardinalityErrorTracker {
    /// Create a new empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            table_agg: HashMap::new(),
            operator_agg: HashMap::new(),
        }
    }

    /// Record a cardinality estimation error.
    pub fn record(
        &mut self,
        table: &str,
        operator: OperatorKind,
        estimated: f64,
        actual: f64,
        context: Option<String>,
    ) {
        let q = q_error(estimated, actual);
        let severity = classify_error(q);

        self.errors.push(CardinalityError {
            table: table.to_string(),
            operator,
            estimated,
            actual,
            q_error: q,
            severity,
            context,
        });

        let table_entry = self.table_agg.entry(table.to_string()).or_insert((0.0, 0));
        table_entry.0 += q;
        table_entry.1 += 1;

        let op_entry = self.operator_agg.entry(operator).or_insert((0.0, 0));
        op_entry.0 += q;
        op_entry.1 += 1;
    }

    /// All recorded errors.
    #[must_use]
    pub fn errors(&self) -> &[CardinalityError] {
        &self.errors
    }

    /// Number of recorded errors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Whether no errors have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Errors filtered by minimum severity.
    #[must_use]
    pub fn errors_at_least(&self, min_severity: ErrorSeverity) -> Vec<&CardinalityError> {
        self.errors
            .iter()
            .filter(|e| e.severity >= min_severity)
            .collect()
    }

    /// High-severity errors only (q-error >= 10).
    #[must_use]
    pub fn high_errors(&self) -> Vec<&CardinalityError> {
        self.errors_at_least(ErrorSeverity::High)
    }

    /// Average q-error for a specific table.
    #[must_use]
    pub fn table_avg_q_error(&self, table: &str) -> Option<f64> {
        self.table_agg
            .get(table)
            .map(|(sum, count)| sum / *count as f64)
    }

    /// Average q-error for a specific operator kind.
    #[must_use]
    pub fn operator_avg_q_error(&self, op: OperatorKind) -> Option<f64> {
        self.operator_agg
            .get(&op)
            .map(|(sum, count)| sum / *count as f64)
    }

    /// Tables ranked by average q-error (worst first).
    #[must_use]
    pub fn worst_tables(&self, limit: usize) -> Vec<(String, f64)> {
        let mut ranked: Vec<(String, f64)> = self
            .table_agg
            .iter()
            .map(|(table, (sum, count))| (table.clone(), sum / *count as f64))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);
        ranked
    }

    /// Operator types ranked by average q-error (worst first).
    #[must_use]
    pub fn worst_operators(&self, limit: usize) -> Vec<(OperatorKind, f64)> {
        let mut ranked: Vec<(OperatorKind, f64)> = self
            .operator_agg
            .iter()
            .map(|(op, (sum, count))| (*op, sum / *count as f64))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);
        ranked
    }

    /// Maximum q-error observed across all recordings.
    #[must_use]
    pub fn max_q_error(&self) -> f64 {
        self.errors
            .iter()
            .map(|e| e.q_error)
            .fold(0.0_f64, f64::max)
    }

    /// Mean q-error across all recordings.
    #[must_use]
    pub fn mean_q_error(&self) -> f64 {
        if self.errors.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.errors.iter().map(|e| e.q_error).sum();
        sum / self.errors.len() as f64
    }

    /// Median q-error across all recordings.
    #[must_use]
    pub fn median_q_error(&self) -> f64 {
        if self.errors.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.errors.iter().map(|e| e.q_error).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    }

    /// Clear all recorded errors.
    pub fn clear(&mut self) {
        self.errors.clear();
        self.table_agg.clear();
        self.operator_agg.clear();
    }
}

impl Default for CardinalityErrorTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Type of recommendation generated from error analysis.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecommendationKind {
    /// Run ANALYZE on a table to refresh statistics.
    Analyze,
    /// Create extended statistics for correlated columns.
    ExtendedStatistics,
    /// Create a missing index.
    CreateIndex,
    /// Create or update a histogram on a skewed column.
    Histogram,
}

impl fmt::Display for RecommendationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Analyze => write!(f, "ANALYZE"),
            Self::ExtendedStatistics => {
                write!(f, "Extended Statistics")
            }
            Self::CreateIndex => write!(f, "Create Index"),
            Self::Histogram => write!(f, "Histogram"),
        }
    }
}

/// A recommendation generated from cardinality error analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorRecommendation {
    /// What kind of action is recommended.
    pub kind: RecommendationKind,
    /// Table the recommendation applies to.
    pub table: String,
    /// Severity based on the underlying q-errors.
    pub severity: ErrorSeverity,
    /// Human-readable description.
    pub message: String,
    /// Suggested SQL or action.
    pub suggestion: String,
    /// Average q-error that triggered this recommendation.
    pub avg_q_error: f64,
}

impl fmt::Display for ErrorRecommendation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {} (avg q-error: {:.1})",
            self.severity, self.kind, self.message, self.avg_q_error,
        )
    }
}

/// Generates actionable recommendations from cardinality errors.
///
/// Maps error patterns to specific fixes:
/// - High scan errors -> ANALYZE the table
/// - High join errors -> extended statistics for join columns
/// - High filter errors with no index -> suggest index creation
/// - Medium scan errors -> histogram for skewed columns
#[derive(Debug, Clone)]
pub struct RecommendationEngine {
    /// q-error threshold for suggesting ANALYZE (default: 10.0).
    analyze_threshold: f64,
    /// q-error threshold for extended stats (default: 5.0).
    extended_stats_threshold: f64,
    /// q-error threshold for index suggestion (default: 5.0).
    index_threshold: f64,
    /// q-error threshold for histogram suggestion (default: 3.0).
    histogram_threshold: f64,
}

impl RecommendationEngine {
    /// Create an engine with default thresholds.
    #[must_use]
    pub fn new() -> Self {
        Self {
            analyze_threshold: 10.0,
            extended_stats_threshold: 5.0,
            index_threshold: 5.0,
            histogram_threshold: 3.0,
        }
    }

    /// Create an engine with custom thresholds.
    #[must_use]
    pub fn with_thresholds(analyze: f64, extended_stats: f64, index: f64, histogram: f64) -> Self {
        Self {
            analyze_threshold: analyze,
            extended_stats_threshold: extended_stats,
            index_threshold: index,
            histogram_threshold: histogram,
        }
    }

    /// Generate recommendations from a tracker's collected errors.
    #[must_use]
    pub fn recommend(&self, tracker: &CardinalityErrorTracker) -> Vec<ErrorRecommendation> {
        let mut recs = Vec::new();

        self.check_analyze_needed(tracker, &mut recs);
        self.check_extended_stats(tracker, &mut recs);
        self.check_missing_indexes(tracker, &mut recs);
        self.check_histograms(tracker, &mut recs);

        recs.sort_by(|a, b| {
            b.severity.cmp(&a.severity).then_with(|| {
                b.avg_q_error
                    .partial_cmp(&a.avg_q_error)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        recs
    }

    fn check_analyze_needed(
        &self,
        tracker: &CardinalityErrorTracker,
        recs: &mut Vec<ErrorRecommendation>,
    ) {
        for (table, (sum, count)) in &tracker.table_agg {
            let avg = sum / *count as f64;
            if avg >= self.analyze_threshold {
                recs.push(ErrorRecommendation {
                    kind: RecommendationKind::Analyze,
                    table: table.clone(),
                    severity: ErrorSeverity::High,
                    message: format!(
                        "Table '{}' has avg q-error {:.1}, \
                         statistics are likely stale",
                        table, avg,
                    ),
                    suggestion: format!("ANALYZE {};", table),
                    avg_q_error: avg,
                });
            }
        }
    }

    fn check_extended_stats(
        &self,
        tracker: &CardinalityErrorTracker,
        recs: &mut Vec<ErrorRecommendation>,
    ) {
        let mut join_tables: HashMap<String, (f64, u64)> = HashMap::new();
        for error in &tracker.errors {
            if error.operator == OperatorKind::Join
                && error.q_error >= self.extended_stats_threshold
            {
                let entry = join_tables.entry(error.table.clone()).or_insert((0.0, 0));
                entry.0 += error.q_error;
                entry.1 += 1;
            }
        }

        for (table, (sum, count)) in &join_tables {
            let avg = sum / *count as f64;
            recs.push(ErrorRecommendation {
                kind: RecommendationKind::ExtendedStatistics,
                table: table.clone(),
                severity: classify_error(avg),
                message: format!(
                    "Table '{}' has {} join estimation errors \
                     (avg q-error {:.1}), consider extended \
                     statistics for correlated join columns",
                    table, count, avg,
                ),
                suggestion: format!(
                    "CREATE STATISTICS ON <correlated_columns> \
                     FROM {};",
                    table,
                ),
                avg_q_error: avg,
            });
        }
    }

    fn check_missing_indexes(
        &self,
        tracker: &CardinalityErrorTracker,
        recs: &mut Vec<ErrorRecommendation>,
    ) {
        let mut filter_tables: HashMap<String, (f64, u64)> = HashMap::new();
        for error in &tracker.errors {
            if error.operator == OperatorKind::Filter && error.q_error >= self.index_threshold {
                let entry = filter_tables.entry(error.table.clone()).or_insert((0.0, 0));
                entry.0 += error.q_error;
                entry.1 += 1;
            }
        }

        for (table, (sum, count)) in &filter_tables {
            let avg = sum / *count as f64;
            recs.push(ErrorRecommendation {
                kind: RecommendationKind::CreateIndex,
                table: table.clone(),
                severity: classify_error(avg),
                message: format!(
                    "Table '{}' has {} filter estimation errors \
                     (avg q-error {:.1}), a missing index may \
                     cause poor selectivity estimates",
                    table, count, avg,
                ),
                suggestion: format!("CREATE INDEX ON {} (<filter_columns>);", table,),
                avg_q_error: avg,
            });
        }
    }

    fn check_histograms(
        &self,
        tracker: &CardinalityErrorTracker,
        recs: &mut Vec<ErrorRecommendation>,
    ) {
        let mut scan_tables: HashMap<String, (f64, u64)> = HashMap::new();
        for error in &tracker.errors {
            if error.operator == OperatorKind::Scan
                && error.q_error >= self.histogram_threshold
                && error.q_error < self.analyze_threshold
            {
                let entry = scan_tables.entry(error.table.clone()).or_insert((0.0, 0));
                entry.0 += error.q_error;
                entry.1 += 1;
            }
        }

        for (table, (sum, count)) in &scan_tables {
            let avg = sum / *count as f64;
            recs.push(ErrorRecommendation {
                kind: RecommendationKind::Histogram,
                table: table.clone(),
                severity: classify_error(avg),
                message: format!(
                    "Table '{}' has {} moderate scan errors \
                     (avg q-error {:.1}), histograms may improve \
                     estimation for skewed columns",
                    table, count, avg,
                ),
                suggestion: format!(
                    "ALTER TABLE {} ALTER COLUMN <col> \
                     SET STATISTICS 1000;",
                    table,
                ),
                avg_q_error: avg,
            });
        }
    }
}

impl Default for RecommendationEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    // -- q_error --

    #[test]
    fn q_error_perfect_estimate() {
        assert!((q_error(100.0, 100.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_overestimate() {
        // estimated=1000, actual=100 -> 1000/100 = 10
        assert!((q_error(1000.0, 100.0) - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_underestimate() {
        // estimated=100, actual=1000 -> 1000/100 = 10
        assert!((q_error(100.0, 1000.0) - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_symmetric() {
        let q1 = q_error(50.0, 200.0);
        let q2 = q_error(200.0, 50.0);
        assert!((q1 - q2).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_zero_estimated_clamped() {
        // 0.0 is clamped to 1.0, so q_error(0, 100) = 100/1 = 100
        let q = q_error(0.0, 100.0);
        assert!((q - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_zero_actual_clamped() {
        let q = q_error(100.0, 0.0);
        assert!((q - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_both_zero() {
        assert!((q_error(0.0, 0.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_small_values() {
        // estimated=1, actual=5 -> 5/1 = 5
        assert!((q_error(1.0, 5.0) - 5.0).abs() < f64::EPSILON);
    }

    // -- classify_error --

    #[test]
    fn classify_low() {
        assert_eq!(classify_error(1.0), ErrorSeverity::Low);
        assert_eq!(classify_error(1.5), ErrorSeverity::Low);
        assert_eq!(classify_error(1.99), ErrorSeverity::Low);
    }

    #[test]
    fn classify_medium() {
        assert_eq!(classify_error(2.0), ErrorSeverity::Medium);
        assert_eq!(classify_error(5.0), ErrorSeverity::Medium);
        assert_eq!(classify_error(9.99), ErrorSeverity::Medium);
    }

    #[test]
    fn classify_high() {
        assert_eq!(classify_error(10.0), ErrorSeverity::High);
        assert_eq!(classify_error(100.0), ErrorSeverity::High);
    }

    // -- ErrorSeverity ordering --

    #[test]
    fn severity_ordering() {
        assert!(ErrorSeverity::High > ErrorSeverity::Medium);
        assert!(ErrorSeverity::Medium > ErrorSeverity::Low);
    }

    // -- OperatorKind display --

    #[test]
    fn operator_display() {
        assert_eq!(OperatorKind::Scan.to_string(), "Scan");
        assert_eq!(OperatorKind::Join.to_string(), "Join");
        assert_eq!(OperatorKind::Aggregate.to_string(), "Aggregate");
        assert_eq!(OperatorKind::Filter.to_string(), "Filter");
        assert_eq!(OperatorKind::Sort.to_string(), "Sort");
        assert_eq!(OperatorKind::Other.to_string(), "Other");
    }

    // -- CardinalityError display --

    #[test]
    fn cardinality_error_display() {
        let err = CardinalityError {
            table: "orders".to_string(),
            operator: OperatorKind::Scan,
            estimated: 100.0,
            actual: 1000.0,
            q_error: 10.0,
            severity: ErrorSeverity::High,
            context: None,
        };
        let display = err.to_string();
        assert!(display.contains("High"));
        assert!(display.contains("orders"));
        assert!(display.contains("Scan"));
        assert!(display.contains("10.0"));
    }

    // -- CardinalityErrorTracker --

    #[test]
    fn tracker_new_empty() {
        let t = CardinalityErrorTracker::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn tracker_default_empty() {
        let t = CardinalityErrorTracker::default();
        assert!(t.is_empty());
    }

    #[test]
    fn tracker_record_single() {
        let mut t = CardinalityErrorTracker::new();
        t.record("orders", OperatorKind::Scan, 100.0, 500.0, None);
        assert_eq!(t.len(), 1);
        assert!(!t.is_empty());
        let err = &t.errors()[0];
        assert_eq!(err.table, "orders");
        assert_eq!(err.operator, OperatorKind::Scan);
        assert!((err.q_error - 5.0).abs() < f64::EPSILON);
        assert_eq!(err.severity, ErrorSeverity::Medium);
    }

    #[test]
    fn tracker_record_with_context() {
        let mut t = CardinalityErrorTracker::new();
        t.record(
            "orders",
            OperatorKind::Join,
            100.0,
            1000.0,
            Some("orders.id = items.order_id".to_string()),
        );
        let err = &t.errors()[0];
        assert_eq!(err.context.as_deref(), Some("orders.id = items.order_id"),);
    }

    #[test]
    fn tracker_record_multiple() {
        let mut t = CardinalityErrorTracker::new();
        t.record("orders", OperatorKind::Scan, 100.0, 100.0, None);
        t.record("orders", OperatorKind::Scan, 100.0, 500.0, None);
        t.record("items", OperatorKind::Join, 1000.0, 100.0, None);
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn tracker_errors_at_least_medium() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 100.0, None); // q=1
        t.record("b", OperatorKind::Scan, 100.0, 500.0, None); // q=5
        t.record("c", OperatorKind::Scan, 100.0, 2000.0, None); // q=20
        let medium_plus = t.errors_at_least(ErrorSeverity::Medium);
        assert_eq!(medium_plus.len(), 2);
    }

    #[test]
    fn tracker_high_errors() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 500.0, None); // q=5
        t.record("b", OperatorKind::Scan, 100.0, 2000.0, None); // q=20
        let high = t.high_errors();
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].table, "b");
    }

    #[test]
    fn tracker_table_avg_q_error() {
        let mut t = CardinalityErrorTracker::new();
        // q=5 and q=10 for "orders" -> avg=7.5
        t.record("orders", OperatorKind::Scan, 100.0, 500.0, None);
        t.record("orders", OperatorKind::Scan, 100.0, 1000.0, None);
        let avg = t.table_avg_q_error("orders");
        assert!(avg.is_some());
        assert!((avg.unwrap_or(0.0) - 7.5).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_table_avg_q_error_missing() {
        let t = CardinalityErrorTracker::new();
        assert!(t.table_avg_q_error("nonexistent").is_none());
    }

    #[test]
    fn tracker_operator_avg_q_error() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Join, 100.0, 500.0, None); // q=5
        t.record("b", OperatorKind::Join, 100.0, 1000.0, None); // q=10
        let avg = t.operator_avg_q_error(OperatorKind::Join);
        assert!((avg.unwrap_or(0.0) - 7.5).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_operator_avg_missing() {
        let t = CardinalityErrorTracker::new();
        assert!(t.operator_avg_q_error(OperatorKind::Sort).is_none());
    }

    #[test]
    fn tracker_worst_tables() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 500.0, None); // q=5
        t.record("b", OperatorKind::Scan, 100.0, 2000.0, None); // q=20
        t.record("c", OperatorKind::Scan, 100.0, 200.0, None); // q=2
        let worst = t.worst_tables(2);
        assert_eq!(worst.len(), 2);
        assert_eq!(worst[0].0, "b");
        assert!((worst[0].1 - 20.0).abs() < f64::EPSILON);
        assert_eq!(worst[1].0, "a");
    }

    #[test]
    fn tracker_worst_tables_empty() {
        let t = CardinalityErrorTracker::new();
        assert!(t.worst_tables(5).is_empty());
    }

    #[test]
    fn tracker_worst_operators() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 200.0, None); // q=2
        t.record("a", OperatorKind::Join, 100.0, 2000.0, None); // q=20
        let worst = t.worst_operators(2);
        assert_eq!(worst.len(), 2);
        assert_eq!(worst[0].0, OperatorKind::Join);
    }

    #[test]
    fn tracker_max_q_error() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 200.0, None);
        t.record("b", OperatorKind::Scan, 100.0, 2000.0, None);
        assert!((t.max_q_error() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_max_q_error_empty() {
        let t = CardinalityErrorTracker::new();
        assert!((t.max_q_error() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_mean_q_error() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 500.0, None); // q=5
        t.record("b", OperatorKind::Scan, 100.0, 1000.0, None); // q=10
        assert!((t.mean_q_error() - 7.5).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_mean_q_error_empty() {
        let t = CardinalityErrorTracker::new();
        assert!((t.mean_q_error() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_median_q_error_odd() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 200.0, None); // q=2
        t.record("b", OperatorKind::Scan, 100.0, 500.0, None); // q=5
        t.record("c", OperatorKind::Scan, 100.0, 2000.0, None); // q=20
        assert!((t.median_q_error() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_median_q_error_even() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 200.0, None); // q=2
        t.record("b", OperatorKind::Scan, 100.0, 500.0, None); // q=5
        let expected = (2.0 + 5.0) / 2.0;
        assert!((t.median_q_error() - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_median_q_error_empty() {
        let t = CardinalityErrorTracker::new();
        assert!((t.median_q_error() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_clear() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 500.0, None);
        t.record("b", OperatorKind::Join, 100.0, 1000.0, None);
        assert_eq!(t.len(), 2);
        t.clear();
        assert!(t.is_empty());
        assert!(t.table_avg_q_error("a").is_none());
        assert!(t.operator_avg_q_error(OperatorKind::Scan).is_none());
    }

    // -- RecommendationEngine --

    #[test]
    fn engine_default_thresholds() {
        let e = RecommendationEngine::new();
        assert!((e.analyze_threshold - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn engine_custom_thresholds() {
        let e = RecommendationEngine::with_thresholds(5.0, 3.0, 3.0, 2.0);
        assert!((e.analyze_threshold - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn engine_no_recs_for_low_errors() {
        let mut t = CardinalityErrorTracker::new();
        t.record("a", OperatorKind::Scan, 100.0, 110.0, None); // q~=1.1
        let e = RecommendationEngine::new();
        let recs = e.recommend(&t);
        assert!(recs.is_empty());
    }

    #[test]
    fn engine_analyze_for_high_scan_error() {
        let mut t = CardinalityErrorTracker::new();
        // q=20, well above analyze threshold of 10
        t.record("orders", OperatorKind::Scan, 100.0, 2000.0, None);
        let e = RecommendationEngine::new();
        let recs = e.recommend(&t);
        let analyze_recs: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == RecommendationKind::Analyze)
            .collect();
        assert_eq!(analyze_recs.len(), 1);
        assert_eq!(analyze_recs[0].table, "orders");
        assert!(analyze_recs[0].suggestion.contains("ANALYZE"));
    }

    #[test]
    fn engine_extended_stats_for_join_errors() {
        let mut t = CardinalityErrorTracker::new();
        // q=8, above extended_stats threshold of 5 but below analyze
        t.record("orders", OperatorKind::Join, 100.0, 800.0, None);
        let e = RecommendationEngine::new();
        let recs = e.recommend(&t);
        let ext_recs: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == RecommendationKind::ExtendedStatistics)
            .collect();
        assert_eq!(ext_recs.len(), 1);
        assert!(ext_recs[0].suggestion.contains("STATISTICS"));
    }

    #[test]
    fn engine_index_for_filter_errors() {
        let mut t = CardinalityErrorTracker::new();
        t.record("users", OperatorKind::Filter, 100.0, 800.0, None);
        let e = RecommendationEngine::new();
        let recs = e.recommend(&t);
        let idx_recs: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == RecommendationKind::CreateIndex)
            .collect();
        assert_eq!(idx_recs.len(), 1);
        assert!(idx_recs[0].suggestion.contains("INDEX"));
    }

    #[test]
    fn engine_histogram_for_moderate_scan_errors() {
        let mut t = CardinalityErrorTracker::new();
        // q=5, above histogram threshold (3) but below analyze (10)
        t.record("orders", OperatorKind::Scan, 100.0, 500.0, None);
        let e = RecommendationEngine::new();
        let recs = e.recommend(&t);
        let hist_recs: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == RecommendationKind::Histogram)
            .collect();
        assert_eq!(hist_recs.len(), 1);
        assert!(hist_recs[0].suggestion.contains("STATISTICS"));
    }

    #[test]
    fn engine_recs_sorted_by_severity() {
        let mut t = CardinalityErrorTracker::new();
        // Low q-error filter
        t.record("a", OperatorKind::Filter, 100.0, 800.0, None);
        // High q-error scan -> ANALYZE
        t.record("b", OperatorKind::Scan, 100.0, 5000.0, None);
        let e = RecommendationEngine::new();
        let recs = e.recommend(&t);
        assert!(recs.len() >= 2);
        // First rec should be highest severity
        assert!(recs[0].severity >= recs[1].severity);
    }

    #[test]
    fn engine_multiple_tables() {
        let mut t = CardinalityErrorTracker::new();
        t.record("orders", OperatorKind::Scan, 100.0, 5000.0, None);
        t.record("users", OperatorKind::Scan, 100.0, 3000.0, None);
        let e = RecommendationEngine::new();
        let recs = e.recommend(&t);
        let analyze_recs: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == RecommendationKind::Analyze)
            .collect();
        assert_eq!(analyze_recs.len(), 2);
    }

    #[test]
    fn engine_empty_tracker() {
        let t = CardinalityErrorTracker::new();
        let e = RecommendationEngine::new();
        assert!(e.recommend(&t).is_empty());
    }

    // -- RecommendationKind display --

    #[test]
    fn recommendation_kind_display() {
        assert_eq!(RecommendationKind::Analyze.to_string(), "ANALYZE",);
        assert_eq!(
            RecommendationKind::ExtendedStatistics.to_string(),
            "Extended Statistics",
        );
        assert_eq!(RecommendationKind::CreateIndex.to_string(), "Create Index",);
        assert_eq!(RecommendationKind::Histogram.to_string(), "Histogram",);
    }

    // -- ErrorRecommendation display --

    #[test]
    fn error_recommendation_display() {
        let rec = ErrorRecommendation {
            kind: RecommendationKind::Analyze,
            table: "orders".to_string(),
            severity: ErrorSeverity::High,
            message: "stale stats".to_string(),
            suggestion: "ANALYZE orders;".to_string(),
            avg_q_error: 15.0,
        };
        let display = rec.to_string();
        assert!(display.contains("High"));
        assert!(display.contains("ANALYZE"));
        assert!(display.contains("15.0"));
    }

    // -- Serialization --

    #[test]
    fn cardinality_error_serialize_roundtrip() {
        let err = CardinalityError {
            table: "orders".to_string(),
            operator: OperatorKind::Scan,
            estimated: 100.0,
            actual: 1000.0,
            q_error: 10.0,
            severity: ErrorSeverity::High,
            context: Some("test".to_string()),
        };
        let json = serde_json::to_string(&err).expect("serialize");
        let d: CardinalityError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, d);
    }

    #[test]
    fn error_recommendation_serialize_roundtrip() {
        let rec = ErrorRecommendation {
            kind: RecommendationKind::Analyze,
            table: "orders".to_string(),
            severity: ErrorSeverity::High,
            message: "stale".to_string(),
            suggestion: "ANALYZE".to_string(),
            avg_q_error: 15.0,
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        let d: ErrorRecommendation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rec, d);
    }

    #[test]
    fn error_severity_display() {
        assert_eq!(ErrorSeverity::Low.to_string(), "Low");
        assert_eq!(ErrorSeverity::Medium.to_string(), "Medium");
        assert_eq!(ErrorSeverity::High.to_string(), "High");
    }
}
