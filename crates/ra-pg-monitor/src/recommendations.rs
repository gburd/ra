//! Unified advice engine that aggregates findings from all analyzers.
//!
//! Collects recommendations from query monitoring, schema analysis,
//! configuration checking, bloat detection, and statistics staleness
//! into a single prioritized list of actionable advice.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::bloat_detector::BloatDetector;
use crate::config_checker::ConfigChecker;
use crate::query_monitor::QueryMonitor;
use crate::schema_analyzer::SchemaAnalyzer;
use crate::stats_staleness::StalenessChecker;

/// Severity level for recommendations.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
    Hash, Serialize, Deserialize,
)]
pub enum Severity {
    /// Informational advice, no immediate action needed.
    Info,
    /// Worth investigating, may affect performance.
    Warning,
    /// Significant issue that should be addressed.
    Error,
    /// Urgent problem actively degrading performance.
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
            Self::Critical => write!(f, "CRIT"),
        }
    }
}

/// Category of a recommendation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
)]
pub enum Category {
    /// Query performance issue.
    Query,
    /// Schema design issue.
    Schema,
    /// Configuration issue.
    Config,
    /// Table or index bloat.
    Bloat,
    /// Statistics freshness.
    Statistics,
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query => write!(f, "Query"),
            Self::Schema => write!(f, "Schema"),
            Self::Config => write!(f, "Config"),
            Self::Bloat => write!(f, "Bloat"),
            Self::Statistics => write!(f, "Statistics"),
        }
    }
}

/// A single actionable recommendation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    /// Severity of this recommendation.
    pub severity: Severity,
    /// Category of the issue.
    pub category: Category,
    /// Target object (table name, index name, config param, etc.).
    pub target: String,
    /// Human-readable description of the issue.
    pub message: String,
    /// Suggested fix or action.
    pub suggestion: String,
}

impl fmt::Display for Recommendation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {} - {} ({})",
            self.severity,
            self.category,
            self.target,
            self.message,
            self.suggestion,
        )
    }
}

/// Aggregates all analyzers and produces a prioritized list of advice.
pub struct Advisor {
    query_monitor: QueryMonitor,
    schema_analyzer: SchemaAnalyzer,
    config_checker: ConfigChecker,
    bloat_detector: BloatDetector,
    staleness_checker: StalenessChecker,
}

impl Advisor {
    /// Create a new advisor with all sub-analyzers.
    #[must_use]
    pub fn new(
        query_monitor: QueryMonitor,
        schema_analyzer: SchemaAnalyzer,
        config_checker: ConfigChecker,
        bloat_detector: BloatDetector,
        staleness_checker: StalenessChecker,
    ) -> Self {
        Self {
            query_monitor,
            schema_analyzer,
            config_checker,
            bloat_detector,
            staleness_checker,
        }
    }

    /// Collect all recommendations, sorted by severity (critical first).
    #[must_use]
    pub fn all_recommendations(&self) -> Vec<Recommendation> {
        let mut recs = Vec::new();

        for record in self.query_monitor.slow_queries() {
            recs.push(Recommendation {
                severity: match record.severity {
                    crate::query_monitor::QuerySeverity::Normal => {
                        continue;
                    }
                    crate::query_monitor::QuerySeverity::Slow => {
                        Severity::Warning
                    }
                    crate::query_monitor::QuerySeverity::VerySlow => {
                        Severity::Error
                    }
                    crate::query_monitor::QuerySeverity::Critical => {
                        Severity::Critical
                    }
                },
                category: Category::Query,
                target: truncate_query(&record.query, 60),
                message: format!(
                    "Query took {:.1}ms (cost: {:.0})",
                    record.duration_ms, record.total_cost,
                ),
                suggestion: record.suggestion.clone(),
            });
        }

        for issue in self.schema_analyzer.issues() {
            recs.push(Recommendation {
                severity: issue.severity(),
                category: Category::Schema,
                target: issue.table.clone(),
                message: issue.message.clone(),
                suggestion: issue.suggestion.clone(),
            });
        }

        for issue in self.config_checker.issues() {
            recs.push(Recommendation {
                severity: issue.severity,
                category: Category::Config,
                target: issue.parameter.clone(),
                message: issue.message.clone(),
                suggestion: issue.suggestion.clone(),
            });
        }

        for info in self.bloat_detector.findings() {
            recs.push(Recommendation {
                severity: match info.severity {
                    crate::bloat_detector::BloatSeverity::Low => {
                        Severity::Info
                    }
                    crate::bloat_detector::BloatSeverity::Moderate => {
                        Severity::Warning
                    }
                    crate::bloat_detector::BloatSeverity::High => {
                        Severity::Error
                    }
                    crate::bloat_detector::BloatSeverity::Critical => {
                        Severity::Critical
                    }
                },
                category: Category::Bloat,
                target: info.table.clone(),
                message: format!(
                    "{:.1}% bloat ({} dead tuples)",
                    info.bloat_percent, info.dead_tuples,
                ),
                suggestion: info.suggestion.clone(),
            });
        }

        for info in self.staleness_checker.findings() {
            recs.push(Recommendation {
                severity: match info.level {
                    crate::stats_staleness::StalenessLevel::Fresh => {
                        continue;
                    }
                    crate::stats_staleness::StalenessLevel::Aging => {
                        Severity::Info
                    }
                    crate::stats_staleness::StalenessLevel::Stale => {
                        Severity::Warning
                    }
                    crate::stats_staleness::StalenessLevel::VeryStale => {
                        Severity::Error
                    }
                },
                category: Category::Statistics,
                target: info.table.clone(),
                message: info.message.clone(),
                suggestion: info.suggestion.clone(),
            });
        }

        recs.sort_by(|a, b| b.severity.cmp(&a.severity));
        recs
    }

    /// Get a mutable reference to the query monitor.
    pub fn query_monitor_mut(&mut self) -> &mut QueryMonitor {
        &mut self.query_monitor
    }

    /// Get a reference to the query monitor.
    #[must_use]
    pub fn query_monitor(&self) -> &QueryMonitor {
        &self.query_monitor
    }

    /// Get a reference to the schema analyzer.
    #[must_use]
    pub fn schema_analyzer(&self) -> &SchemaAnalyzer {
        &self.schema_analyzer
    }

    /// Get a reference to the config checker.
    #[must_use]
    pub fn config_checker(&self) -> &ConfigChecker {
        &self.config_checker
    }

    /// Get a reference to the bloat detector.
    #[must_use]
    pub fn bloat_detector(&self) -> &BloatDetector {
        &self.bloat_detector
    }

    /// Get a reference to the staleness checker.
    #[must_use]
    pub fn staleness_checker(&self) -> &StalenessChecker {
        &self.staleness_checker
    }
}

fn truncate_query(query: &str, max_len: usize) -> String {
    let trimmed = query.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(Severity::Critical > Severity::Error);
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Info.to_string(), "INFO");
        assert_eq!(Severity::Warning.to_string(), "WARN");
        assert_eq!(Severity::Error.to_string(), "ERROR");
        assert_eq!(Severity::Critical.to_string(), "CRIT");
    }

    #[test]
    fn category_display() {
        assert_eq!(Category::Query.to_string(), "Query");
        assert_eq!(Category::Schema.to_string(), "Schema");
        assert_eq!(Category::Config.to_string(), "Config");
        assert_eq!(Category::Bloat.to_string(), "Bloat");
        assert_eq!(Category::Statistics.to_string(), "Statistics");
    }

    #[test]
    fn truncate_short_query() {
        let q = "SELECT 1";
        assert_eq!(truncate_query(q, 60), "SELECT 1");
    }

    #[test]
    fn truncate_long_query() {
        let q = "SELECT very_long_column_name, another_column, yet_another FROM extremely_long_table_name WHERE condition = true";
        let result = truncate_query(q, 60);
        assert_eq!(result.len(), 60);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn recommendation_display() {
        let rec = Recommendation {
            severity: Severity::Warning,
            category: Category::Schema,
            target: "users".to_string(),
            message: "3 unused indexes".to_string(),
            suggestion: "DROP INDEX ...".to_string(),
        };
        let display = rec.to_string();
        assert!(display.contains("WARN"));
        assert!(display.contains("Schema"));
        assert!(display.contains("users"));
    }

    #[test]
    fn advisor_empty_recommendations() {
        let advisor = Advisor::new(
            QueryMonitor::new(100.0),
            SchemaAnalyzer::new(),
            ConfigChecker::new(),
            BloatDetector::new(),
            StalenessChecker::new(),
        );
        let recs = advisor.all_recommendations();
        assert!(recs.is_empty());
    }
}
