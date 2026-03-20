//! Table and index bloat detection.
//!
//! Identifies tables with excessive dead tuples that need VACUUM,
//! and indexes with bloat from page splits and deletions.

use std::fmt;

use serde::{Deserialize, Serialize};

/// How severe is the bloat?
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
    Hash, Serialize, Deserialize,
)]
pub enum BloatSeverity {
    /// Under 10% bloat, normal.
    Low,
    /// 10-30% bloat, worth monitoring.
    Moderate,
    /// 30-50% bloat, should vacuum.
    High,
    /// Over 50% bloat, urgent action needed.
    Critical,
}

impl fmt::Display for BloatSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Moderate => write!(f, "Moderate"),
            Self::High => write!(f, "High"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

/// Bloat information for a single table or index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BloatInfo {
    /// Table name.
    pub table: String,
    /// Index name (None for table-level bloat).
    pub index_name: Option<String>,
    /// Number of live tuples.
    pub live_tuples: u64,
    /// Number of dead tuples.
    pub dead_tuples: u64,
    /// Bloat percentage (dead / (live + dead) * 100).
    pub bloat_percent: f64,
    /// Severity classification.
    pub severity: BloatSeverity,
    /// Suggested remediation.
    pub suggestion: String,
    /// Last autovacuum timestamp (Unix epoch, if known).
    pub last_autovacuum: Option<i64>,
}

impl fmt::Display for BloatInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(idx) = &self.index_name {
            write!(
                f,
                "{}.{}: {:.1}% bloat ({})",
                self.table, idx, self.bloat_percent, self.severity,
            )
        } else {
            write!(
                f,
                "{}: {:.1}% bloat ({})",
                self.table, self.bloat_percent, self.severity,
            )
        }
    }
}

/// Input data for bloat analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableBloatInput {
    /// Table name.
    pub table: String,
    /// Live tuples from `pg_stat_user_tables`.
    pub live_tuples: u64,
    /// Dead tuples from `pg_stat_user_tables`.
    pub dead_tuples: u64,
    /// Last autovacuum time (Unix epoch).
    pub last_autovacuum: Option<i64>,
    /// Index bloat entries (name, live, dead).
    pub index_bloat: Vec<(String, u64, u64)>,
}

/// Detects table and index bloat.
pub struct BloatDetector {
    findings: Vec<BloatInfo>,
    /// Threshold percentage above which bloat is reported.
    threshold_percent: f64,
}

impl BloatDetector {
    /// Create a new bloat detector with default 5% threshold.
    #[must_use]
    pub fn new() -> Self {
        Self {
            findings: Vec::new(),
            threshold_percent: 5.0,
        }
    }

    /// Create a detector with a custom threshold.
    #[must_use]
    pub fn with_threshold(threshold_percent: f64) -> Self {
        Self {
            findings: Vec::new(),
            threshold_percent,
        }
    }

    /// Analyze a table and its indexes for bloat.
    pub fn analyze_table(&mut self, input: &TableBloatInput) {
        let total = input.live_tuples + input.dead_tuples;
        if total == 0 {
            return;
        }

        let bloat_percent =
            (input.dead_tuples as f64 / total as f64) * 100.0;
        if bloat_percent >= self.threshold_percent {
            let severity = classify_bloat(bloat_percent);
            self.findings.push(BloatInfo {
                table: input.table.clone(),
                index_name: None,
                live_tuples: input.live_tuples,
                dead_tuples: input.dead_tuples,
                bloat_percent,
                severity,
                suggestion: table_suggestion(
                    &input.table,
                    severity,
                ),
                last_autovacuum: input.last_autovacuum,
            });
        }

        for (idx_name, live, dead) in &input.index_bloat {
            let idx_total = live + dead;
            if idx_total == 0 {
                continue;
            }
            let idx_bloat =
                (*dead as f64 / idx_total as f64) * 100.0;
            if idx_bloat >= self.threshold_percent {
                let severity = classify_bloat(idx_bloat);
                self.findings.push(BloatInfo {
                    table: input.table.clone(),
                    index_name: Some(idx_name.clone()),
                    live_tuples: *live,
                    dead_tuples: *dead,
                    bloat_percent: idx_bloat,
                    severity,
                    suggestion: format!(
                        "REINDEX INDEX {idx_name};",
                    ),
                    last_autovacuum: None,
                });
            }
        }
    }

    /// Get all bloat findings.
    #[must_use]
    pub fn findings(&self) -> &[BloatInfo] {
        &self.findings
    }

    /// Get findings exceeding a given severity.
    #[must_use]
    pub fn findings_above(
        &self,
        min_severity: BloatSeverity,
    ) -> Vec<&BloatInfo> {
        self.findings
            .iter()
            .filter(|f| f.severity >= min_severity)
            .collect()
    }

    /// Clear all findings.
    pub fn clear(&mut self) {
        self.findings.clear();
    }
}

impl Default for BloatDetector {
    fn default() -> Self {
        Self::new()
    }
}

fn classify_bloat(percent: f64) -> BloatSeverity {
    if percent >= 50.0 {
        BloatSeverity::Critical
    } else if percent >= 30.0 {
        BloatSeverity::High
    } else if percent >= 10.0 {
        BloatSeverity::Moderate
    } else {
        BloatSeverity::Low
    }
}

fn table_suggestion(
    table: &str,
    severity: BloatSeverity,
) -> String {
    match severity {
        BloatSeverity::Low => {
            format!("Monitor {table}; autovacuum should handle this")
        }
        BloatSeverity::Moderate => {
            format!("VACUUM {table};")
        }
        BloatSeverity::High => {
            format!("VACUUM (VERBOSE) {table};")
        }
        BloatSeverity::Critical => {
            format!(
                "VACUUM FULL {table}; -- requires exclusive lock"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_bloat_levels() {
        assert_eq!(classify_bloat(3.0), BloatSeverity::Low);
        assert_eq!(
            classify_bloat(15.0),
            BloatSeverity::Moderate,
        );
        assert_eq!(classify_bloat(35.0), BloatSeverity::High);
        assert_eq!(
            classify_bloat(60.0),
            BloatSeverity::Critical,
        );
    }

    #[test]
    fn severity_ordering() {
        assert!(
            BloatSeverity::Critical > BloatSeverity::High
        );
        assert!(
            BloatSeverity::High > BloatSeverity::Moderate
        );
        assert!(
            BloatSeverity::Moderate > BloatSeverity::Low
        );
    }

    #[test]
    fn detect_table_bloat() {
        let mut detector = BloatDetector::new();
        detector.analyze_table(&TableBloatInput {
            table: "orders".to_string(),
            live_tuples: 70_000,
            dead_tuples: 30_000,
            last_autovacuum: None,
            index_bloat: vec![],
        });

        assert_eq!(detector.findings().len(), 1);
        let finding = &detector.findings()[0];
        assert_eq!(finding.table, "orders");
        assert!(finding.index_name.is_none());
        assert!((finding.bloat_percent - 30.0).abs() < 0.1);
        assert_eq!(finding.severity, BloatSeverity::High);
    }

    #[test]
    fn detect_index_bloat() {
        let mut detector = BloatDetector::new();
        detector.analyze_table(&TableBloatInput {
            table: "orders".to_string(),
            live_tuples: 100_000,
            dead_tuples: 0,
            last_autovacuum: None,
            index_bloat: vec![(
                "idx_status".to_string(),
                50_000,
                50_000,
            )],
        });

        assert_eq!(detector.findings().len(), 1);
        let finding = &detector.findings()[0];
        assert_eq!(
            finding.index_name.as_deref(),
            Some("idx_status"),
        );
        assert!((finding.bloat_percent - 50.0).abs() < 0.1);
        assert_eq!(finding.severity, BloatSeverity::Critical);
    }

    #[test]
    fn skip_zero_total() {
        let mut detector = BloatDetector::new();
        detector.analyze_table(&TableBloatInput {
            table: "empty".to_string(),
            live_tuples: 0,
            dead_tuples: 0,
            last_autovacuum: None,
            index_bloat: vec![],
        });

        assert!(detector.findings().is_empty());
    }

    #[test]
    fn below_threshold_not_reported() {
        let mut detector =
            BloatDetector::with_threshold(10.0);
        detector.analyze_table(&TableBloatInput {
            table: "clean".to_string(),
            live_tuples: 95_000,
            dead_tuples: 5_000,
            last_autovacuum: None,
            index_bloat: vec![],
        });

        assert!(detector.findings().is_empty());
    }

    #[test]
    fn findings_above_filter() {
        let mut detector = BloatDetector::new();
        detector.analyze_table(&TableBloatInput {
            table: "mild".to_string(),
            live_tuples: 90_000,
            dead_tuples: 10_000,
            last_autovacuum: None,
            index_bloat: vec![],
        });
        detector.analyze_table(&TableBloatInput {
            table: "bad".to_string(),
            live_tuples: 40_000,
            dead_tuples: 60_000,
            last_autovacuum: None,
            index_bloat: vec![],
        });

        let high =
            detector.findings_above(BloatSeverity::High);
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].table, "bad");
    }

    #[test]
    fn clear_findings() {
        let mut detector = BloatDetector::new();
        detector.analyze_table(&TableBloatInput {
            table: "t".to_string(),
            live_tuples: 50,
            dead_tuples: 50,
            last_autovacuum: None,
            index_bloat: vec![],
        });
        assert!(!detector.findings().is_empty());
        detector.clear();
        assert!(detector.findings().is_empty());
    }

    #[test]
    fn bloat_info_display() {
        let info = BloatInfo {
            table: "users".to_string(),
            index_name: None,
            live_tuples: 90_000,
            dead_tuples: 10_000,
            bloat_percent: 10.0,
            severity: BloatSeverity::Moderate,
            suggestion: "VACUUM users;".to_string(),
            last_autovacuum: None,
        };
        let display = info.to_string();
        assert!(display.contains("users"));
        assert!(display.contains("10.0%"));
    }

    #[test]
    fn bloat_info_display_with_index() {
        let info = BloatInfo {
            table: "users".to_string(),
            index_name: Some("idx_name".to_string()),
            live_tuples: 50_000,
            dead_tuples: 50_000,
            bloat_percent: 50.0,
            severity: BloatSeverity::Critical,
            suggestion: "REINDEX INDEX idx_name;".to_string(),
            last_autovacuum: None,
        };
        let display = info.to_string();
        assert!(display.contains("idx_name"));
        assert!(display.contains("50.0%"));
    }
}
