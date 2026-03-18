//! Statistics abstraction system for query optimization.
//!
//! This crate provides a comprehensive statistics framework modeling
//! how database systems track and maintain query optimization metadata.
//! It includes:
//!
//! - **Statistics Types** ([`types`]): Catalog of 20+ statistics types
//!   including table-level, column-level, index, and correlation statistics.
//! - **Accuracy Modeling** ([`accuracy`]): Tracks staleness, confidence,
//!   and source quality to guide re-analysis decisions.
//! - **Gathering Cost** ([`gathering_cost`]): Estimates CPU, I/O, memory,
//!   and query interference for statistics collection operations.
//! - **Configuration Profiles** ([`profiles`]): Pre-configured profiles
//!   for different workload patterns (`RealTime`, `Standard`, `Lazy`, `Stale`,
//!   `Analytical`, `Streaming`).
//!
//! # Design Philosophy
//!
//! The statistics system models real-world database behavior where:
//! - Statistics become stale as data changes
//! - Gathering has measurable cost and interference
//! - Different workloads need different accuracy/cost tradeoffs
//! - Statistics quality affects query plan quality
//!
//! # Examples
//!
//! ## Creating table statistics
//!
//! ```
//! use ra_stats::types::TableStats;
//!
//! let stats = TableStats {
//!     row_count: 1_000_000,
//!     page_count: 10_000,
//!     average_row_size: 100.0,
//!     table_size_bytes: 100_000_000,
//!     live_tuples: Some(950_000),
//!     dead_tuples: Some(50_000),
//!     last_analyzed: Some(1234567890),
//! };
//! ```
//!
//! ## Tracking statistics staleness
//!
//! ```
//! use ra_stats::accuracy::{StatisticsState, StatisticsSource, Staleness};
//!
//! let mut state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
//! assert_eq!(state.staleness(), Staleness::Fresh);
//!
//! state.record_modifications(100_000);
//! assert_eq!(state.staleness(), Staleness::ModeratelyStale);
//! ```
//!
//! ## Estimating gathering cost
//!
//! ```
//! use ra_stats::gathering_cost::{CostEstimator, GatheringMethod};
//!
//! let estimator = CostEstimator::default();
//! let cost = estimator.estimate(
//!     GatheringMethod::BlockSample { sample_rate: 10 },
//!     1_000_000,
//!     10_000,
//! );
//! println!("CPU time: {}ms", cost.cpu_time_ms);
//! ```
//!
//! ## Using configuration profiles
//!
//! ```
//! use ra_stats::profiles::StatisticsProfile;
//!
//! let profile = StatisticsProfile::standard();
//! assert_eq!(profile.name, "Standard");
//! assert!(profile.use_sketches);
//! ```

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::must_use_candidate)]
#![cfg_attr(test, allow(clippy::float_cmp))]

pub mod accuracy;
pub mod delta;
pub mod gathering_cost;
pub mod index_types;
pub mod integration;
pub mod profiles;
pub mod skew;
pub mod timeline;
pub mod types;

pub use accuracy::{QualityMetrics, RefreshThreshold, Staleness, StatisticsSource, StatisticsState};
pub use delta::{DeltaSet, StatisticsDelta};
pub use gathering_cost::{CostEstimator, GatheringCost, GatheringMethod, GatheringPriority};
pub use index_types::{IndexCostFactors, IndexMetadata, IndexType};
pub use profiles::{ProfileSelector, StatisticsProfile};
pub use timeline::{
    PlaybackState, Timeline, TimelineError, TimelineEvent, TimelinePlayer,
};
pub use skew::{
    FrequencyBucket, FrequencyHistogram, HotKey, SkewAnalysis, SkewDetector, SkewSeverity,
    SkewStrategy,
};
pub use types::{
    AccessPattern, ColumnId, ColumnStats, CorrelationStats, FunctionalDependency, Histogram,
    HotColumn, IndexStats, JoinStats, MostCommonValues, MultiColumnNdv, PredicateStats, Sketch,
    TableStats, TimeSeriesStats, WorkloadStats,
};
