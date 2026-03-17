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

    #[test]
    fn test_statistics_state_creation() {
        let state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        assert_eq!(state.confidence, 1.0);
        assert_eq!(state.rows_at_gathering, 1_000_000);
        assert_eq!(state.modifications_since, 0);
    }

    #[test]
    fn test_staleness_calculation() {
        let mut state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        assert_eq!(state.staleness(), Staleness::Fresh);

        state.record_modifications(20_000);
        assert_eq!(state.staleness(), Staleness::SlightlyStale);

        state.record_modifications(80_000);
        assert_eq!(state.staleness(), Staleness::ModeratelyStale);

        state.record_modifications(200_000);
        assert_eq!(state.staleness(), Staleness::VeryStale);
    }

    #[test]
    fn test_sampled_statistics_confidence() {
        let state = StatisticsState::new(
            StatisticsSource::Sampled { sample_rate: 10 },
            1_000_000,
        );
        assert_eq!(state.confidence, 0.1);
    }

    #[test]
    fn test_refresh_threshold_age() {
        let state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        let threshold = RefreshThreshold::Age(3600);
        assert!(!state.should_refresh(threshold));
    }

    #[test]
    fn test_refresh_threshold_modifications() {
        let mut state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        state.record_modifications(100_000);
        let threshold = RefreshThreshold::Modifications(50_000);
        assert!(state.should_refresh(threshold));
    }

    #[test]
    fn test_quality_metrics() {
        let state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        let metrics = QualityMetrics::from_state(&state);
        assert!(metrics.quality_score > 0.9);
        assert_eq!(metrics.freshness, 1.0);
        assert_eq!(metrics.confidence, 1.0);
    }

    #[test]
    fn test_confidence_decay() {
        let mut state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
        state.gathered_at -= 86400; // 1 day ago
        state.decay_confidence(0.1);
        assert!(state.confidence < 1.0);
        assert!(state.confidence > 0.8);
    }
}
