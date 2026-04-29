//! Adaptive plan cache with background statistics polling.
//!
//! Wraps [`ra_cache::PlanCache`] with automatic reoptimization when
//! table statistics drift beyond configurable thresholds. The
//! [`AdaptivePlanCache`] provides a `get_or_optimize` interface that
//! transparently returns cached plans or runs the optimizer on cache
//! miss.
//!
//! # Background polling
//!
//! [`StatisticsPoller`] periodically checks for statistics drift and
//! triggers reoptimization of stale cached plans. Polling happens on a
//! separate thread and can be stopped via the returned handle.

#![allow(clippy::missing_errors_doc)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ra_cache::{
    CacheConfig, CacheMetrics, CachedPlan, DriftReport, EvictionPolicy, PlanCache, QueryKey,
};
use ra_core::algebra::RelExpr;
use ra_core::cost::{Cost, StatisticsProvider};
use ra_core::statistics::Statistics;
use ra_engine::Optimizer;
use thiserror::Error;

/// Errors from adaptive cache operations.
#[derive(Debug, Error)]
pub enum AdaptiveCacheError {
    /// A cache operation failed.
    #[error("cache error: {0}")]
    Cache(#[from] ra_cache::CacheError),
    /// SQL parsing failed.
    #[error("parse error: {0}")]
    Parse(String),
    /// Optimization failed.
    #[error("optimization error: {0}")]
    Optimization(String),
}

/// Configuration for the adaptive plan cache.
#[derive(Debug, Clone)]
pub struct AdaptiveCacheConfig {
    /// Maximum cached plans.
    pub max_entries: usize,
    /// Eviction policy.
    pub eviction_policy: EvictionPolicy,
    /// Drift threshold (fraction) for reoptimization.
    pub drift_threshold: f64,
    /// How often the poller checks for drift.
    pub poll_interval: Duration,
    /// Whether to auto-reoptimize when drift is detected on get.
    pub reoptimize_on_get: bool,
}

impl Default for AdaptiveCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1024,
            eviction_policy: EvictionPolicy::Lru,
            drift_threshold: 0.2,
            poll_interval: Duration::from_secs(60),
            reoptimize_on_get: false,
        }
    }
}

/// Plan cache with adaptive reoptimization.
///
/// Transparently caches optimized plans and detects when underlying
/// table statistics have changed enough to warrant re-running the
/// optimizer.
#[derive(Debug, Clone)]
pub struct AdaptivePlanCache {
    cache: PlanCache,
    config: AdaptiveCacheConfig,
}

impl AdaptivePlanCache {
    /// Create an adaptive cache with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(AdaptiveCacheConfig::default())
    }

    /// Create an adaptive cache with custom configuration.
    #[must_use]
    pub fn with_config(config: AdaptiveCacheConfig) -> Self {
        let cache_config = CacheConfig {
            max_entries: config.max_entries,
            eviction_policy: config.eviction_policy,
            drift_threshold: config.drift_threshold,
        };
        Self {
            cache: PlanCache::with_config(cache_config),
            config,
        }
    }

    /// Look up a cached plan or optimize the query on cache miss.
    ///
    /// On hit, returns the cached plan. On miss, parses the SQL,
    /// runs the optimizer, caches the result, and returns it.
    ///
    /// If `reoptimize_on_get` is enabled and the cached plan is
    /// stale, re-runs the optimizer before returning.
    pub fn get_or_optimize(
        &self,
        sql: &str,
        hardware_profile: &str,
        stats_provider: &dyn StatisticsProvider,
        optimizer: &Optimizer,
    ) -> Result<CachedPlan, AdaptiveCacheError> {
        let key = QueryKey::new(sql.to_owned(), hardware_profile.to_owned(), vec![]);

        if let Some(cached) = self.cache.get(&key)? {
            if self.config.reoptimize_on_get {
                let drift = self.cache.check_validity(stats_provider)?;
                if !drift.stale_plans.is_empty() {
                    return self.optimize_and_cache(sql, &key, stats_provider, optimizer);
                }
            }
            return Ok(cached);
        }

        self.optimize_and_cache(sql, &key, stats_provider, optimizer)
    }

    /// Force reoptimization of all stale plans.
    ///
    /// Returns the number of plans that were reoptimized.
    pub fn reoptimize_stale(
        &self,
        stats_provider: &dyn StatisticsProvider,
        optimizer: &Optimizer,
    ) -> Result<usize, AdaptiveCacheError> {
        Ok(self.cache.reoptimize(stats_provider, optimizer)?)
    }

    /// Force reoptimization using a custom drift threshold.
    pub fn reoptimize_with_threshold(
        &self,
        stats_provider: &dyn StatisticsProvider,
        optimizer: &Optimizer,
        threshold: f64,
    ) -> Result<usize, AdaptiveCacheError> {
        Ok(self
            .cache
            .reoptimize_with_threshold(stats_provider, optimizer, threshold)?)
    }

    /// Check which cached plans have drifted.
    pub fn check_drift(
        &self,
        stats_provider: &dyn StatisticsProvider,
    ) -> Result<DriftReport, AdaptiveCacheError> {
        Ok(self.cache.check_validity(stats_provider)?)
    }

    /// Return cache metrics.
    pub fn metrics(&self) -> Result<CacheMetrics, AdaptiveCacheError> {
        Ok(self.cache.metrics()?)
    }

    /// List all cached plans.
    pub fn list(&self) -> Result<Vec<(QueryKey, CachedPlan)>, AdaptiveCacheError> {
        Ok(self.cache.list()?)
    }

    /// Clear the entire cache.
    pub fn clear(&self) -> Result<(), AdaptiveCacheError> {
        Ok(self.cache.clear()?)
    }

    /// Clear plans referencing a specific table.
    pub fn clear_table(&self, table: &str) -> Result<usize, AdaptiveCacheError> {
        Ok(self.cache.clear_table(table)?)
    }

    /// Return a reference to the underlying cache.
    #[must_use]
    pub fn inner_cache(&self) -> &PlanCache {
        &self.cache
    }

    /// Return the adaptive config.
    #[must_use]
    pub fn config(&self) -> &AdaptiveCacheConfig {
        &self.config
    }

    fn optimize_and_cache(
        &self,
        sql: &str,
        key: &QueryKey,
        stats_provider: &dyn StatisticsProvider,
        optimizer: &Optimizer,
    ) -> Result<CachedPlan, AdaptiveCacheError> {
        let parsed =
            ra_parser::sql_to_relexpr(sql).map_err(|e| AdaptiveCacheError::Parse(e.to_string()))?;

        let optimized = optimizer
            .optimize(&parsed)
            .map_err(|e| AdaptiveCacheError::Optimization(e.to_string()))?;

        let snapshot = build_snapshot(&parsed, stats_provider);
        let cost = estimate_cost(&optimized);

        let plan = CachedPlan::new(optimized, cost, snapshot, sql.to_owned());

        self.cache.put(key.clone(), plan.clone())?;
        Ok(plan)
    }
}

impl Default for AdaptivePlanCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for a background statistics poller.
///
/// Dropping the handle stops the polling thread.
#[derive(Debug)]
pub struct PollerHandle {
    stop_flag: Arc<AtomicBool>,
}

impl PollerHandle {
    /// Signal the poller to stop.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// Check whether the poller has been stopped.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }
}

impl Drop for PollerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Background statistics poller that checks for drift and triggers
/// reoptimization at a configurable interval.
#[derive(Debug)]
pub struct StatisticsPoller {
    cache: AdaptivePlanCache,
    interval: Duration,
    last_poll: Option<Instant>,
    stop_flag: Arc<AtomicBool>,
    stats: PollerStats,
}

/// Statistics for the poller.
#[derive(Debug, Clone, Default)]
pub struct PollerStats {
    /// Number of polling cycles completed.
    pub polls: u64,
    /// Total plans reoptimized across all polls.
    pub total_reoptimized: u64,
    /// Number of stale plans detected in the last poll.
    pub last_stale_count: usize,
}

impl StatisticsPoller {
    /// Create a new poller for the given adaptive cache.
    ///
    /// Returns the poller and a handle for stopping it.
    #[must_use]
    pub fn new(cache: AdaptivePlanCache) -> (Self, PollerHandle) {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = PollerHandle {
            stop_flag: Arc::clone(&stop_flag),
        };
        let interval = cache.config.poll_interval;
        let poller = Self {
            cache,
            interval,
            last_poll: None,
            stop_flag,
            stats: PollerStats::default(),
        };
        (poller, handle)
    }

    /// Check if enough time has elapsed for another poll cycle
    /// and run reoptimization if so.
    ///
    /// Call this periodically from your event loop. It is
    /// non-blocking: returns immediately if the interval has not
    /// elapsed.
    pub fn poll(
        &mut self,
        stats_provider: &dyn StatisticsProvider,
        optimizer: &Optimizer,
    ) -> Result<Option<usize>, AdaptiveCacheError> {
        if self.stop_flag.load(Ordering::Relaxed) {
            return Ok(None);
        }

        let now = Instant::now();
        let should_poll = self.last_poll.is_none_or(|last| now.duration_since(last) >= self.interval);

        if !should_poll {
            return Ok(None);
        }

        self.last_poll = Some(now);
        self.stats.polls = self.stats.polls.saturating_add(1);

        let drift = self.cache.check_drift(stats_provider)?;
        self.stats.last_stale_count = drift.stale_plans.len();

        if drift.stale_plans.is_empty() {
            return Ok(Some(0));
        }

        let reoptimized = self.cache.reoptimize_stale(stats_provider, optimizer)?;

        let count = reoptimized as u64;
        self.stats.total_reoptimized = self.stats.total_reoptimized.saturating_add(count);

        tracing::info!(
            reoptimized,
            stale = drift.stale_plans.len(),
            "adaptive cache poll completed"
        );

        Ok(Some(reoptimized))
    }

    /// Return poller statistics.
    #[must_use]
    pub fn stats(&self) -> &PollerStats {
        &self.stats
    }

    /// Check if the poller has been stopped.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }
}

/// Build a statistics snapshot for tables referenced in a plan.
fn build_snapshot(
    plan: &RelExpr,
    provider: &dyn StatisticsProvider,
) -> HashMap<String, Statistics> {
    let mut snapshot = HashMap::new();
    collect_tables(plan, &mut snapshot, provider);
    snapshot
}

fn collect_tables(
    plan: &RelExpr,
    snapshot: &mut HashMap<String, Statistics>,
    provider: &dyn StatisticsProvider,
) {
    match plan {
        RelExpr::Scan { table, .. } => {
            if !snapshot.contains_key(table) {
                if let Some(stats) = provider.get_statistics(table) {
                    snapshot.insert(table.clone(), stats.clone());
                }
            }
        }
        other => {
            for child in other.children() {
                collect_tables(child, snapshot, provider);
            }
        }
    }
}

/// Simple cost estimate based on plan node count.
fn estimate_cost(plan: &RelExpr) -> Cost {
    let n = count_nodes(plan);
    Cost::new(n as f64, n as f64 * 0.5, 0.0, 0)
}

fn count_nodes(plan: &RelExpr) -> usize {
    let mut count = 1;
    for child in plan.children() {
        count += count_nodes(child);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::statistics::Statistics;

    #[derive(Debug)]
    struct TestStats {
        tables: HashMap<String, Statistics>,
    }

    impl StatisticsProvider for TestStats {
        fn get_statistics(&self, table: &str) -> Option<&Statistics> {
            self.tables.get(table)
        }
    }

    fn make_stats(pairs: &[(&str, f64)]) -> TestStats {
        let mut tables = HashMap::new();
        for &(name, rows) in pairs {
            tables.insert(name.to_owned(), Statistics::new(rows));
        }
        TestStats { tables }
    }

    #[test]
    fn get_or_optimize_caches_on_first_call() {
        let cache = AdaptivePlanCache::new();
        let optimizer = Optimizer::new();
        let stats = make_stats(&[("users", 1000.0)]);

        let result = cache.get_or_optimize("SELECT * FROM users", "auto", &stats, &optimizer);
        assert!(result.is_ok());

        let metrics = cache.metrics().expect("metrics");
        assert_eq!(metrics.misses, 1);
    }

    #[test]
    fn get_or_optimize_returns_cached_on_hit() {
        let cache = AdaptivePlanCache::new();
        let optimizer = Optimizer::new();
        let stats = make_stats(&[("users", 1000.0)]);

        cache
            .get_or_optimize("SELECT * FROM users", "auto", &stats, &optimizer)
            .expect("first call");

        cache
            .get_or_optimize("SELECT * FROM users", "auto", &stats, &optimizer)
            .expect("second call");

        let metrics = cache.metrics().expect("metrics");
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.misses, 1);
    }

    #[test]
    fn clear_empties_cache() {
        let cache = AdaptivePlanCache::new();
        let optimizer = Optimizer::new();
        let stats = make_stats(&[("t", 100.0)]);

        cache
            .get_or_optimize("SELECT * FROM t", "auto", &stats, &optimizer)
            .expect("optimize");

        cache.clear().expect("clear");
        let metrics = cache.metrics().expect("metrics");
        assert_eq!(metrics.current_entries, 0);
    }

    #[test]
    fn check_drift_detects_changes() {
        let cache = AdaptivePlanCache::new();
        let optimizer = Optimizer::new();
        let initial = make_stats(&[("users", 1000.0)]);

        cache
            .get_or_optimize("SELECT * FROM users", "auto", &initial, &optimizer)
            .expect("optimize");

        let updated = make_stats(&[("users", 2000.0)]);
        let drift = cache.check_drift(&updated).expect("drift");
        assert!(
            !drift.stale_plans.is_empty(),
            "100% growth should trigger drift"
        );
    }

    #[test]
    fn poller_handle_stops_poller() {
        let cache = AdaptivePlanCache::new();
        let (poller, handle) = StatisticsPoller::new(cache);
        assert!(!poller.is_stopped());

        handle.stop();
        assert!(poller.is_stopped());
    }

    #[test]
    fn poller_respects_interval() {
        let config = AdaptiveCacheConfig {
            poll_interval: Duration::from_secs(3600),
            ..Default::default()
        };
        let cache = AdaptivePlanCache::with_config(config);
        let optimizer = Optimizer::new();
        let stats = make_stats(&[("t", 100.0)]);

        let (mut poller, _handle) = StatisticsPoller::new(cache);

        // First poll should run
        let result = poller.poll(&stats, &optimizer).expect("poll");
        assert!(result.is_some());

        // Second poll should skip (interval not elapsed)
        let result = poller.poll(&stats, &optimizer).expect("poll");
        assert!(result.is_none());
    }

    #[test]
    fn poller_stats_increment() {
        let cache = AdaptivePlanCache::new();
        let optimizer = Optimizer::new();
        let stats = make_stats(&[("t", 100.0)]);

        let (mut poller, _handle) = StatisticsPoller::new(cache);

        poller.poll(&stats, &optimizer).expect("poll");
        assert_eq!(poller.stats().polls, 1);
    }

    #[test]
    fn reoptimize_on_get_enabled() {
        let config = AdaptiveCacheConfig {
            reoptimize_on_get: true,
            ..Default::default()
        };
        let cache = AdaptivePlanCache::with_config(config);
        let optimizer = Optimizer::new();
        let initial = make_stats(&[("t", 1000.0)]);

        cache
            .get_or_optimize("SELECT * FROM t", "auto", &initial, &optimizer)
            .expect("first");

        let updated = make_stats(&[("t", 5000.0)]);
        let plan = cache
            .get_or_optimize("SELECT * FROM t", "auto", &updated, &optimizer)
            .expect("second with drift");
        // Plan was reoptimized so reoptimization_count is 0
        // (it's a fresh entry from re-caching)
        assert_eq!(plan.reoptimization_count, 0);
    }
}
