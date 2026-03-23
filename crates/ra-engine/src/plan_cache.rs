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

use std::collections::HashMap;

use ra_core::algebra::RelExpr;

use crate::genetic_fingerprint::QueryFingerprint;

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
}

impl Default for PlanCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1024,
            similarity_threshold: 0.9,
            enable_fuzzy_matching: true,
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
    /// Exact fingerprint match (join graph, predicate, aggregation
    /// all identical).
    Exact,
    /// Fuzzy match above the similarity threshold.
    Fuzzy,
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
}

impl PlanCacheStats {
    /// Overall cache hit rate (exact + fuzzy).
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let hits = (self.exact_hits + self.fuzzy_hits) as f64;
        #[allow(clippy::cast_precision_loss)]
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
    /// Tries exact match first, then fuzzy match if enabled.
    /// Returns `None` on cache miss.
    pub fn lookup(
        &mut self,
        fingerprint: &QueryFingerprint,
    ) -> Option<CacheLookupResult> {
        self.stats.lookups += 1;

        // Try exact match first
        if let Some(&idx) = self.exact_index.get(fingerprint) {
            self.access_counter += 1;
            self.entries[idx].last_access = self.access_counter;
            self.entries[idx].hit_count += 1;
            self.stats.exact_hits += 1;
            return Some(CacheLookupResult {
                plan: self.entries[idx].plan.clone(),
                match_type: CacheMatchType::Exact,
                similarity: 1.0,
            });
        }

        // Try fuzzy match if enabled
        if self.config.enable_fuzzy_matching {
            if let Some(result) =
                self.fuzzy_lookup(fingerprint)
            {
                self.stats.fuzzy_hits += 1;
                return Some(result);
            }
        }

        self.stats.misses += 1;
        None
    }

    /// Insert a plan into the cache.
    ///
    /// If the fingerprint already exists, the plan is updated.
    /// If the cache is full, the least-recently-used entry is evicted.
    pub fn insert(
        &mut self,
        fingerprint: QueryFingerprint,
        plan: RelExpr,
    ) {
        // Update existing entry
        if let Some(&idx) = self.exact_index.get(&fingerprint) {
            self.access_counter += 1;
            self.entries[idx].plan = plan;
            self.entries[idx].last_access = self.access_counter;
            return;
        }

        // Evict if at capacity
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
        });
        self.stats.current_entries = self.entries.len();
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

    fn fuzzy_lookup(
        &mut self,
        fingerprint: &QueryFingerprint,
    ) -> Option<CacheLookupResult> {
        let mut best_idx: Option<usize> = None;
        let mut best_similarity: f64 = 0.0;

        for (idx, entry) in self.entries.iter().enumerate() {
            let sim =
                fingerprint.similarity(&entry.fingerprint);
            if sim >= self.config.similarity_threshold
                && sim > best_similarity
            {
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

        // Find the entry with the smallest last_access
        let lru_idx = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.last_access)
            .map(|(i, _)| i)
            .expect("entries is non-empty");

        // Remove from exact index
        self.exact_index
            .remove(&self.entries[lru_idx].fingerprint);

        // Swap-remove from entries vec and fix up the index
        // for the entry that was moved into the vacated slot
        self.entries.swap_remove(lru_idx);

        if lru_idx < self.entries.len() {
            // The entry that was at the end is now at lru_idx
            let moved_fp =
                self.entries[lru_idx].fingerprint.clone();
            self.exact_index.insert(moved_fp, lru_idx);
        }

        self.stats.evictions += 1;
        self.stats.current_entries = self.entries.len();
    }
}

impl std::fmt::Debug for PlanCache {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("PlanCache")
            .field("entries", &self.entries.len())
            .field("max_entries", &self.config.max_entries)
            .field("stats", &self.stats)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::JoinType;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn make_scan_filter(
        table: &str,
        col: &str,
        value: i64,
    ) -> RelExpr {
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
                left: Box::new(Expr::Column(
                    ColumnRef::qualified("u", "id"),
                )),
                right: Box::new(Expr::Column(
                    ColumnRef::qualified("o", "uid"),
                )),
            },
            left: Box::new(make_scan_filter("users", "age", value)),
            right: Box::new(RelExpr::scan("orders")),
        }
    }

    // ── Basic cache operations ───────────────────────────────────

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

        // Insert plan with value=18
        let plan1 = make_scan_filter("users", "age", 18);
        let fp1 = QueryFingerprint::from_rel_expr(&plan1);
        cache.insert(fp1, plan1.clone());

        // Look up with value=65 -- same fingerprint
        let plan2 = make_scan_filter("users", "age", 65);
        let fp2 = QueryFingerprint::from_rel_expr(&plan2);
        let result = cache.lookup(&fp2);

        assert!(result.is_some());
        let hit = result.expect("parameter variation should hit");
        assert_eq!(hit.match_type, CacheMatchType::Exact);
    }

    // ── LRU eviction ─────────────────────────────────────────────

    #[test]
    fn lru_eviction_at_capacity() {
        let config = PlanCacheConfig {
            max_entries: 3,
            ..PlanCacheConfig::default()
        };
        let mut cache = PlanCache::new(config);

        // Insert 3 entries
        for table in &["t1", "t2", "t3"] {
            let plan = RelExpr::scan(*table);
            let fp = QueryFingerprint::from_rel_expr(&plan);
            cache.insert(fp, plan);
        }
        assert_eq!(cache.len(), 3);

        // Access t2 and t3 to make t1 the LRU
        let fp2 = QueryFingerprint::from_rel_expr(
            &RelExpr::scan("t2"),
        );
        let fp3 = QueryFingerprint::from_rel_expr(
            &RelExpr::scan("t3"),
        );
        let _ = cache.lookup(&fp2);
        let _ = cache.lookup(&fp3);

        // Insert a 4th entry -> should evict t1
        let plan4 = RelExpr::scan("t4");
        let fp4 = QueryFingerprint::from_rel_expr(&plan4);
        cache.insert(fp4.clone(), plan4);

        assert_eq!(cache.len(), 3);
        assert!(cache.stats().evictions >= 1);

        // t1 should be gone
        let fp1 = QueryFingerprint::from_rel_expr(
            &RelExpr::scan("t1"),
        );
        assert!(cache.lookup(&fp1).is_none());

        // t4 should be present
        assert!(cache.lookup(&fp4).is_some());
    }

    // ── Statistics tracking ──────────────────────────────────────

    #[test]
    fn stats_tracking() {
        let mut cache = PlanCache::with_defaults();
        let plan = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan);

        // Miss
        let _ = cache.lookup(&fp);
        assert_eq!(cache.stats().lookups, 1);
        assert_eq!(cache.stats().misses, 1);

        // Insert and hit
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

        // 1 miss + 3 hits = 75% hit rate
        let different = RelExpr::scan("nonexistent");
        let dfp = QueryFingerprint::from_rel_expr(&different);
        let _ = cache.lookup(&dfp);
        let _ = cache.lookup(&fp);
        let _ = cache.lookup(&fp);
        let _ = cache.lookup(&fp);

        let rate = cache.stats().hit_rate();
        assert!((rate - 0.75).abs() < f64::EPSILON);
    }

    // ── Update existing entry ────────────────────────────────────

    #[test]
    fn insert_same_fingerprint_updates_plan() {
        let mut cache = PlanCache::with_defaults();

        let plan1 = make_scan_filter("users", "age", 18);
        let fp = QueryFingerprint::from_rel_expr(&plan1);
        cache.insert(fp.clone(), plan1);

        // Insert a different plan with the same fingerprint
        let plan2 = make_scan_filter("users", "age", 99);
        cache.insert(fp.clone(), plan2.clone());

        assert_eq!(cache.len(), 1);
        let hit = cache.lookup(&fp).expect("should hit");
        assert_eq!(hit.plan, plan2);
    }

    // ── Clear ────────────────────────────────────────────────────

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

    // ── Cache hit rate on parameter workload ─────────────────────

    #[test]
    fn workload_with_parameter_variations_high_hit_rate() {
        let mut cache = PlanCache::with_defaults();

        // Simulate a workload: same query structure, varying params
        let base = make_join_query(1000);
        let fp = QueryFingerprint::from_rel_expr(&base);
        cache.insert(fp, base);

        // 20 queries with different parameter values
        let mut hits = 0_u32;
        for i in 0..20 {
            let q = make_join_query(i * 100 + 50);
            let qfp = QueryFingerprint::from_rel_expr(&q);
            if cache.lookup(&qfp).is_some() {
                hits += 1;
            }
        }

        // All 20 should hit since only the constant differs
        assert_eq!(hits, 20);
        assert!(cache.stats().hit_rate() > 0.9);
    }
}
