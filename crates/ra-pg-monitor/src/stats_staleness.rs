//! Statistics freshness checking.
//!
//! Detects stale statistics that may cause the query planner to
//! produce suboptimal plans. Recommends ANALYZE operations and
//! auto-analyze threshold adjustments.

use std::fmt;

use serde::{Deserialize, Serialize};

/// How stale are the statistics?
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
    Hash, Serialize, Deserialize,
)]
pub enum StalenessLevel {
    /// Statistics are current.
    Fresh,
    /// Starting to age, but still usable.
    Aging,
    /// Stale enough to potentially affect plan quality.
    Stale,
    /// Severely outdated, likely causing bad plans.
    VeryStale,
}

impl fmt::Display for StalenessLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fresh => write!(f, "Fresh"),
            Self::Aging => write!(f, "Aging"),
            Self::Stale => write!(f, "Stale"),
            Self::VeryStale => write!(f, "Very Stale"),
        }
    }
}

/// Statistics staleness info for a table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StalenessInfo {
    /// Table name.
    pub table: String,
    /// Staleness level.
    pub level: StalenessLevel,
    /// Number of modifications since last ANALYZE.
    pub modifications_since_analyze: u64,
    /// Live tuple count.
    pub live_tuples: u64,
    /// Modification ratio (mods / `live_tuples`).
    pub modification_ratio: f64,
    /// Unix timestamp of last analyze.
    pub last_analyze: Option<i64>,
    /// Unix timestamp of last autoanalyze.
    pub last_autoanalyze: Option<i64>,
    /// Description of the staleness situation.
    pub message: String,
    /// Suggested action.
    pub suggestion: String,
}

impl fmt::Display for StalenessInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} ({:.1}% modified since last analyze)",
            self.table,
            self.level,
            self.modification_ratio * 100.0,
        )
    }
}

/// Input data for staleness analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStatsInput {
    /// Table name.
    pub table: String,
    /// Live tuples from `pg_stat_user_tables`.
    pub live_tuples: u64,
    /// `n_mod_since_analyze` from `pg_stat_user_tables`.
    pub modifications_since_analyze: u64,
    /// Unix timestamp of last manual ANALYZE.
    pub last_analyze: Option<i64>,
    /// Unix timestamp of last autoanalyze.
    pub last_autoanalyze: Option<i64>,
    /// Current `autovacuum_analyze_threshold` setting.
    pub analyze_threshold: u64,
    /// Current `autovacuum_analyze_scale_factor`.
    pub analyze_scale_factor: f64,
}

/// Checks statistics freshness across tables.
pub struct StalenessChecker {
    findings: Vec<StalenessInfo>,
    /// Ratio above which statistics are considered "aging".
    aging_ratio: f64,
    /// Ratio above which statistics are considered "stale".
    stale_ratio: f64,
    /// Ratio above which statistics are "very stale".
    very_stale_ratio: f64,
}

impl StalenessChecker {
    /// Create a new checker with default thresholds.
    ///
    /// Default thresholds: aging at 10%, stale at 25%,
    /// very stale at 50% of rows modified since last ANALYZE.
    #[must_use]
    pub fn new() -> Self {
        Self {
            findings: Vec::new(),
            aging_ratio: 0.10,
            stale_ratio: 0.25,
            very_stale_ratio: 0.50,
        }
    }

    /// Create a checker with custom thresholds.
    #[must_use]
    pub fn with_thresholds(
        aging: f64,
        stale: f64,
        very_stale: f64,
    ) -> Self {
        Self {
            findings: Vec::new(),
            aging_ratio: aging,
            stale_ratio: stale,
            very_stale_ratio: very_stale,
        }
    }

    /// Analyze a table's statistics freshness.
    pub fn analyze_table(&mut self, input: &TableStatsInput) {
        if input.live_tuples == 0 {
            return;
        }

        let ratio = input.modifications_since_analyze as f64
            / input.live_tuples as f64;

        let level = self.classify(ratio);

        let message = format!(
            "{} modifications since last analyze \
             ({:.1}% of {} live tuples)",
            input.modifications_since_analyze,
            ratio * 100.0,
            input.live_tuples,
        );

        let suggestion = match level {
            StalenessLevel::Fresh => String::new(),
            StalenessLevel::Aging => format!(
                "Consider scheduling ANALYZE {} soon",
                input.table,
            ),
            StalenessLevel::Stale => format!(
                "ANALYZE {};",
                input.table,
            ),
            StalenessLevel::VeryStale => {
                let threshold_suggestion =
                    self.check_autoanalyze_config(input);
                if threshold_suggestion.is_empty() {
                    format!(
                        "ANALYZE {} IMMEDIATELY; statistics \
                         are severely outdated",
                        input.table,
                    )
                } else {
                    format!(
                        "ANALYZE {} IMMEDIATELY; also {}",
                        input.table, threshold_suggestion,
                    )
                }
            }
        };

        self.findings.push(StalenessInfo {
            table: input.table.clone(),
            level,
            modifications_since_analyze: input
                .modifications_since_analyze,
            live_tuples: input.live_tuples,
            modification_ratio: ratio,
            last_analyze: input.last_analyze,
            last_autoanalyze: input.last_autoanalyze,
            message,
            suggestion,
        });
    }

    /// Get all staleness findings.
    #[must_use]
    pub fn findings(&self) -> &[StalenessInfo] {
        &self.findings
    }

    /// Get stale or worse findings only.
    #[must_use]
    pub fn stale_tables(&self) -> Vec<&StalenessInfo> {
        self.findings
            .iter()
            .filter(|f| f.level >= StalenessLevel::Stale)
            .collect()
    }

    /// Clear all findings.
    pub fn clear(&mut self) {
        self.findings.clear();
    }

    fn classify(&self, ratio: f64) -> StalenessLevel {
        if ratio >= self.very_stale_ratio {
            StalenessLevel::VeryStale
        } else if ratio >= self.stale_ratio {
            StalenessLevel::Stale
        } else if ratio >= self.aging_ratio {
            StalenessLevel::Aging
        } else {
            StalenessLevel::Fresh
        }
    }

    #[allow(clippy::unused_self)]
    fn check_autoanalyze_config(
        &self,
        input: &TableStatsInput,
    ) -> String {
        let trigger_count = input.analyze_threshold as f64
            + (input.analyze_scale_factor
                * input.live_tuples as f64);

        if input.modifications_since_analyze as f64
            > trigger_count * 2.0
        {
            format!(
                "autoanalyze should have triggered at {} mods \
                 (threshold: {}, scale_factor: {:.2}); \
                 check autovacuum is running",
                trigger_count as u64,
                input.analyze_threshold,
                input.analyze_scale_factor,
            )
        } else {
            String::new()
        }
    }
}

impl Default for StalenessChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(
        table: &str,
        live: u64,
        mods: u64,
    ) -> TableStatsInput {
        TableStatsInput {
            table: table.to_string(),
            live_tuples: live,
            modifications_since_analyze: mods,
            last_analyze: Some(1_700_000_000),
            last_autoanalyze: Some(1_700_000_000),
            analyze_threshold: 50,
            analyze_scale_factor: 0.1,
        }
    }

    #[test]
    fn classify_fresh() {
        let checker = StalenessChecker::new();
        assert_eq!(
            checker.classify(0.05),
            StalenessLevel::Fresh,
        );
    }

    #[test]
    fn classify_aging() {
        let checker = StalenessChecker::new();
        assert_eq!(
            checker.classify(0.15),
            StalenessLevel::Aging,
        );
    }

    #[test]
    fn classify_stale() {
        let checker = StalenessChecker::new();
        assert_eq!(
            checker.classify(0.30),
            StalenessLevel::Stale,
        );
    }

    #[test]
    fn classify_very_stale() {
        let checker = StalenessChecker::new();
        assert_eq!(
            checker.classify(0.60),
            StalenessLevel::VeryStale,
        );
    }

    #[test]
    fn staleness_level_ordering() {
        assert!(
            StalenessLevel::VeryStale > StalenessLevel::Stale
        );
        assert!(
            StalenessLevel::Stale > StalenessLevel::Aging
        );
        assert!(
            StalenessLevel::Aging > StalenessLevel::Fresh
        );
    }

    #[test]
    fn analyze_fresh_table() {
        let mut checker = StalenessChecker::new();
        checker.analyze_table(&make_input(
            "users", 100_000, 1_000,
        ));
        assert_eq!(checker.findings().len(), 1);
        assert_eq!(
            checker.findings()[0].level,
            StalenessLevel::Fresh,
        );
    }

    #[test]
    fn analyze_stale_table() {
        let mut checker = StalenessChecker::new();
        checker.analyze_table(&make_input(
            "orders", 100_000, 30_000,
        ));
        let stale = checker.stale_tables();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].table, "orders");
    }

    #[test]
    fn skip_empty_table() {
        let mut checker = StalenessChecker::new();
        checker.analyze_table(&make_input("empty", 0, 0));
        assert!(checker.findings().is_empty());
    }

    #[test]
    fn very_stale_with_autoanalyze_warning() {
        let mut checker = StalenessChecker::new();
        let mut input =
            make_input("big_table", 100_000, 60_000);
        // The autoanalyze trigger should be at
        // 50 + 0.1 * 100000 = 10050 mods.
        // 60000 > 10050 * 2 = 20100, so should warn.
        input.analyze_threshold = 50;
        input.analyze_scale_factor = 0.1;
        checker.analyze_table(&input);

        assert_eq!(checker.findings().len(), 1);
        assert_eq!(
            checker.findings()[0].level,
            StalenessLevel::VeryStale,
        );
        assert!(
            checker.findings()[0]
                .suggestion
                .contains("autovacuum"),
        );
    }

    #[test]
    fn custom_thresholds() {
        let checker =
            StalenessChecker::with_thresholds(0.05, 0.15, 0.30);
        assert_eq!(
            checker.classify(0.06),
            StalenessLevel::Aging,
        );
        assert_eq!(
            checker.classify(0.20),
            StalenessLevel::Stale,
        );
        assert_eq!(
            checker.classify(0.35),
            StalenessLevel::VeryStale,
        );
    }

    #[test]
    fn clear_findings() {
        let mut checker = StalenessChecker::new();
        checker.analyze_table(&make_input("t", 1000, 500));
        assert!(!checker.findings().is_empty());
        checker.clear();
        assert!(checker.findings().is_empty());
    }

    #[test]
    fn staleness_info_display() {
        let info = StalenessInfo {
            table: "users".to_string(),
            level: StalenessLevel::Stale,
            modifications_since_analyze: 25_000,
            live_tuples: 100_000,
            modification_ratio: 0.25,
            last_analyze: Some(1_700_000_000),
            last_autoanalyze: None,
            message: "test".to_string(),
            suggestion: "ANALYZE users;".to_string(),
        };
        let display = info.to_string();
        assert!(display.contains("users"));
        assert!(display.contains("Stale"));
        assert!(display.contains("25.0%"));
    }
}
