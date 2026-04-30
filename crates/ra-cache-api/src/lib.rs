//! Plan cache trait definitions and shared types.
//!
//! This crate defines the interface for plan caching without
//! any concrete implementation. Production systems implement
//! the [`PlanCacheApi`] trait with their own storage backend;
//! the `ra-cache-impl` crate provides a reference implementation
//! with LRU/LFU/adaptive eviction.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::missing_errors_doc)]

mod eviction;
mod key;
mod metrics;
mod plan;
mod validity;

pub use eviction::EvictionPolicy;
pub use key::QueryKey;
pub use metrics::CacheMetrics;
pub use plan::CachedPlan;
pub use validity::{DriftDimension, DriftReport, DriftStatus, PlanDrift, TableDrift};

use ra_core::cost::StatisticsProvider;
use thiserror::Error;

/// Errors produced by cache operations.
#[derive(Debug, Error)]
pub enum CacheError {
    /// The optimizer failed during reoptimization.
    #[error("reoptimization failed: {0}")]
    OptimizationFailed(String),
    /// The cache lock was poisoned.
    #[error("cache lock poisoned")]
    LockPoisoned,
}

/// Configuration for a plan cache.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum number of entries the cache can hold.
    pub max_entries: usize,
    /// Eviction policy to use when the cache is full.
    pub eviction_policy: EvictionPolicy,
    /// Statistics drift threshold (fraction, e.g. 0.2 = 20%).
    pub drift_threshold: f64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1024,
            eviction_policy: EvictionPolicy::Lru,
            drift_threshold: 0.2,
        }
    }
}

/// Trait for plan cache implementations.
///
/// Defines the interface that any plan cache must implement.
/// The reference implementation is in `ra-cache-impl`.
pub trait PlanCacheApi: Send + Sync {
    /// Look up a cached plan by key.
    fn get(&self, key: &QueryKey) -> Result<Option<CachedPlan>, CacheError>;

    /// Insert or replace a cached plan.
    fn put(&self, key: QueryKey, plan: CachedPlan) -> Result<(), CacheError>;

    /// Remove a specific entry from the cache.
    fn remove(&self, key: &QueryKey) -> Result<Option<CachedPlan>, CacheError>;

    /// Clear all entries from the cache.
    fn clear(&self) -> Result<(), CacheError>;

    /// Clear entries whose plans reference the given table name.
    fn clear_table(&self, table: &str) -> Result<usize, CacheError>;

    /// Check validity of all cached plans against current statistics.
    fn check_validity(
        &self,
        current_stats: &dyn StatisticsProvider,
    ) -> Result<DriftReport, CacheError>;

    /// Return a list of all cached plans with their keys.
    fn list(&self) -> Result<Vec<(QueryKey, CachedPlan)>, CacheError>;

    /// Return current cache metrics.
    fn metrics(&self) -> Result<CacheMetrics, CacheError>;

    /// Return the number of entries currently in the cache.
    fn len(&self) -> Result<usize, CacheError>;

    /// Return whether the cache is empty.
    fn is_empty(&self) -> Result<bool, CacheError> {
        Ok(self.len()? == 0)
    }
}
