#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]
//! Reference plan cache implementation with LRU/LFU/adaptive eviction
//! and statistics-driven reoptimization.
//!
//! This crate provides a concrete [`PlanCache`] backed by an in-memory
//! `HashMap` with configurable eviction and drift detection. For the
//! trait definition and shared types, see `ra-cache-api`.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::missing_errors_doc)]

mod eviction;
mod validity;

// Re-export all API types for convenience.
pub use ra_cache_api::{
    CacheConfig, CacheError, CachedPlan, DriftDimension, DriftReport, DriftStatus, EvictionPolicy,
    PlanCacheApi, PlanDrift, QueryKey, TableDrift,
};
pub use ra_cache_api::CacheMetrics;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ra_core::algebra::RelExpr;
use ra_core::cost::{Cost, StatisticsProvider};
use ra_core::statistics::Statistics;
use ra_engine::Optimizer;

/// Thread-safe plan cache with configurable eviction.
///
/// Stores optimized [`RelExpr`] plans keyed by [`QueryKey`]. Each
/// entry records its optimization cost, the statistics snapshot at
/// optimization time, and access metadata used for eviction.
#[derive(Debug)]
pub struct PlanCache {
    inner: Arc<Mutex<CacheInner>>,
    config: CacheConfig,
}

#[derive(Debug)]
struct CacheInner {
    entries: HashMap<QueryKey, CachedPlan>,
    metrics: CacheMetrics,
    insertion_order: Vec<QueryKey>,
}

impl PlanCache {
    /// Create a new plan cache with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    /// Create a new plan cache with the given configuration.
    #[must_use]
    pub fn with_config(config: CacheConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CacheInner {
                entries: HashMap::new(),
                metrics: CacheMetrics::new(),
                insertion_order: Vec::new(),
            })),
            config,
        }
    }

    /// Look up a cached plan by key.
    ///
    /// Returns `None` on miss. On hit, updates access metadata
    /// (last-accessed time, use count) for eviction tracking.
    pub fn get(&self, key: &QueryKey) -> Result<Option<CachedPlan>, CacheError> {
        let mut inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        if inner.entries.contains_key(key) {
            if let Some(entry) = inner.entries.get_mut(key) {
                entry.last_accessed = Instant::now();
                entry.use_count = entry.use_count.saturating_add(1);
            }
            let cloned = inner.entries.get(key).cloned();
            inner.metrics.record_hit();
            Ok(cloned)
        } else {
            inner.metrics.record_miss();
            Ok(None)
        }
    }

    /// Insert or replace a cached plan.
    ///
    /// If the cache is at capacity, evicts one entry according to
    /// the configured [`EvictionPolicy`] before inserting.
    pub fn put(&self, key: QueryKey, plan: CachedPlan) -> Result<(), CacheError> {
        let mut inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;

        if inner.entries.len() >= self.config.max_entries && !inner.entries.contains_key(&key) {
            self.evict_one(&mut inner);
        }

        if !inner.entries.contains_key(&key) {
            inner.insertion_order.push(key.clone());
        }
        inner.entries.insert(key, plan);
        Ok(())
    }

    /// Remove a specific entry from the cache.
    pub fn remove(&self, key: &QueryKey) -> Result<Option<CachedPlan>, CacheError> {
        let mut inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        let removed = inner.entries.remove(key);
        if removed.is_some() {
            inner.insertion_order.retain(|k| k != key);
        }
        Ok(removed)
    }

    /// Clear all entries from the cache.
    pub fn clear(&self) -> Result<(), CacheError> {
        let mut inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        inner.entries.clear();
        inner.insertion_order.clear();
        inner.metrics.record_clear();
        Ok(())
    }

    /// Clear entries whose plans reference the given table name.
    pub fn clear_table(&self, table: &str) -> Result<usize, CacheError> {
        let mut inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        let before = inner.entries.len();
        inner
            .entries
            .retain(|_, plan| !plan.references_table(table));
        let removed = before - inner.entries.len();
        let remaining_keys: std::collections::HashSet<_> = inner.entries.keys().cloned().collect();
        inner.insertion_order.retain(|k| remaining_keys.contains(k));
        Ok(removed)
    }

    /// Check validity of all cached plans against current statistics.
    ///
    /// Returns a drift report indicating which plans are stale and
    /// which tables have drifted.
    pub fn check_validity(
        &self,
        current_stats: &dyn StatisticsProvider,
    ) -> Result<DriftReport, CacheError> {
        let inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        let mut report = DriftReport::new();

        for (key, plan) in &inner.entries {
            let drift =
                validity::check_plan_drift(plan, current_stats, self.config.drift_threshold);
            if drift.status != DriftStatus::Fresh {
                report.stale_plans.push((key.clone(), drift));
            }
        }

        Ok(report)
    }

    /// Reoptimize all stale plans in the cache.
    ///
    /// Plans whose referenced tables have statistics drift exceeding
    /// the configured threshold are re-run through the optimizer with
    /// current statistics. Returns the number of plans reoptimized.
    pub fn reoptimize(
        &self,
        current_stats: &dyn StatisticsProvider,
        optimizer: &Optimizer,
    ) -> Result<usize, CacheError> {
        self.reoptimize_with_threshold(current_stats, optimizer, self.config.drift_threshold)
    }

    /// Reoptimize stale plans using a custom drift threshold.
    pub fn reoptimize_with_threshold(
        &self,
        current_stats: &dyn StatisticsProvider,
        optimizer: &Optimizer,
        threshold: f64,
    ) -> Result<usize, CacheError> {
        let stale_keys = self.find_stale_keys(current_stats, threshold)?;

        let mut reoptimized = 0;
        for key in &stale_keys {
            if self.try_reoptimize_entry(key, current_stats, optimizer)? {
                reoptimized += 1;
            }
        }

        Ok(reoptimized)
    }

    /// Return a list of all cached plans with their keys.
    pub fn list(&self) -> Result<Vec<(QueryKey, CachedPlan)>, CacheError> {
        let inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        Ok(inner
            .entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    /// Return current cache metrics.
    pub fn metrics(&self) -> Result<CacheMetrics, CacheError> {
        let inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        let mut m = inner.metrics.clone();
        m.current_entries = inner.entries.len();
        m.max_entries = self.config.max_entries;
        Ok(m)
    }

    /// Return the number of entries currently in the cache.
    pub fn len(&self) -> Result<usize, CacheError> {
        let inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        Ok(inner.entries.len())
    }

    /// Return whether the cache is empty.
    pub fn is_empty(&self) -> Result<bool, CacheError> {
        Ok(self.len()? == 0)
    }

    fn find_stale_keys(
        &self,
        current_stats: &dyn StatisticsProvider,
        threshold: f64,
    ) -> Result<Vec<QueryKey>, CacheError> {
        let inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        let mut keys = Vec::new();
        for (key, plan) in &inner.entries {
            let drift = validity::check_plan_drift(plan, current_stats, threshold);
            if drift.status != DriftStatus::Fresh {
                keys.push(key.clone());
            }
        }
        Ok(keys)
    }

    fn try_reoptimize_entry(
        &self,
        key: &QueryKey,
        current_stats: &dyn StatisticsProvider,
        optimizer: &Optimizer,
    ) -> Result<bool, CacheError> {
        let sql = {
            let inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
            inner.entries.get(key).map(|e| e.original_sql.clone())
        };

        let Some(sql) = sql else {
            return Ok(false);
        };

        let Ok(parsed_plan) = ra_parser::sql_to_relexpr(&sql) else {
            return Ok(false);
        };

        match optimizer.optimize(&parsed_plan) {
            Ok(new_plan) => {
                let new_snapshot = build_snapshot(&parsed_plan, current_stats);
                let new_cost = estimate_simple_cost(&new_plan);
                self.update_entry(key, new_plan, new_cost, new_snapshot)?;
                Ok(true)
            }
            Err(e) => {
                tracing::warn!(
                    sql = %sql,
                    error = %e,
                    "reoptimization failed for cached plan"
                );
                Ok(false)
            }
        }
    }

    fn update_entry(
        &self,
        key: &QueryKey,
        new_plan: RelExpr,
        new_cost: Cost,
        new_snapshot: HashMap<String, Statistics>,
    ) -> Result<(), CacheError> {
        let mut inner = self.inner.lock().map_err(|_| CacheError::LockPoisoned)?;
        if let Some(entry) = inner.entries.get_mut(key) {
            entry.plan = new_plan;
            entry.cost = new_cost;
            entry.statistics_snapshot = new_snapshot;
            entry.optimized_at = Instant::now();
            entry.reoptimization_count = entry.reoptimization_count.saturating_add(1);
        }
        Ok(())
    }

    fn evict_one(&self, inner: &mut CacheInner) {
        if inner.entries.is_empty() {
            return;
        }

        let victim_key = match self.config.eviction_policy {
            EvictionPolicy::Lru => eviction::find_lru_victim(&inner.entries),
            EvictionPolicy::Lfu => eviction::find_lfu_victim(&inner.entries),
            EvictionPolicy::Adaptive => eviction::find_adaptive_victim(&inner.entries),
        };

        if let Some(key) = victim_key {
            inner.entries.remove(&key);
            inner.insertion_order.retain(|k| k != &key);
            inner.metrics.record_eviction();
        }
    }
}

impl Default for PlanCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for PlanCache {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            config: self.config.clone(),
        }
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

/// Simple cost estimate (sum of CPU + IO components).
fn estimate_simple_cost(plan: &RelExpr) -> Cost {
    let node_count = count_nodes(plan);
    Cost::new(node_count as f64, node_count as f64 * 0.5, 0.0, 0)
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
    struct TestStatsProvider {
        tables: HashMap<String, Statistics>,
    }

    impl StatisticsProvider for TestStatsProvider {
        fn get_statistics(&self, table: &str) -> Option<&Statistics> {
            self.tables.get(table)
        }
    }

    fn make_provider(pairs: &[(&str, f64)]) -> TestStatsProvider {
        let mut tables = HashMap::new();
        for &(name, rows) in pairs {
            tables.insert(name.to_owned(), Statistics::new(rows));
        }
        TestStatsProvider { tables }
    }

    fn make_key(sql: &str) -> QueryKey {
        QueryKey::new(sql.to_owned(), "auto".to_owned(), vec![])
    }

    fn make_plan(sql: &str, tables: &[(&str, f64)]) -> CachedPlan {
        let plan = RelExpr::scan(tables[0].0);
        let mut snapshot = HashMap::new();
        for &(name, rows) in tables {
            snapshot.insert(name.to_owned(), Statistics::new(rows));
        }
        CachedPlan::new(
            plan,
            Cost::new(10.0, 5.0, 0.0, 1024),
            snapshot,
            sql.to_owned(),
        )
    }

    #[test]
    fn put_and_get() {
        let cache = PlanCache::new();
        let key = make_key("SELECT * FROM users");
        let plan = make_plan("SELECT * FROM users", &[("users", 1000.0)]);

        cache.put(key.clone(), plan).expect("put should succeed");
        let fetched = cache
            .get(&key)
            .expect("get should succeed")
            .expect("entry should exist");
        assert_eq!(fetched.use_count, 1);
    }

    #[test]
    fn miss_returns_none() {
        let cache = PlanCache::new();
        let key = make_key("SELECT 1");
        let result = cache.get(&key).expect("get should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn use_count_increments() {
        let cache = PlanCache::new();
        let key = make_key("SELECT * FROM t");
        let plan = make_plan("SELECT * FROM t", &[("t", 100.0)]);

        cache.put(key.clone(), plan).expect("put should succeed");
        cache.get(&key).expect("get should succeed");
        cache.get(&key).expect("get should succeed");
        let entry = cache
            .get(&key)
            .expect("get should succeed")
            .expect("should exist");
        assert_eq!(entry.use_count, 3);
    }

    #[test]
    fn clear_empties_cache() {
        let cache = PlanCache::new();
        let key = make_key("SELECT 1");
        let plan = make_plan("SELECT 1", &[("t", 100.0)]);

        cache.put(key, plan).expect("put should succeed");
        assert_eq!(cache.len().expect("len"), 1);

        cache.clear().expect("clear should succeed");
        assert!(cache.is_empty().expect("is_empty"));
    }

    #[test]
    fn clear_table_removes_matching() {
        let cache = PlanCache::new();
        let k1 = make_key("SELECT * FROM users");
        let k2 = make_key("SELECT * FROM orders");
        let p1 = make_plan("SELECT * FROM users", &[("users", 1000.0)]);
        let p2 = make_plan("SELECT * FROM orders", &[("orders", 500.0)]);

        cache.put(k1.clone(), p1).expect("put");
        cache.put(k2.clone(), p2).expect("put");
        assert_eq!(cache.len().expect("len"), 2);

        let removed = cache.clear_table("users").expect("clear_table");
        assert_eq!(removed, 1);
        assert_eq!(cache.len().expect("len"), 1);
    }

    #[test]
    fn eviction_lru() {
        let config = CacheConfig {
            max_entries: 2,
            eviction_policy: EvictionPolicy::Lru,
            drift_threshold: 0.2,
        };
        let cache = PlanCache::with_config(config);

        let k1 = make_key("q1");
        let k2 = make_key("q2");
        let k3 = make_key("q3");
        let p1 = make_plan("q1", &[("t1", 100.0)]);
        let p2 = make_plan("q2", &[("t2", 200.0)]);
        let p3 = make_plan("q3", &[("t3", 300.0)]);

        cache.put(k1.clone(), p1).expect("put");
        cache.put(k2.clone(), p2).expect("put");

        // Access k1 so k2 becomes least-recently-used
        cache.get(&k1).expect("get");

        // k3 should evict k2
        cache.put(k3.clone(), p3).expect("put");
        assert_eq!(cache.len().expect("len"), 2);
        assert!(
            cache.get(&k2).expect("get").is_none(),
            "k2 should have been evicted"
        );
        assert!(cache.get(&k1).expect("get").is_some());
        assert!(cache.get(&k3).expect("get").is_some());
    }

    #[test]
    fn eviction_lfu() {
        let config = CacheConfig {
            max_entries: 2,
            eviction_policy: EvictionPolicy::Lfu,
            drift_threshold: 0.2,
        };
        let cache = PlanCache::with_config(config);

        let k1 = make_key("q1");
        let k2 = make_key("q2");
        let k3 = make_key("q3");
        let p1 = make_plan("q1", &[("t1", 100.0)]);
        let p2 = make_plan("q2", &[("t2", 200.0)]);
        let p3 = make_plan("q3", &[("t3", 300.0)]);

        cache.put(k1.clone(), p1).expect("put");
        cache.put(k2.clone(), p2).expect("put");

        // Access k1 multiple times to raise its frequency
        cache.get(&k1).expect("get");
        cache.get(&k1).expect("get");
        cache.get(&k1).expect("get");
        // k2 only accessed once (via get to confirm it exists)
        cache.get(&k2).expect("get");

        // k3 should evict k2 (lower frequency)
        cache.put(k3.clone(), p3).expect("put");
        assert_eq!(cache.len().expect("len"), 2);
        assert!(
            cache.get(&k2).expect("get").is_none(),
            "k2 should have been evicted (lower frequency)"
        );
    }

    #[test]
    fn check_validity_detects_drift() {
        let cache = PlanCache::new();
        let key = make_key("SELECT * FROM users");
        let plan = make_plan("SELECT * FROM users", &[("users", 1000.0)]);
        cache.put(key, plan).expect("put");

        // Current stats show users grew 50% (1000 -> 1500)
        let provider = make_provider(&[("users", 1500.0)]);
        let report = cache.check_validity(&provider).expect("check");
        assert!(
            !report.stale_plans.is_empty(),
            "50% drift should exceed 20% threshold"
        );
    }

    #[test]
    fn check_validity_fresh_within_threshold() {
        let cache = PlanCache::new();
        let key = make_key("SELECT * FROM users");
        let plan = make_plan("SELECT * FROM users", &[("users", 1000.0)]);
        cache.put(key, plan).expect("put");

        // Only 10% change -- within threshold
        let provider = make_provider(&[("users", 1100.0)]);
        let report = cache.check_validity(&provider).expect("check");
        assert!(
            report.stale_plans.is_empty(),
            "10% drift should be within 20% threshold"
        );
    }

    #[test]
    fn metrics_tracking() {
        let cache = PlanCache::new();
        let key = make_key("SELECT 1");
        let plan = make_plan("SELECT 1", &[("t", 100.0)]);

        cache.get(&key).expect("miss");
        cache.put(key.clone(), plan).expect("put");
        cache.get(&key).expect("hit");

        let m = cache.metrics().expect("metrics");
        assert_eq!(m.hits, 1);
        assert_eq!(m.misses, 1);
        assert_eq!(m.current_entries, 1);
    }

    #[test]
    fn list_returns_all_entries() {
        let cache = PlanCache::new();
        let k1 = make_key("q1");
        let k2 = make_key("q2");
        cache
            .put(k1, make_plan("q1", &[("t1", 100.0)]))
            .expect("put");
        cache
            .put(k2, make_plan("q2", &[("t2", 200.0)]))
            .expect("put");

        let entries = cache.list().expect("list");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn remove_entry() {
        let cache = PlanCache::new();
        let key = make_key("SELECT 1");
        let plan = make_plan("SELECT 1", &[("t", 100.0)]);

        cache.put(key.clone(), plan).expect("put");
        let removed = cache.remove(&key).expect("remove");
        assert!(removed.is_some());
        assert!(cache.is_empty().expect("is_empty"));
    }
}
