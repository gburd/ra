//! Statistics accuracy and staleness modeling.
//!
//! Tracks the reliability and freshness of statistics to guide
//! optimizer decisions and re-analysis triggers.

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// State of statistics including staleness and confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatisticsState {
    /// When statistics were gathered (Unix timestamp).
    pub gathered_at: i64,
    /// Source of the statistics.
    pub source: StatisticsSource,
    /// Confidence level (0.0 to 1.0).
    pub confidence: f64,
    /// Number of modifications since statistics were gathered.
    pub modifications_since: u64,
    /// Total rows at time of gathering.
    pub rows_at_gathering: u64,
}

/// Source of statistics information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatisticsSource {
    /// Exact count from full table scan.
    ExactCount,
    /// Sampled statistics (with sample rate).
    Sampled {
        /// Sample rate as percentage (0-100).
        sample_rate: u32,
    },
    /// Histogram-based estimation.
    Histogram,
    /// Machine learning model prediction.
    MlModel {
        /// Name of the ML model used.
        model_name: String,
    },
    /// Derived from other statistics.
    Derived,
    /// Default/hardcoded values.
    Default,
}

/// Staleness classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Staleness {
    /// Fresh (< 1% data change).
    Fresh,
    /// Slightly stale (1-5% change).
    SlightlyStale,
    /// Moderately stale (5-20% change).
    ModeratelyStale,
    /// Very stale (> 20% change).
    VeryStale,
    /// Unknown staleness.
    Unknown,
}

impl StatisticsState {
    /// Create new statistics state.
    pub fn new(source: StatisticsSource, rows: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs() as i64;

        let confidence = match source {
            StatisticsSource::ExactCount => 1.0,
            StatisticsSource::Sampled { sample_rate } => f64::from(sample_rate) / 100.0,
            StatisticsSource::Histogram => 0.8,
            StatisticsSource::MlModel { .. } => 0.7,
            StatisticsSource::Derived => 0.6,
            StatisticsSource::Default => 0.3,
        };

        Self {
            gathered_at: now,
            source,
            confidence,
            modifications_since: 0,
            rows_at_gathering: rows,
        }
    }

    /// Calculate staleness based on modifications.
    pub fn staleness(&self) -> Staleness {
        if self.rows_at_gathering == 0 {
            return Staleness::Unknown;
        }

        let change_rate =
            self.modifications_since as f64 / self.rows_at_gathering as f64;

        if change_rate < 0.01 {
            Staleness::Fresh
        } else if change_rate < 0.05 {
            Staleness::SlightlyStale
        } else if change_rate < 0.20 {
            Staleness::ModeratelyStale
        } else {
            Staleness::VeryStale
        }
    }

    /// Calculate age in seconds.
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs() as i64;
        (now - self.gathered_at).max(0) as u64
    }

    /// Check if statistics should be refreshed.
    pub fn should_refresh(&self, threshold: RefreshThreshold) -> bool {
        match threshold {
            RefreshThreshold::Never => false,
            RefreshThreshold::Age(max_age) => self.age_seconds() > max_age,
            RefreshThreshold::Staleness(max_staleness) => self.staleness() > max_staleness,
            RefreshThreshold::Modifications(max_mods) => {
                self.modifications_since > max_mods
            }
            RefreshThreshold::Confidence(min_confidence) => {
                self.confidence < min_confidence
            }
            RefreshThreshold::Any(thresholds) => {
                thresholds.iter().any(|t| self.should_refresh(t.clone()))
            }
            RefreshThreshold::All(thresholds) => {
                thresholds.iter().all(|t| self.should_refresh(t.clone()))
            }
        }
    }

    /// Record modifications to the table.
    pub fn record_modifications(&mut self, count: u64) {
        self.modifications_since += count;
    }

    /// Decay confidence over time.
    pub fn decay_confidence(&mut self, decay_rate: f64) {
        let age_days = self.age_seconds() as f64 / 86400.0;
        self.confidence *= (-decay_rate * age_days).exp();
        self.confidence = self.confidence.max(0.0);
    }
}

/// Threshold for triggering statistics refresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RefreshThreshold {
    /// Never refresh.
    Never,
    /// Refresh after age in seconds.
    Age(u64),
    /// Refresh when staleness exceeds level.
    Staleness(Staleness),
    /// Refresh after number of modifications.
    Modifications(u64),
    /// Refresh when confidence drops below threshold.
    Confidence(f64),
    /// Refresh if any condition is met.
    Any(Vec<RefreshThreshold>),
    /// Refresh only if all conditions are met.
    All(Vec<RefreshThreshold>),
}

/// Statistics quality assessment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityMetrics {
    /// Overall quality score (0.0 to 1.0).
    pub quality_score: f64,
    /// Freshness score (0.0 to 1.0).
    pub freshness: f64,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Coverage score (0.0 to 1.0).
    pub coverage: f64,
}

impl QualityMetrics {
    /// Calculate quality metrics from statistics state.
    pub fn from_state(state: &StatisticsState) -> Self {
        let freshness = match state.staleness() {
            Staleness::Fresh => 1.0,
            Staleness::SlightlyStale => 0.8,
            Staleness::ModeratelyStale => 0.5,
            Staleness::VeryStale => 0.2,
            Staleness::Unknown => 0.0,
        };

        let confidence = state.confidence;

        let coverage = match state.source {
            StatisticsSource::ExactCount => 1.0,
            StatisticsSource::Sampled { sample_rate } => f64::from(sample_rate) / 100.0,
            StatisticsSource::Histogram => 0.7,
            StatisticsSource::MlModel { .. } => 0.6,
            StatisticsSource::Derived => 0.5,
            StatisticsSource::Default => 0.1,
        };

        let quality_score = (freshness + confidence + coverage) / 3.0;

        Self {
            quality_score,
            freshness,
            confidence,
            coverage,
        }
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    // ---- StatisticsState creation ----

    #[test]
    fn state_exact_count_confidence() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        assert_eq!(s.confidence, 1.0);
        assert_eq!(s.rows_at_gathering, 1_000_000);
        assert_eq!(s.modifications_since, 0);
    }

    #[test]
    fn state_sampled_10_confidence() {
        let s = StatisticsState::new(
            StatisticsSource::Sampled { sample_rate: 10 },
            1_000_000,
        );
        assert_eq!(s.confidence, 0.1);
    }

    #[test]
    fn state_sampled_100_confidence() {
        let s = StatisticsState::new(
            StatisticsSource::Sampled { sample_rate: 100 },
            1_000,
        );
        assert_eq!(s.confidence, 1.0);
    }

    #[test]
    fn state_histogram_confidence() {
        let s = StatisticsState::new(StatisticsSource::Histogram, 1_000);
        assert_eq!(s.confidence, 0.8);
    }

    #[test]
    fn state_ml_model_confidence() {
        let s = StatisticsState::new(
            StatisticsSource::MlModel {
                model_name: "xgboost_v1".to_string(),
            },
            1_000,
        );
        assert_eq!(s.confidence, 0.7);
    }

    #[test]
    fn state_derived_confidence() {
        let s = StatisticsState::new(StatisticsSource::Derived, 1_000);
        assert_eq!(s.confidence, 0.6);
    }

    #[test]
    fn state_default_confidence() {
        let s = StatisticsState::new(StatisticsSource::Default, 1_000);
        assert_eq!(s.confidence, 0.3);
    }

    // ---- Staleness ----

    #[test]
    fn staleness_fresh() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        assert_eq!(s.staleness(), Staleness::Fresh);
    }

    #[test]
    fn staleness_slightly_stale() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            1_000_000,
        );
        s.record_modifications(20_000);
        assert_eq!(s.staleness(), Staleness::SlightlyStale);
    }

    #[test]
    fn staleness_moderately_stale() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            1_000_000,
        );
        s.record_modifications(100_000);
        assert_eq!(s.staleness(), Staleness::ModeratelyStale);
    }

    #[test]
    fn staleness_very_stale() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            1_000_000,
        );
        s.record_modifications(300_000);
        assert_eq!(s.staleness(), Staleness::VeryStale);
    }

    #[test]
    fn staleness_zero_rows_unknown() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 0);
        assert_eq!(s.staleness(), Staleness::Unknown);
    }

    #[test]
    fn staleness_ordering() {
        assert!(Staleness::Fresh < Staleness::SlightlyStale);
        assert!(Staleness::SlightlyStale < Staleness::ModeratelyStale);
        assert!(Staleness::ModeratelyStale < Staleness::VeryStale);
        assert!(Staleness::VeryStale < Staleness::Unknown);
    }

    #[test]
    fn staleness_boundary_1_percent() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            100_000,
        );
        s.record_modifications(999);
        assert_eq!(s.staleness(), Staleness::Fresh);
        s.record_modifications(1);
        assert_eq!(s.staleness(), Staleness::SlightlyStale);
    }

    #[test]
    fn staleness_boundary_5_percent() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            100_000,
        );
        s.record_modifications(4_999);
        assert_eq!(s.staleness(), Staleness::SlightlyStale);
        s.record_modifications(1);
        assert_eq!(s.staleness(), Staleness::ModeratelyStale);
    }

    #[test]
    fn staleness_boundary_20_percent() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            100_000,
        );
        s.record_modifications(19_999);
        assert_eq!(s.staleness(), Staleness::ModeratelyStale);
        s.record_modifications(1);
        assert_eq!(s.staleness(), Staleness::VeryStale);
    }

    // ---- record_modifications accumulates ----

    #[test]
    fn record_modifications_accumulates() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.record_modifications(100);
        s.record_modifications(200);
        assert_eq!(s.modifications_since, 300);
    }

    // ---- age_seconds ----

    #[test]
    fn age_seconds_recent() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        assert!(s.age_seconds() < 2);
    }

    #[test]
    fn age_seconds_old() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.gathered_at -= 3600;
        assert!(s.age_seconds() >= 3599);
    }

    // ---- RefreshThreshold ----

    #[test]
    fn refresh_never() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        assert!(!s.should_refresh(RefreshThreshold::Never));
    }

    #[test]
    fn refresh_age_not_met() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        assert!(!s.should_refresh(RefreshThreshold::Age(3600)));
    }

    #[test]
    fn refresh_age_met() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.gathered_at -= 7200;
        assert!(s.should_refresh(RefreshThreshold::Age(3600)));
    }

    #[test]
    fn refresh_modifications_not_met() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        assert!(!s.should_refresh(RefreshThreshold::Modifications(100)));
    }

    #[test]
    fn refresh_modifications_met() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.record_modifications(200);
        assert!(s.should_refresh(RefreshThreshold::Modifications(100)));
    }

    #[test]
    fn refresh_staleness_not_met() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        assert!(
            !s.should_refresh(RefreshThreshold::Staleness(Staleness::Fresh))
        );
    }

    #[test]
    fn refresh_staleness_met() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            1_000,
        );
        s.record_modifications(500);
        assert!(s.should_refresh(RefreshThreshold::Staleness(
            Staleness::Fresh
        )));
    }

    #[test]
    fn refresh_confidence_not_met() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        assert!(!s.should_refresh(RefreshThreshold::Confidence(0.5)));
    }

    #[test]
    fn refresh_confidence_met() {
        let s = StatisticsState::new(StatisticsSource::Default, 1_000);
        assert!(s.should_refresh(RefreshThreshold::Confidence(0.5)));
    }

    #[test]
    fn refresh_any_one_met() {
        let s = StatisticsState::new(StatisticsSource::Default, 1_000);
        let threshold = RefreshThreshold::Any(vec![
            RefreshThreshold::Confidence(0.5),
            RefreshThreshold::Never,
        ]);
        assert!(s.should_refresh(threshold));
    }

    #[test]
    fn refresh_any_none_met() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        let threshold = RefreshThreshold::Any(vec![
            RefreshThreshold::Confidence(0.5),
            RefreshThreshold::Age(999_999),
        ]);
        assert!(!s.should_refresh(threshold));
    }

    #[test]
    fn refresh_all_both_met() {
        let mut s = StatisticsState::new(StatisticsSource::Default, 1_000);
        s.record_modifications(200);
        let threshold = RefreshThreshold::All(vec![
            RefreshThreshold::Confidence(0.5),
            RefreshThreshold::Modifications(100),
        ]);
        assert!(s.should_refresh(threshold));
    }

    #[test]
    fn refresh_all_only_one_met() {
        let s = StatisticsState::new(StatisticsSource::Default, 1_000);
        let threshold = RefreshThreshold::All(vec![
            RefreshThreshold::Confidence(0.5),
            RefreshThreshold::Modifications(100),
        ]);
        assert!(!s.should_refresh(threshold));
    }

    // ---- confidence decay ----

    #[test]
    fn confidence_decay_recent() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.decay_confidence(0.1);
        assert!(s.confidence > 0.99);
    }

    #[test]
    fn confidence_decay_old() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.gathered_at -= 86400;
        s.decay_confidence(0.1);
        assert!(s.confidence < 1.0);
        assert!(s.confidence > 0.8);
    }

    #[test]
    fn confidence_decay_very_old() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.gathered_at -= 86400 * 30;
        s.decay_confidence(0.1);
        assert!(s.confidence < 0.1);
    }

    #[test]
    fn confidence_decay_zero_rate() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.gathered_at -= 86400;
        s.decay_confidence(0.0);
        assert_eq!(s.confidence, 1.0);
    }

    #[test]
    fn confidence_never_negative() {
        let mut s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        s.gathered_at -= 86400 * 365;
        s.decay_confidence(1.0);
        assert!(s.confidence >= 0.0);
    }

    // ---- QualityMetrics ----

    #[test]
    fn quality_metrics_exact_fresh() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        let m = QualityMetrics::from_state(&s);
        assert_eq!(m.freshness, 1.0);
        assert_eq!(m.confidence, 1.0);
        assert_eq!(m.coverage, 1.0);
        assert_eq!(m.quality_score, 1.0);
    }

    #[test]
    fn quality_metrics_default_source() {
        let s = StatisticsState::new(StatisticsSource::Default, 1_000);
        let m = QualityMetrics::from_state(&s);
        assert_eq!(m.coverage, 0.1);
        assert_eq!(m.confidence, 0.3);
    }

    #[test]
    fn quality_metrics_stale() {
        let mut s = StatisticsState::new(
            StatisticsSource::ExactCount,
            1_000,
        );
        s.record_modifications(500);
        let m = QualityMetrics::from_state(&s);
        assert_eq!(m.freshness, 0.2);
    }

    #[test]
    fn quality_metrics_unknown_staleness() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 0);
        let m = QualityMetrics::from_state(&s);
        assert_eq!(m.freshness, 0.0);
    }

    #[test]
    fn quality_score_is_average() {
        let s = StatisticsState::new(
            StatisticsSource::Sampled { sample_rate: 50 },
            1_000,
        );
        let m = QualityMetrics::from_state(&s);
        let expected = (m.freshness + m.confidence + m.coverage) / 3.0;
        assert!((m.quality_score - expected).abs() < f64::EPSILON);
    }

    // ---- Serialization ----

    #[test]
    fn statistics_state_serialize_roundtrip() {
        let s = StatisticsState::new(StatisticsSource::ExactCount, 1_000);
        let json = serde_json::to_string(&s)
            .expect("serialize");
        let d: StatisticsState = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(s, d);
    }

    #[test]
    fn staleness_serialize_roundtrip() {
        let s = Staleness::ModeratelyStale;
        let json = serde_json::to_string(&s)
            .expect("serialize");
        let d: Staleness = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(s, d);
    }

    #[test]
    fn refresh_threshold_serialize_roundtrip() {
        let t = RefreshThreshold::Any(vec![
            RefreshThreshold::Age(3600),
            RefreshThreshold::Modifications(1000),
        ]);
        let json = serde_json::to_string(&t)
            .expect("serialize");
        let d: RefreshThreshold = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(t, d);
    }
}
