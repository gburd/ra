//! Statistics caching to avoid repeated clones during optimization.
//!
//! During iterative optimization, table statistics are accessed frequently
//! for cost extraction. Without caching, the same Statistics objects are
//! cloned repeatedly (once per iteration for cost pruning, beam search, etc.).
//!
//! This module provides `StatsCache` which wraps statistics in `Arc` for
//! cheap reference counting instead of expensive clones.

use std::collections::HashMap;
use std::sync::Arc;

use ra_core::statistics::Statistics;

/// Thread-safe cache for table statistics.
///
/// Wraps statistics in `Arc` to enable cheap sharing without clones.
/// During optimization, statistics are read-only, so Arc is ideal.
#[derive(Debug, Clone)]
pub struct StatsCache {
    /// Shared statistics keyed by table name.
    inner: Arc<HashMap<String, Statistics>>,
}

impl StatsCache {
    /// Create a new empty statistics cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(HashMap::new()),
        }
    }

    /// Create a cache from an existing `HashMap` of statistics.
    ///
    /// This performs one clone to move data into the Arc, but subsequent
    /// accesses via `clone()` are cheap (just reference count increments).
    #[must_use]
    pub fn from_map(map: HashMap<String, Statistics>) -> Self {
        Self {
            inner: Arc::new(map),
        }
    }

    /// Create a cache by sharing an existing `Arc<HashMap>`.
    ///
    /// This is a zero-copy operation (just an Arc reference count
    /// increment). Use when the source already stores stats in an Arc.
    #[must_use]
    pub fn from_arc(inner: Arc<HashMap<String, Statistics>>) -> Self {
        Self { inner }
    }

    /// Get statistics for a table.
    ///
    /// Returns None if the table is not registered.
    #[inline]
    #[must_use]
    pub fn get(&self, table: &str) -> Option<&Statistics> {
        self.inner.get(table)
    }

    /// Check if a table has statistics registered.
    #[inline]
    #[must_use]
    pub fn contains_key(&self, table: &str) -> bool {
        self.inner.contains_key(table)
    }

    /// Check if the cache is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get the number of tables with statistics.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Get a reference to the inner `HashMap`.
    ///
    /// This allows passing the stats to functions that expect `HashMap`.
    /// Since the `HashMap` is Arc-wrapped, this is a cheap operation.
    #[inline]
    #[must_use]
    pub fn as_map(&self) -> &HashMap<String, Statistics> {
        &self.inner
    }

    /// Convert cache to a cloned `HashMap`.
    ///
    /// This performs an actual clone of the Statistics objects.
    /// Use this only when mutation is needed; prefer `as_map()` for reads.
    #[must_use]
    pub fn to_map(&self) -> HashMap<String, Statistics> {
        self.inner.as_ref().clone()
    }

    /// Iterate over all table names.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.inner.keys()
    }

    /// Iterate over all (table, statistics) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Statistics)> {
        self.inner.iter()
    }
}

impl Default for StatsCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for incrementally constructing a `StatsCache`.
///
/// Allows adding statistics one table at a time, then finalizing
/// into an Arc-wrapped cache.
#[derive(Debug, Default)]
pub struct StatsCacheBuilder {
    map: HashMap<String, Statistics>,
}

impl StatsCacheBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Add statistics for a table.
    ///
    /// If the table already exists, the previous statistics are replaced.
    pub fn insert(&mut self, table: String, stats: Statistics) {
        self.map.insert(table, stats);
    }

    /// Add statistics for a table (builder pattern).
    #[must_use]
    pub fn with_table(mut self, table: String, stats: Statistics) -> Self {
        self.map.insert(table, stats);
        self
    }

    /// Build the final `StatsCache`.
    ///
    /// This moves the `HashMap` into an Arc, making future clones cheap.
    #[must_use]
    pub fn build(self) -> StatsCache {
        StatsCache::from_map(self.map)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
#[expect(clippy::float_cmp, reason = "exact float literals in tests")]
mod tests {
    use super::*;

    #[test]
    fn test_cache_empty() {
        let cache = StatsCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_cache_from_map() {
        let mut map = HashMap::new();
        map.insert("users".to_string(), Statistics::new(1000.0));
        map.insert("orders".to_string(), Statistics::new(5000.0));

        let cache = StatsCache::from_map(map);

        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 2);
        assert!(cache.contains_key("users"));
        assert!(cache.contains_key("orders"));
        assert!(!cache.contains_key("products"));

        let users_stats = cache.get("users").unwrap();
        assert_eq!(users_stats.row_count, 1000.0);

        let orders_stats = cache.get("orders").unwrap();
        assert_eq!(orders_stats.row_count, 5000.0);
    }

    #[test]
    fn test_cache_clone_is_cheap() {
        let mut map = HashMap::new();
        map.insert("users".to_string(), Statistics::new(1000.0));

        let cache1 = StatsCache::from_map(map);
        let cache2 = cache1.clone();

        // Both caches should point to same underlying data
        assert_eq!(cache1.len(), cache2.len());
        assert_eq!(
            cache1.get("users").unwrap().row_count,
            cache2.get("users").unwrap().row_count
        );

        // Arc count should be 2 (both cache1 and cache2 hold references)
        assert_eq!(Arc::strong_count(&cache1.inner), 2);
    }

    #[test]
    fn test_cache_as_map() {
        let mut map = HashMap::new();
        map.insert("users".to_string(), Statistics::new(1000.0));

        let cache = StatsCache::from_map(map);
        let map_ref = cache.as_map();

        assert_eq!(map_ref.len(), 1);
        assert!(map_ref.contains_key("users"));
    }

    #[test]
    fn test_cache_to_map() {
        let mut map = HashMap::new();
        map.insert("users".to_string(), Statistics::new(1000.0));

        let cache = StatsCache::from_map(map.clone());
        let cloned_map = cache.to_map();

        assert_eq!(cloned_map.len(), map.len());
        assert_eq!(
            cloned_map.get("users").unwrap().row_count,
            map.get("users").unwrap().row_count
        );
    }

    #[test]
    fn test_cache_builder() {
        let cache = StatsCacheBuilder::new()
            .with_table("users".to_string(), Statistics::new(1000.0))
            .with_table("orders".to_string(), Statistics::new(5000.0))
            .build();

        assert_eq!(cache.len(), 2);
        assert!(cache.contains_key("users"));
        assert!(cache.contains_key("orders"));
    }

    #[test]
    fn test_cache_builder_insert() {
        let mut builder = StatsCacheBuilder::new();
        builder.insert("users".to_string(), Statistics::new(1000.0));
        builder.insert("orders".to_string(), Statistics::new(5000.0));

        let cache = builder.build();

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_cache_builder_replace() {
        let cache = StatsCacheBuilder::new()
            .with_table("users".to_string(), Statistics::new(1000.0))
            .with_table("users".to_string(), Statistics::new(2000.0)) // Replace
            .build();

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("users").unwrap().row_count, 2000.0);
    }

    #[test]
    fn test_cache_iteration() {
        let cache = StatsCacheBuilder::new()
            .with_table("a".to_string(), Statistics::new(100.0))
            .with_table("b".to_string(), Statistics::new(200.0))
            .with_table("c".to_string(), Statistics::new(300.0))
            .build();

        let keys: Vec<_> = cache.keys().cloned().collect();
        assert_eq!(keys.len(), 3);

        let mut total_rows = 0.0;
        for (_, stats) in cache.iter() {
            total_rows += stats.row_count;
        }
        assert_eq!(total_rows, 600.0);
    }
}
