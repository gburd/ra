//! Statistics gathering configuration profiles.
//!
//! Pre-configured profiles for different workload patterns and
//! performance requirements.

use crate::accuracy::RefreshThreshold;
use crate::gathering_cost::{GatheringMethod, GatheringPriority};
use serde::{Deserialize, Serialize};

/// Statistics configuration profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatisticsProfile {
    /// Profile name.
    pub name: String,
    /// Description of the profile.
    pub description: String,
    /// Default gathering method.
    pub default_method: GatheringMethod,
    /// Refresh threshold.
    pub refresh_threshold: RefreshThreshold,
    /// Gathering priority.
    pub priority: GatheringPriority,
    /// Maximum age before forced refresh (seconds).
    pub max_age_seconds: Option<u64>,
    /// Minimum confidence threshold.
    pub min_confidence: f64,
    /// Whether to gather multi-column statistics.
    pub multi_column_stats: bool,
    /// Whether to gather correlation statistics.
    pub correlation_stats: bool,
    /// Whether to use sketches for approximate stats.
    pub use_sketches: bool,
}

impl StatisticsProfile {
    /// Real-time profile for OLTP workloads.
    ///
    /// Prioritizes accuracy and freshness, using exact counts and
    /// frequent updates. Suitable for mission-critical transactional
    /// systems where query performance depends on accurate statistics.
    pub fn real_time() -> Self {
        Self {
            name: "RealTime".to_string(),
            description: "Aggressive statistics gathering for OLTP workloads".to_string(),
            default_method: GatheringMethod::Incremental,
            refresh_threshold: RefreshThreshold::Any(vec![
                RefreshThreshold::Modifications(1000),
                RefreshThreshold::Age(300),
            ]),
            priority: GatheringPriority::High,
            max_age_seconds: Some(600),
            min_confidence: 0.95,
            multi_column_stats: true,
            correlation_stats: true,
            use_sketches: false,
        }
    }

    /// Standard profile for mixed workloads.
    ///
    /// Balances accuracy and overhead using sampling and periodic
    /// refreshes. Suitable for most production workloads with moderate
    /// update rates.
    pub fn standard() -> Self {
        Self {
            name: "Standard".to_string(),
            description: "Balanced profile for mixed workloads".to_string(),
            default_method: GatheringMethod::BlockSample { sample_rate: 10 },
            refresh_threshold: RefreshThreshold::Any(vec![
                RefreshThreshold::Modifications(100_000),
                RefreshThreshold::Age(3600),
            ]),
            priority: GatheringPriority::Normal,
            max_age_seconds: Some(7200),
            min_confidence: 0.8,
            multi_column_stats: true,
            correlation_stats: false,
            use_sketches: true,
        }
    }

    /// Lazy profile for read-mostly workloads.
    ///
    /// Minimizes gathering overhead using aggressive sampling and
    /// infrequent updates. Suitable for data warehouses and analytics
    /// workloads with stable data.
    pub fn lazy() -> Self {
        Self {
            name: "Lazy".to_string(),
            description: "Minimal overhead for read-mostly workloads".to_string(),
            default_method: GatheringMethod::BlockSample { sample_rate: 5 },
            refresh_threshold: RefreshThreshold::Any(vec![
                RefreshThreshold::Modifications(1_000_000),
                RefreshThreshold::Age(86400),
            ]),
            priority: GatheringPriority::Low,
            max_age_seconds: Some(172_800),
            min_confidence: 0.6,
            multi_column_stats: false,
            correlation_stats: false,
            use_sketches: true,
        }
    }

    /// Stale profile for append-only workloads.
    ///
    /// Accepts stale statistics using sketches and very infrequent
    /// updates. Suitable for log data, time-series, or other workloads
    /// where approximate statistics are sufficient.
    pub fn stale() -> Self {
        Self {
            name: "Stale".to_string(),
            description: "Sketch-based approximations for append-only data".to_string(),
            default_method: GatheringMethod::Sketch,
            refresh_threshold: RefreshThreshold::Age(604_800),
            priority: GatheringPriority::Deferred,
            max_age_seconds: None,
            min_confidence: 0.3,
            multi_column_stats: false,
            correlation_stats: false,
            use_sketches: true,
        }
    }

    /// Analytical profile for OLAP workloads.
    ///
    /// Emphasizes comprehensive statistics including multi-column
    /// and correlation stats. Uses full scans for accuracy but with
    /// lower frequency updates.
    pub fn analytical() -> Self {
        Self {
            name: "Analytical".to_string(),
            description: "Comprehensive statistics for OLAP workloads".to_string(),
            default_method: GatheringMethod::FullScan,
            refresh_threshold: RefreshThreshold::Any(vec![
                RefreshThreshold::Modifications(10_000_000),
                RefreshThreshold::Age(43200),
            ]),
            priority: GatheringPriority::Normal,
            max_age_seconds: Some(86400),
            min_confidence: 0.9,
            multi_column_stats: true,
            correlation_stats: true,
            use_sketches: false,
        }
    }

    /// Streaming profile for continuous data ingestion.
    ///
    /// Uses incremental updates and sketches to maintain approximate
    /// statistics with minimal latency. Suitable for streaming systems
    /// and real-time analytics.
    pub fn streaming() -> Self {
        Self {
            name: "Streaming".to_string(),
            description: "Incremental updates for streaming data".to_string(),
            default_method: GatheringMethod::Sketch,
            refresh_threshold: RefreshThreshold::Modifications(10_000),
            priority: GatheringPriority::High,
            max_age_seconds: Some(60),
            min_confidence: 0.7,
            multi_column_stats: false,
            correlation_stats: false,
            use_sketches: true,
        }
    }

    /// Get all built-in profiles.
    pub fn all_profiles() -> Vec<Self> {
        vec![
            Self::real_time(),
            Self::standard(),
            Self::lazy(),
            Self::stale(),
            Self::analytical(),
            Self::streaming(),
        ]
    }

    /// Get profile by name.
    pub fn by_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "realtime" | "real_time" => Some(Self::real_time()),
            "standard" | "default" => Some(Self::standard()),
            "lazy" => Some(Self::lazy()),
            "stale" => Some(Self::stale()),
            "analytical" | "olap" => Some(Self::analytical()),
            "streaming" => Some(Self::streaming()),
            _ => None,
        }
    }
}

/// Profile selector based on workload characteristics.
#[derive(Debug, Clone)]
pub struct ProfileSelector {
    /// Average writes per second.
    pub writes_per_second: f64,
    /// Average reads per second.
    pub reads_per_second: f64,
    /// Table size in rows.
    pub table_size: u64,
    /// Query latency sensitivity (0.0 to 1.0).
    pub latency_sensitivity: f64,
}

impl ProfileSelector {
    /// Recommend a profile based on workload characteristics.
    pub fn recommend(&self) -> StatisticsProfile {
        let write_ratio = self.writes_per_second / (self.writes_per_second + self.reads_per_second);

        if write_ratio > 0.5 && self.latency_sensitivity > 0.8 {
            StatisticsProfile::real_time()
        } else if write_ratio > 0.3 {
            StatisticsProfile::standard()
        } else if self.table_size > 100_000_000 {
            StatisticsProfile::analytical()
        } else if write_ratio < 0.01 {
            StatisticsProfile::lazy()
        } else {
            StatisticsProfile::standard()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_real_time_profile() {
        let profile = StatisticsProfile::real_time();
        assert_eq!(profile.name, "RealTime");
        assert!(profile.min_confidence > 0.9);
        assert_eq!(profile.priority, GatheringPriority::High);
        assert!(profile.multi_column_stats);
    }

    #[test]
    fn test_standard_profile() {
        let profile = StatisticsProfile::standard();
        assert_eq!(profile.name, "Standard");
        assert!(profile.use_sketches);
        assert_eq!(profile.priority, GatheringPriority::Normal);
    }

    #[test]
    fn test_lazy_profile() {
        let profile = StatisticsProfile::lazy();
        assert_eq!(profile.name, "Lazy");
        assert_eq!(profile.priority, GatheringPriority::Low);
        assert!(!profile.multi_column_stats);
    }

    #[test]
    fn test_stale_profile() {
        let profile = StatisticsProfile::stale();
        assert_eq!(profile.name, "Stale");
        assert_eq!(profile.priority, GatheringPriority::Deferred);
        assert!(matches!(profile.default_method, GatheringMethod::Sketch));
    }

    #[test]
    fn test_analytical_profile() {
        let profile = StatisticsProfile::analytical();
        assert_eq!(profile.name, "Analytical");
        assert!(profile.correlation_stats);
        assert!(matches!(profile.default_method, GatheringMethod::FullScan));
    }

    #[test]
    fn test_streaming_profile() {
        let profile = StatisticsProfile::streaming();
        assert_eq!(profile.name, "Streaming");
        assert!(profile.use_sketches);
        assert!(matches!(profile.default_method, GatheringMethod::Sketch));
    }

    #[test]
    fn test_all_profiles() {
        let profiles = StatisticsProfile::all_profiles();
        assert_eq!(profiles.len(), 6);
    }

    #[test]
    fn test_profile_by_name() {
        assert!(StatisticsProfile::by_name("realtime").is_some());
        assert!(StatisticsProfile::by_name("standard").is_some());
        assert!(StatisticsProfile::by_name("lazy").is_some());
        assert!(StatisticsProfile::by_name("nonexistent").is_none());
    }

    #[test]
    fn test_profile_selector_high_write() {
        let selector = ProfileSelector {
            writes_per_second: 1000.0,
            reads_per_second: 500.0,
            table_size: 1_000_000,
            latency_sensitivity: 0.9,
        };
        let profile = selector.recommend();
        assert_eq!(profile.name, "RealTime");
    }

    #[test]
    fn test_profile_selector_read_mostly() {
        let selector = ProfileSelector {
            writes_per_second: 1.0,
            reads_per_second: 1000.0,
            table_size: 1_000_000,
            latency_sensitivity: 0.5,
        };
        let profile = selector.recommend();
        assert_eq!(profile.name, "Lazy");
    }

    #[test]
    fn test_profile_selector_analytical() {
        let selector = ProfileSelector {
            writes_per_second: 10.0,
            reads_per_second: 100.0,
            table_size: 200_000_000,
            latency_sensitivity: 0.3,
        };
        let profile = selector.recommend();
        assert_eq!(profile.name, "Analytical");
    }
}
