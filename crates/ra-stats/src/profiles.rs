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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    // ---- Individual profiles ----

    #[test]
    fn real_time_profile_name() {
        assert_eq!(StatisticsProfile::real_time().name, "RealTime");
    }

    #[test]
    fn real_time_high_confidence() {
        assert!(StatisticsProfile::real_time().min_confidence > 0.9);
    }

    #[test]
    fn real_time_high_priority() {
        assert_eq!(
            StatisticsProfile::real_time().priority,
            GatheringPriority::High
        );
    }

    #[test]
    fn real_time_multi_column() {
        assert!(StatisticsProfile::real_time().multi_column_stats);
    }

    #[test]
    fn real_time_correlation() {
        assert!(StatisticsProfile::real_time().correlation_stats);
    }

    #[test]
    fn real_time_no_sketches() {
        assert!(!StatisticsProfile::real_time().use_sketches);
    }

    #[test]
    fn standard_profile_name() {
        assert_eq!(StatisticsProfile::standard().name, "Standard");
    }

    #[test]
    fn standard_uses_sketches() {
        assert!(StatisticsProfile::standard().use_sketches);
    }

    #[test]
    fn standard_normal_priority() {
        assert_eq!(
            StatisticsProfile::standard().priority,
            GatheringPriority::Normal
        );
    }

    #[test]
    fn standard_block_sample() {
        assert!(matches!(
            StatisticsProfile::standard().default_method,
            GatheringMethod::BlockSample { .. }
        ));
    }

    #[test]
    fn lazy_profile_name() {
        assert_eq!(StatisticsProfile::lazy().name, "Lazy");
    }

    #[test]
    fn lazy_low_priority() {
        assert_eq!(
            StatisticsProfile::lazy().priority,
            GatheringPriority::Low
        );
    }

    #[test]
    fn lazy_no_multi_column() {
        assert!(!StatisticsProfile::lazy().multi_column_stats);
    }

    #[test]
    fn lazy_no_correlation() {
        assert!(!StatisticsProfile::lazy().correlation_stats);
    }

    #[test]
    fn stale_profile_name() {
        assert_eq!(StatisticsProfile::stale().name, "Stale");
    }

    #[test]
    fn stale_deferred_priority() {
        assert_eq!(
            StatisticsProfile::stale().priority,
            GatheringPriority::Deferred
        );
    }

    #[test]
    fn stale_uses_sketch() {
        assert!(matches!(
            StatisticsProfile::stale().default_method,
            GatheringMethod::Sketch
        ));
    }

    #[test]
    fn stale_no_max_age() {
        assert!(StatisticsProfile::stale().max_age_seconds.is_none());
    }

    #[test]
    fn analytical_profile_name() {
        assert_eq!(StatisticsProfile::analytical().name, "Analytical");
    }

    #[test]
    fn analytical_full_scan() {
        assert!(matches!(
            StatisticsProfile::analytical().default_method,
            GatheringMethod::FullScan
        ));
    }

    #[test]
    fn analytical_correlation() {
        assert!(StatisticsProfile::analytical().correlation_stats);
    }

    #[test]
    fn analytical_multi_column() {
        assert!(StatisticsProfile::analytical().multi_column_stats);
    }

    #[test]
    fn streaming_profile_name() {
        assert_eq!(StatisticsProfile::streaming().name, "Streaming");
    }

    #[test]
    fn streaming_uses_sketch() {
        assert!(matches!(
            StatisticsProfile::streaming().default_method,
            GatheringMethod::Sketch
        ));
    }

    #[test]
    fn streaming_uses_sketches() {
        assert!(StatisticsProfile::streaming().use_sketches);
    }

    #[test]
    fn streaming_high_priority() {
        assert_eq!(
            StatisticsProfile::streaming().priority,
            GatheringPriority::High
        );
    }

    // ---- all_profiles ----

    #[test]
    fn all_profiles_count() {
        assert_eq!(StatisticsProfile::all_profiles().len(), 6);
    }

    #[test]
    fn all_profiles_unique_names() {
        let names: Vec<String> = StatisticsProfile::all_profiles()
            .into_iter()
            .map(|p| p.name)
            .collect();
        let unique: std::collections::HashSet<&String> =
            names.iter().collect();
        assert_eq!(names.len(), unique.len());
    }

    // ---- by_name ----

    #[test]
    fn by_name_realtime() {
        assert!(StatisticsProfile::by_name("realtime").is_some());
    }

    #[test]
    fn by_name_real_time() {
        assert!(StatisticsProfile::by_name("real_time").is_some());
    }

    #[test]
    fn by_name_standard() {
        assert!(StatisticsProfile::by_name("standard").is_some());
    }

    #[test]
    fn by_name_default_alias() {
        let p = StatisticsProfile::by_name("default");
        assert!(p.is_some());
        assert_eq!(p.map(|p| p.name), Some("Standard".to_string()));
    }

    #[test]
    fn by_name_lazy() {
        assert!(StatisticsProfile::by_name("lazy").is_some());
    }

    #[test]
    fn by_name_stale() {
        assert!(StatisticsProfile::by_name("stale").is_some());
    }

    #[test]
    fn by_name_analytical() {
        assert!(StatisticsProfile::by_name("analytical").is_some());
    }

    #[test]
    fn by_name_olap_alias() {
        let p = StatisticsProfile::by_name("olap");
        assert!(p.is_some());
        assert_eq!(p.map(|p| p.name), Some("Analytical".to_string()));
    }

    #[test]
    fn by_name_streaming() {
        assert!(StatisticsProfile::by_name("streaming").is_some());
    }

    #[test]
    fn by_name_nonexistent() {
        assert!(StatisticsProfile::by_name("nonexistent").is_none());
    }

    #[test]
    fn by_name_case_insensitive() {
        assert!(StatisticsProfile::by_name("STANDARD").is_some());
        assert!(StatisticsProfile::by_name("Lazy").is_some());
    }

    // ---- ProfileSelector ----

    #[test]
    fn selector_high_write_latency_sensitive() {
        let s = ProfileSelector {
            writes_per_second: 1000.0,
            reads_per_second: 500.0,
            table_size: 1_000_000,
            latency_sensitivity: 0.9,
        };
        assert_eq!(s.recommend().name, "RealTime");
    }

    #[test]
    fn selector_moderate_write() {
        let s = ProfileSelector {
            writes_per_second: 100.0,
            reads_per_second: 200.0,
            table_size: 1_000_000,
            latency_sensitivity: 0.5,
        };
        assert_eq!(s.recommend().name, "Standard");
    }

    #[test]
    fn selector_read_mostly() {
        let s = ProfileSelector {
            writes_per_second: 1.0,
            reads_per_second: 1000.0,
            table_size: 1_000_000,
            latency_sensitivity: 0.5,
        };
        assert_eq!(s.recommend().name, "Lazy");
    }

    #[test]
    fn selector_large_analytical() {
        let s = ProfileSelector {
            writes_per_second: 10.0,
            reads_per_second: 100.0,
            table_size: 200_000_000,
            latency_sensitivity: 0.3,
        };
        assert_eq!(s.recommend().name, "Analytical");
    }

    #[test]
    fn selector_balanced_defaults_to_standard() {
        let s = ProfileSelector {
            writes_per_second: 50.0,
            reads_per_second: 200.0,
            table_size: 1_000_000,
            latency_sensitivity: 0.5,
        };
        assert_eq!(s.recommend().name, "Standard");
    }

    // ---- Serialization ----

    #[test]
    fn profile_serialize_roundtrip() {
        let p = StatisticsProfile::standard();
        let json = serde_json::to_string(&p)
            .expect("serialize");
        let d: StatisticsProfile = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(p, d);
    }
}
