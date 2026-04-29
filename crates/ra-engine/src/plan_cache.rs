//! Fingerprint-based plan cache for query plan reuse (RFC 0060).
//!
//! Caches optimized [`RelExpr`] plans keyed by [`QueryFingerprint`].
//! Queries that differ only in literal values share the same
//! fingerprint and can reuse a cached plan, avoiding redundant
//! equality saturation passes.
//!
//! The cache uses LRU eviction with similarity clustering: when the
//! cache is full, the least-recently-used entry is evicted. Lookup
//! supports both exact fingerprint matches and fuzzy similarity
//! matches above a configurable threshold.
//!
//! RFC 0059 adds event-driven invalidation: plans can be
//! hard-evicted or soft-invalidated (marked stale) when statistics
//! change, without per-access staleness checking.

use std::collections::HashMap;

use ra_core::algebra::RelExpr;

#[cfg(feature = "streaming")]
use crate::differential::PlanDependencies;
use crate::genetic_fingerprint::QueryFingerprint;
#[cfg(not(feature = "streaming"))]
use std::collections::HashSet;

#[cfg(not(feature = "streaming"))]
#[derive(Debug, Clone, PartialEq)]
pub struct PlanDependencies {
    pub table_cardinalities: HashMap<String, f64>,
    pub indexes: HashSet<(String, String)>,
    pub distinct_counts: HashMap<(String, String), f64>,
    pub rules: HashSet<String>,
}

/// Configuration for the plan cache.
#[derive(Debug, Clone)]
pub struct PlanCacheConfig {
    /// Maximum number of cached plans.
    pub max_entries: usize,
    /// Minimum similarity score for a fuzzy cache hit.
    /// Range: 0.0..=1.0. Default: 0.9 (90% similarity).
    pub similarity_threshold: f64,
    /// Whether to enable fuzzy (similarity-based) matching
    /// in addition to exact fingerprint matching.
    pub enable_fuzzy_matching: bool,
    /// Hit count threshold above which soft invalidation
    /// (mark stale) is preferred over hard eviction.
    /// Default: 100.
    pub soft_invalidation_hit_threshold: u64,
}

impl Default for PlanCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1024,
            similarity_threshold: 0.9,
            enable_fuzzy_matching: true,
            soft_invalidation_hit_threshold: 100,
        }
    }
}

/// A cached plan entry with metadata.
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The fingerprint that produced this plan.
    fingerprint: QueryFingerprint,
    /// The optimized plan.
    plan: RelExpr,
    /// Monotonic counter for LRU tracking.
    last_access: u64,
    /// Number of cache hits for this entry.
    hit_count: u64,
    /// Statistics dependencies for this plan (RFC 0059).
    dependencies: Option<PlanDependencies>,
    /// Whether this entry has been soft-invalidated (RFC 0059).
    stale: bool,
}

/// Result of a plan cache lookup.
#[derive(Debug, Clone)]
pub struct CacheLookupResult {
    /// The cached plan.
    pub plan: RelExpr,
    /// Whether this was an exact or fuzzy match.
    pub match_type: CacheMatchType,
    /// Similarity score (1.0 for exact, <1.0 for fuzzy).
    pub similarity: f64,
}

/// How a cache hit was matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMatchType {
    /// Exact fingerprint match.
    Exact,
    /// Fuzzy match above the similarity threshold.
    Fuzzy,
    /// Entry found but soft-invalidated (RFC 0059). The caller
    /// should attempt streaming adjustment before using the plan.
    Stale,
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct PlanCacheStats {
    /// Total lookup attempts.
    pub lookups: u64,
    /// Exact fingerprint hits.
    pub exact_hits: u64,
    /// Fuzzy similarity hits.
    pub fuzzy_hits: u64,
    /// Cache misses.
    pub misses: u64,
    /// Number of evictions performed.
    pub evictions: u64,
    /// Current number of entries in the cache.
    pub current_entries: usize,
    /// Plans evicted by differential invalidation (RFC 0059).
    pub hard_invalidations: u64,
    /// Plans marked stale for adjustment (RFC 0059).
    pub soft_invalidations: u64,
}

impl PlanCacheStats {
    /// Overall cache hit rate (exact + fuzzy).
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            return 0.0;
        }
        let hits = (self.exact_hits + self.fuzzy_hits) as f64;
        let total = self.lookups as f64;
        hits / total
    }
}

/// LRU plan cache keyed by query fingerprints.
pub struct PlanCache {
    config: PlanCacheConfig,
    /// Primary index: exact fingerprint -> entry index.
    exact_index: HashMap<QueryFingerprint, usize>,
    /// All entries, indexed by position.
    entries: Vec<CacheEntry>,
    /// Monotonic access counter for LRU.
    access_counter: u64,
    /// Accumulated statistics.
    stats: PlanCacheStats,
}

impl PlanCache {
    /// Create a new plan cache with the given configuration.
    #[must_use]
    pub fn new(config: PlanCacheConfig) -> Self {
        Self {
            exact_index: HashMap::with_capacity(config.max_entries),
            entries: Vec::with_capacity(config.max_entries),
            access_counter: 0,
            stats: PlanCacheStats::default(),
            config,
        }
    }

    /// Create a plan cache with default configuration.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(PlanCacheConfig::default())
    }

    /// Look up a plan by fingerprint.
    ///
    /// Returns `None` on cache miss. If the entry is
    /// soft-invalidated, returns `CacheMatchType::Stale`.
    pub fn lookup(&mut self, fingerprint: &QueryFingerprint) -> Option<CacheLookupResult> {
        self.stats.lookups += 1;

        if let Some(&idx) = self.exact_index.get(fingerprint) {
            self.access_counter += 1;
            self.entries[idx].last_access = self.access_counter;
            self.entries[idx].hit_count += 1;
            let match_type = if self.entries[idx].stale {
                CacheMatchType::Stale
            } else {
                CacheMatchType::Exact
            };
            self.stats.exact_hits += 1;
            return Some(CacheLookupResult {
                plan: self.entries[idx].plan.clone(),
                match_type,
                similarity: 1.0,
            });
        }

        if self.config.enable_fuzzy_matching {
            if let Some(result) = self.fuzzy_lookup(fingerprint) {
                self.stats.fuzzy_hits += 1;
                return Some(result);
            }
        }

        self.stats.misses += 1;
        None
    }

    /// Insert a plan into the cache.
    pub fn insert(&mut self, fingerprint: QueryFingerprint, plan: RelExpr) {
        if let Some(&idx) = self.exact_index.get(&fingerprint) {
            self.access_counter += 1;
            self.entries[idx].plan = plan;
            self.entries[idx].last_access = self.access_counter;
            self.entries[idx].stale = false;
            return;
        }

        if self.entries.len() >= self.config.max_entries {
            self.evict_lru();
        }

        self.access_counter += 1;
        let idx = self.entries.len();
        self.exact_index.insert(fingerprint.clone(), idx);
        self.entries.push(CacheEntry {
            fingerprint,
            plan,
            last_access: self.access_counter,
            hit_count: 0,
            dependencies: None,
            stale: false,
        });
        self.stats.current_entries = self.entries.len();
    }

    /// Insert a plan with its statistics dependencies (RFC 0059).
    pub fn insert_with_deps(
        &mut self,
        fingerprint: QueryFingerprint,
        plan: RelExpr,
        deps: PlanDependencies,
    ) {
        if let Some(&idx) = self.exact_index.get(&fingerprint) {
            self.access_counter += 1;
            self.entries[idx].plan = plan;
            self.entries[idx].last_access = self.access_counter;
            self.entries[idx].dependencies = Some(deps);
            self.entries[idx].stale = false;
            return;
        }

        if self.entries.len() >= self.config.max_entries {
            self.evict_lru();
        }

        self.access_counter += 1;
        let idx = self.entries.len();
        self.exact_index.insert(fingerprint.clone(), idx);
        self.entries.push(CacheEntry {
            fingerprint,
            plan,
            last_access: self.access_counter,
            hit_count: 0,
            dependencies: Some(deps),
            stale: false,
        });
        self.stats.current_entries = self.entries.len();
    }

    /// Invalidate specific plans by fingerprint (RFC 0059).
    ///
    /// Hot entries (high hit count) are soft-invalidated;
    /// cold entries are hard-evicted.
    pub fn invalidate(&mut self, fingerprints: &[QueryFingerprint]) {
        let threshold = self.config.soft_invalidation_hit_threshold;
        for fp in fingerprints {
            if let Some(&idx) = self.exact_index.get(fp) {
                if self.entries[idx].hit_count >= threshold {
                    self.entries[idx].stale = true;
                    self.stats.soft_invalidations += 1;
                } else {
                    self.evict_entry(fp);
                    self.stats.hard_invalidations += 1;
                }
            }
        }
    }

    /// Invalidate all plans that depend on a table (RFC 0059).
    pub fn invalidate_for_table(&mut self, table: &str) {
        let affected: Vec<QueryFingerprint> = self
            .entries
            .iter()
            .filter(|e| {
                e.dependencies
                    .as_ref()
                    .is_some_and(|d| d.table_cardinalities.contains_key(table))
            })
            .map(|e| e.fingerprint.clone())
            .collect();
        self.invalidate(&affected);
    }

    /// Get the dependencies for a cached entry.
    #[must_use]
    pub fn get_dependencies(&self, fingerprint: &QueryFingerprint) -> Option<&PlanDependencies> {
        self.exact_index
            .get(fingerprint)
            .and_then(|&idx| self.entries[idx].dependencies.as_ref())
    }

    /// Mark a stale entry as fresh again.
    pub fn mark_fresh(&mut self, fingerprint: &QueryFingerprint) {
        if let Some(&idx) = self.exact_index.get(fingerprint) {
            self.entries[idx].stale = false;
        }
    }

    /// Remove all entries from the cache.
    pub fn clear(&mut self) {
        self.exact_index.clear();
        self.entries.clear();
        self.stats.current_entries = 0;
    }

    /// Return current cache statistics.
    #[must_use]
    pub fn stats(&self) -> &PlanCacheStats {
        &self.stats
    }

    /// Number of entries currently in the cache.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn evict_entry(&mut self, fingerprint: &QueryFingerprint) {
        if let Some(&idx) = self.exact_index.get(fingerprint) {
            self.exact_index.remove(fingerprint);
            self.entries.swap_remove(idx);
            if idx < self.entries.len() {
                let moved_fp = self.entries[idx].fingerprint.clone();
                self.exact_index.insert(moved_fp, idx);
            }
            self.stats.evictions += 1;
            self.stats.current_entries = self.entries.len();
        }
    }

    fn fuzzy_lookup(&mut self, fingerprint: &QueryFingerprint) -> Option<CacheLookupResult> {
        let mut best_idx: Option<usize> = None;
        let mut best_similarity: f64 = 0.0;

        for (idx, entry) in self.entries.iter().enumerate() {
            let sim = fingerprint.similarity(&entry.fingerprint);
            if sim >= self.config.similarity_threshold && sim > best_similarity {
                best_similarity = sim;
                best_idx = Some(idx);
            }
        }

        if let Some(idx) = best_idx {
            self.access_counter += 1;
            self.entries[idx].last_access = self.access_counter;
            self.entries[idx].hit_count += 1;
            Some(CacheLookupResult {
                plan: self.entries[idx].plan.clone(),
                match_type: CacheMatchType::Fuzzy,
                similarity: best_similarity,
            })
        } else {
            None
        }
    }

    fn evict_lru(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        #[expect(clippy::expect_used, reason = "guarded by is_empty check above")]
        let lru_idx = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.last_access)
            .map(|(i, _)| i)
            .expect("entries is non-empty");

        self.exact_index.remove(&self.entries[lru_idx].fingerprint);
        self.entries.swap_remove(lru_idx);

        if lru_idx < self.entries.len() {
            let moved_fp = self.entries[lru_idx].fingerprint.clone();
            self.exact_index.insert(moved_fp, lru_idx);
        }

        self.stats.evictions += 1;
        self.stats.current_entries = self.entries.len();
    }
}

impl std::fmt::Debug for PlanCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanCache")
            .field("entries", &self.entries.len())
            .field("max_entries", &self.config.max_entries)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::JoinType;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn make_scan_filter(table: &str, col: &str, value: i64) -> RelExpr {
        RelExpr::scan(table).filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new(col))),
            right: Box::new(Expr::Const(Const::Int(value))),
        })
    }

    fn make_join_query(value: i64) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("o", "uid"))),
            },
            left: Box::new(make_scan_filter("users", "age", value)),
            right: Box::new(RelExpr::scan("orders")),
        }
    }

    // ── Basic cache operations ──────────────────────────────

    #[test]
    fn insert_and_exact_lookup() {
        let mut cache = PlanCache::with_defaults();
        let plan = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan);

        cache.insert(fp.clone(), plan.clone());
        let result = cache.lookup(&fp);

        assert!(result.is_some());
        let hit = result.expect("cache should hit");
        assert_eq!(hit.match_type, CacheMatchType::Exact);
        assert!((hit.similarity - 1.0).abs() < f64::EPSILON);
        assert_eq!(hit.plan, plan);
    }

    #[test]
    fn miss_on_empty_cache() {
        let mut cache = PlanCache::with_defaults();
        let plan = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        assert!(cache.lookup(&fp).is_none());
    }

    #[test]
    fn parameter_variation_hits_cache() {
        let mut cache = PlanCache::with_defaults();
        let plan1 = make_scan_filter("users", "age", 18);
        let fp1 = QueryFingerprint::from_rel_expr(&plan1);
        cache.insert(fp1, plan1);

        let plan2 = make_scan_filter("users", "age", 65);
        let fp2 = QueryFingerprint::from_rel_expr(&plan2);
        let result = cache.lookup(&fp2);

        assert!(result.is_some());
        let hit = result.expect("parameter variation should hit");
        assert_eq!(hit.match_type, CacheMatchType::Exact);
    }

    // ── LRU eviction ────────────────────────────────────────

    #[test]
    fn lru_eviction_at_capacity() {
        let config = PlanCacheConfig {
            max_entries: 3,
            ..PlanCacheConfig::default()
        };
        let mut cache = PlanCache::new(config);

        for table in &["t1", "t2", "t3"] {
            let plan = RelExpr::scan(*table);
            let fp = QueryFingerprint::from_rel_expr(&plan);
            cache.insert(fp, plan);
        }
        assert_eq!(cache.len(), 3);

        let fp2 = QueryFingerprint::from_rel_expr(&RelExpr::scan("t2"));
        let fp3 = QueryFingerprint::from_rel_expr(&RelExpr::scan("t3"));
        let _ = cache.lookup(&fp2);
        let _ = cache.lookup(&fp3);

        let plan4 = RelExpr::scan("t4");
        let fp4 = QueryFingerprint::from_rel_expr(&plan4);
        cache.insert(fp4.clone(), plan4);

        assert_eq!(cache.len(), 3);
        assert!(cache.stats().evictions >= 1);

        let fp1 = QueryFingerprint::from_rel_expr(&RelExpr::scan("t1"));
        assert!(cache.lookup(&fp1).is_none());
        assert!(cache.lookup(&fp4).is_some());
    }

    // ── Statistics tracking ─────────────────────────────────

    #[test]
    fn stats_tracking() {
        let mut cache = PlanCache::with_defaults();
        let plan = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan);

        let _ = cache.lookup(&fp);
        assert_eq!(cache.stats().lookups, 1);
        assert_eq!(cache.stats().misses, 1);

        cache.insert(fp.clone(), plan);
        let _ = cache.lookup(&fp);
        assert_eq!(cache.stats().lookups, 2);
        assert_eq!(cache.stats().exact_hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn hit_rate_calculation() {
        let mut cache = PlanCache::with_defaults();
        let plan = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan);

        cache.insert(fp.clone(), plan);

        let different = RelExpr::scan("nonexistent");
        let dfp = QueryFingerprint::from_rel_expr(&different);
        let _ = cache.lookup(&dfp);
        let _ = cache.lookup(&fp);
        let _ = cache.lookup(&fp);
        let _ = cache.lookup(&fp);

        let rate = cache.stats().hit_rate();
        assert!((rate - 0.75).abs() < f64::EPSILON);
    }

    // ── Update existing entry ───────────────────────────────

    #[test]
    fn insert_same_fingerprint_updates_plan() {
        let mut cache = PlanCache::with_defaults();
        let plan1 = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan1);
        cache.insert(fp.clone(), plan1);

        let plan2 = make_scan_filter("users", "age", 99);
        cache.insert(fp.clone(), plan2.clone());

        assert_eq!(cache.len(), 1);
        let hit = cache.lookup(&fp).expect("should hit");
        assert_eq!(hit.plan, plan2);
    }

    // ── Clear ───────────────────────────────────────────────

    #[test]
    fn clear_empties_cache() {
        let mut cache = PlanCache::with_defaults();
        let plan = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp, plan);
        assert!(!cache.is_empty());

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    // ── Workload test ───────────────────────────────────────

    #[test]
    fn workload_with_parameter_variations_high_hit_rate() {
        let mut cache = PlanCache::with_defaults();
        let base = make_join_query(1000);
        let fp = QueryFingerprint::from_rel_expr(&base);
        cache.insert(fp, base);

        let mut hits = 0_u32;
        for i in 0..20 {
            let q = make_join_query(i * 100 + 50);
            let qfp = QueryFingerprint::from_rel_expr(&q);
            if cache.lookup(&qfp).is_some() {
                hits += 1;
            }
        }

        assert_eq!(hits, 20);
        assert!(cache.stats().hit_rate() > 0.9);
    }

    // ── RFC 0059: Invalidation tests ────────────────────────

    #[test]
    fn hard_invalidation_evicts_cold_entry() {
        let mut cache = PlanCache::with_defaults();
        let plan = RelExpr::scan("orders");
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp.clone(), plan);

        cache.invalidate(std::slice::from_ref(&fp));
        assert!(cache.lookup(&fp).is_none());
        assert_eq!(cache.stats().hard_invalidations, 1);
    }

    #[test]
    fn soft_invalidation_marks_hot_entry_stale() {
        let config = PlanCacheConfig {
            soft_invalidation_hit_threshold: 3,
            ..PlanCacheConfig::default()
        };
        let mut cache = PlanCache::new(config);
        let plan = RelExpr::scan("users");
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp.clone(), plan);

        // Build up hit count above threshold
        let _ = cache.lookup(&fp);
        let _ = cache.lookup(&fp);
        let _ = cache.lookup(&fp);

        cache.invalidate(std::slice::from_ref(&fp));

        // Entry still exists but marked stale
        let result = cache.lookup(&fp).expect("should still hit");
        assert_eq!(result.match_type, CacheMatchType::Stale);
        assert_eq!(cache.stats().soft_invalidations, 1);
    }

    #[test]
    fn mark_fresh_clears_stale() {
        let config = PlanCacheConfig {
            soft_invalidation_hit_threshold: 1,
            ..PlanCacheConfig::default()
        };
        let mut cache = PlanCache::new(config);
        let plan = RelExpr::scan("users");
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp.clone(), plan);
        let _ = cache.lookup(&fp);

        cache.invalidate(std::slice::from_ref(&fp));
        let r1 = cache.lookup(&fp).expect("hit");
        assert_eq!(r1.match_type, CacheMatchType::Stale);

        cache.mark_fresh(&fp);
        let r2 = cache.lookup(&fp).expect("hit");
        assert_eq!(r2.match_type, CacheMatchType::Exact);
    }

    #[test]
    fn insert_with_deps_stores_dependencies() {
        use std::collections::{HashMap, HashSet};

        let mut cache = PlanCache::with_defaults();
        let plan = RelExpr::scan("orders");
        let fp = QueryFingerprint::from_rel_expr(&plan);
        let deps = PlanDependencies {
            table_cardinalities: [("orders".into(), 5000.0)].into_iter().collect(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };

        cache.insert_with_deps(fp.clone(), plan, deps);
        assert!(cache.get_dependencies(&fp).is_some());
        let d = cache.get_dependencies(&fp).expect("deps");
        assert!(d.table_cardinalities.contains_key("orders"));
    }

    #[test]
    fn invalidate_for_table_targets_correct_entries() {
        use std::collections::{HashMap, HashSet};

        let mut cache = PlanCache::with_defaults();

        let p1 = RelExpr::scan("users");
        let fp1 = QueryFingerprint::from_rel_expr(&p1);
        let d1 = PlanDependencies {
            table_cardinalities: [("users".into(), 1000.0)].into_iter().collect(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };

        let p2 = RelExpr::scan("orders");
        let fp2 = QueryFingerprint::from_rel_expr(&p2);
        let d2 = PlanDependencies {
            table_cardinalities: [("orders".into(), 5000.0)].into_iter().collect(),
            indexes: HashSet::new(),
            distinct_counts: HashMap::new(),
            histogram_digests: HashMap::new(),
            facts: HashSet::new(),
        };

        cache.insert_with_deps(fp1.clone(), p1, d1);
        cache.insert_with_deps(fp2.clone(), p2, d2);

        cache.invalidate_for_table("users");

        // users entry evicted, orders entry remains
        assert!(cache.lookup(&fp1).is_none());
        assert!(cache.lookup(&fp2).is_some());
    }
}
