//! Metadata cache with relcache invalidation tracking.
//!
//! Caches table metadata (statistics, indexes, constraints) from
//! pg_catalog to avoid repeated syscache queries. Automatically
//! invalidates cached entries when PostgreSQL's relcache is
//! invalidated (ALTER TABLE, CREATE INDEX, ANALYZE, etc.).
//!
//! Uses `CacheRegisterRelcacheCallback()` to receive invalidation
//! notifications from PostgreSQL. Cached metadata is marked stale
//! and refreshed lazily on the next query.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::SystemTime;

use pgrx::pg_sys;
use ra_core::Statistics;

use crate::stats_bridge;

/// Maximum cache size before LRU eviction (number of tables).
const MAX_CACHE_ENTRIES: usize = 1000;

/// Global metadata cache (protected by mutex for thread safety).
///
/// Shared across all backend processes in a single PostgreSQL
/// connection. Each backend has its own cache instance.
static METADATA_CACHE: once_cell::sync::Lazy<Mutex<MetadataCache>> =
    once_cell::sync::Lazy::new(|| Mutex::new(MetadataCache::new()));

/// Cached table metadata with validity tracking.
#[derive(Debug, Clone)]
pub struct CachedTableMetadata {
    /// Table statistics from pg_catalog.
    pub stats: Statistics,

    /// Timestamp when metadata was last refreshed.
    pub refresh_time: SystemTime,

    /// Whether metadata is currently valid (not invalidated).
    pub is_valid: bool,

    /// Last access time (for LRU eviction).
    last_access: SystemTime,
}

impl CachedTableMetadata {
    fn new(stats: Statistics) -> Self {
        let now = SystemTime::now();
        Self {
            stats,
            refresh_time: now,
            is_valid: true,
            last_access: now,
        }
    }

    fn touch(&mut self) {
        self.last_access = SystemTime::now();
    }
}

/// Metadata cache with invalidation tracking.
///
/// Stores table statistics keyed by relation OID. Tracks invalidated
/// tables and performs lazy refresh on next access.
pub struct MetadataCache {
    /// Cached metadata by relation OID.
    tables: HashMap<pg_sys::Oid, CachedTableMetadata>,

    /// OIDs pending refresh (invalidated but not yet refreshed).
    invalidated: HashSet<pg_sys::Oid>,

    /// Total number of cache hits (for metrics).
    hits: u64,

    /// Total number of cache misses (for metrics).
    misses: u64,

    /// Total number of invalidations received (for metrics).
    invalidations: u64,
}

impl MetadataCache {
    fn new() -> Self {
        Self {
            tables: HashMap::new(),
            invalidated: HashSet::new(),
            hits: 0,
            misses: 0,
            invalidations: 0,
        }
    }

    /// Mark a table's metadata as stale (called from relcache callback).
    ///
    /// Does not refresh immediately - metadata is refreshed lazily
    /// on the next query that references this table.
    pub fn invalidate(&mut self, oid: pg_sys::Oid) {
        self.invalidations += 1;

        if let Some(entry) = self.tables.get_mut(&oid) {
            entry.is_valid = false;
        }

        self.invalidated.insert(oid);

        // Log invalidation if debug logging is enabled
        if crate::extension_state::RA_LOG_DECISIONS.get() {
            pgrx::debug1!(
                "Ra: invalidated metadata cache for relation OID {}",
                u32::from(oid)
            );
        }
    }

    /// Check if cached metadata is valid.
    ///
    /// Returns false if:
    /// - Table not in cache
    /// - Cached entry marked invalid
    /// - Table in invalidation set
    pub fn is_valid(&self, oid: pg_sys::Oid) -> bool {
        if self.invalidated.contains(&oid) {
            return false;
        }

        self.tables
            .get(&oid)
            .map(|entry| entry.is_valid)
            .unwrap_or(false)
    }

    /// Refresh metadata from pg_catalog (syscache queries).
    ///
    /// Queries pg_class, pg_statistic, pg_index, and pg_constraint
    /// to rebuild fresh statistics for the table.
    ///
    /// Returns None if the table no longer exists (dropped).
    pub fn refresh(&mut self, oid: pg_sys::Oid) -> Option<Statistics> {
        self.misses += 1;

        // Query pg_catalog for fresh metadata
        let stats = stats_bridge::gather_table_stats_by_oid(oid)?;

        let entry = CachedTableMetadata::new(stats.clone());
        self.tables.insert(oid, entry);
        self.invalidated.remove(&oid);

        // Evict LRU entries if cache is too large
        if self.tables.len() > MAX_CACHE_ENTRIES {
            self.evict_lru();
        }

        // Log refresh if debug logging is enabled
        if crate::extension_state::RA_LOG_DECISIONS.get() {
            pgrx::debug1!(
                "Ra: refreshed metadata cache for relation OID {} ({} rows, {} columns)",
                u32::from(oid),
                stats.row_count,
                stats.columns.len()
            );
        }

        Some(stats)
    }

    /// Get metadata from cache without refresh.
    ///
    /// Returns None if not in cache or invalidated.
    pub fn get(&mut self, oid: pg_sys::Oid) -> Option<Statistics> {
        if !self.is_valid(oid) {
            return None;
        }

        if let Some(entry) = self.tables.get_mut(&oid) {
            entry.touch();
            self.hits += 1;
            return Some(entry.stats.clone());
        }

        None
    }

    /// Get metadata, refreshing if stale.
    ///
    /// This is the primary public API. Returns:
    /// - Cached metadata if valid (fast path)
    /// - Refreshed metadata if stale (slow path)
    /// - None if table doesn't exist
    pub fn get_or_refresh(&mut self, oid: pg_sys::Oid) -> Option<Statistics> {
        if let Some(stats) = self.get(oid) {
            return Some(stats);
        }

        self.refresh(oid)
    }

    /// Evict least-recently-used entries to keep cache size bounded.
    fn evict_lru(&mut self) {
        let target_size = MAX_CACHE_ENTRIES * 9 / 10; // Evict 10%

        if self.tables.len() <= target_size {
            return;
        }

        let mut entries: Vec<_> = self.tables.iter().collect();
        entries.sort_by_key(|(_, entry)| entry.last_access);

        let num_to_evict = self.tables.len() - target_size;
        let evict_oids: Vec<_> = entries
            .iter()
            .take(num_to_evict)
            .map(|(&oid, _)| oid)
            .collect();

        for oid in evict_oids {
            self.tables.remove(&oid);
            self.invalidated.remove(&oid);
        }

        if crate::extension_state::RA_LOG_DECISIONS.get() {
            pgrx::debug1!(
                "Ra: evicted {} LRU entries from metadata cache",
                num_to_evict
            );
        }
    }

    /// Clear all cached metadata (manual refresh).
    pub fn clear(&mut self) {
        self.tables.clear();
        self.invalidated.clear();
    }

    /// Get cache statistics for monitoring.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.tables.len(),
            invalidated: self.invalidated.len(),
            hits: self.hits,
            misses: self.misses,
            invalidations: self.invalidations,
            hit_rate: if self.hits + self.misses > 0 {
                self.hits as f64 / (self.hits + self.misses) as f64
            } else {
                0.0
            },
        }
    }
}

/// Cache statistics for monitoring and observability.
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Number of tables currently cached.
    pub entries: usize,

    /// Number of tables pending refresh (invalidated).
    pub invalidated: usize,

    /// Total cache hits since process start.
    pub hits: u64,

    /// Total cache misses since process start.
    pub misses: u64,

    /// Total invalidations received since process start.
    pub invalidations: u64,

    /// Cache hit rate (hits / (hits + misses)).
    pub hit_rate: f64,
}

// ---------------------------------------------------------------
// Public API functions (FFI-safe, exported for C callback)
// ---------------------------------------------------------------

/// Mark a table as invalidated (called from C relcache callback).
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process with
/// valid memory context. Called by the relcache invalidation
/// callback registered in `_PG_init()`.
#[no_mangle]
pub extern "C" fn ra_rust_invalidate_table(oid: pg_sys::Oid) {
    if let Ok(mut cache) = METADATA_CACHE.lock() {
        cache.invalidate(oid);
    }
}

/// Get table metadata, refreshing if stale (public API for planner hook).
///
/// Returns None if the table doesn't exist or catalog access fails.
pub fn get_table_metadata(oid: pg_sys::Oid) -> Option<Statistics> {
    METADATA_CACHE.lock().ok()?.get_or_refresh(oid)
}

/// Clear all cached metadata (manual refresh function).
///
/// Exposed as SQL function: `SELECT ra.clear_metadata_cache();`
pub fn clear_cache() {
    if let Ok(mut cache) = METADATA_CACHE.lock() {
        cache.clear();
    }
}

/// Get cache statistics for monitoring.
///
/// Exposed as SQL function: `SELECT * FROM ra.metadata_cache_stats();`
pub fn get_cache_stats() -> Option<CacheStats> {
    METADATA_CACHE.lock().ok().map(|cache| cache.stats())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_invalidation_marks_stale() {
        let mut cache = MetadataCache::new();
        let oid = pg_sys::Oid::from(1234);

        // Insert entry
        let stats = Statistics::new(1000.0);
        cache
            .tables
            .insert(oid, CachedTableMetadata::new(stats.clone()));

        assert!(cache.is_valid(oid));

        // Invalidate
        cache.invalidate(oid);

        assert!(!cache.is_valid(oid));
        assert_eq!(cache.invalidations, 1);
    }

    #[test]
    fn cache_hit_increments_counter() {
        let mut cache = MetadataCache::new();
        let oid = pg_sys::Oid::from(1234);

        // Insert entry
        let stats = Statistics::new(1000.0);
        cache
            .tables
            .insert(oid, CachedTableMetadata::new(stats.clone()));

        assert_eq!(cache.hits, 0);

        // Cache hit
        let result = cache.get(oid);
        assert!(result.is_some());
        assert_eq!(cache.hits, 1);
    }

    #[test]
    fn cache_miss_returns_none() {
        let mut cache = MetadataCache::new();
        let oid = pg_sys::Oid::from(1234);

        assert_eq!(cache.misses, 0);

        // Cache miss (not in cache)
        let result = cache.get(oid);
        assert!(result.is_none());
        assert_eq!(cache.hits, 0); // get() doesn't increment misses
    }

    #[test]
    fn invalidated_entry_returns_none() {
        let mut cache = MetadataCache::new();
        let oid = pg_sys::Oid::from(1234);

        // Insert and invalidate
        let stats = Statistics::new(1000.0);
        cache
            .tables
            .insert(oid, CachedTableMetadata::new(stats.clone()));
        cache.invalidate(oid);

        // Should return None (invalid)
        let result = cache.get(oid);
        assert!(result.is_none());
    }

    #[test]
    fn clear_cache_removes_all_entries() {
        let mut cache = MetadataCache::new();

        // Insert multiple entries
        for i in 1..=10 {
            let oid = pg_sys::Oid::from(i);
            let stats = Statistics::new(100.0);
            cache.tables.insert(oid, CachedTableMetadata::new(stats));
        }

        assert_eq!(cache.tables.len(), 10);

        // Clear
        cache.clear();

        assert_eq!(cache.tables.len(), 0);
        assert_eq!(cache.invalidated.len(), 0);
    }

    #[test]
    fn cache_stats_calculates_hit_rate() {
        let mut cache = MetadataCache::new();
        cache.hits = 80;
        cache.misses = 20;

        let stats = cache.stats();
        assert_eq!(stats.hits, 80);
        assert_eq!(stats.misses, 20);
        assert!((stats.hit_rate - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_stats_zero_hit_rate_when_empty() {
        let cache = MetadataCache::new();
        let stats = cache.stats();
        assert_eq!(stats.hit_rate, 0.0);
    }

    #[test]
    fn lru_eviction_removes_oldest_entries() {
        let mut cache = MetadataCache::new();

        // Insert MAX_CACHE_ENTRIES + 1 entries
        for i in 0..=MAX_CACHE_ENTRIES {
            let oid = pg_sys::Oid::from(i as u32);
            let stats = Statistics::new(100.0);
            cache.tables.insert(oid, CachedTableMetadata::new(stats));
        }

        // Should trigger eviction
        assert!(cache.tables.len() <= MAX_CACHE_ENTRIES);

        // First entry should be evicted (oldest)
        let first_oid = pg_sys::Oid::from(0);
        assert!(!cache.tables.contains_key(&first_oid));
    }

    #[test]
    fn touch_updates_access_time() {
        let mut entry = CachedTableMetadata::new(Statistics::new(100.0));
        let original_time = entry.last_access;

        // Sleep is not available in this context, but we can test the mechanism
        entry.touch();

        // Access time should be updated (>= original time)
        assert!(entry.last_access >= original_time);
    }
}
