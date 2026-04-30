//! Cache performance metrics.

use serde::{Deserialize, Serialize};

/// Performance counters for the plan cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheMetrics {
    /// Number of cache hits.
    pub hits: u64,
    /// Number of cache misses.
    pub misses: u64,
    /// Number of evictions performed.
    pub evictions: u64,
    /// Number of times the cache was cleared.
    pub clears: u64,
    /// Current number of entries (updated on read).
    pub current_entries: usize,
    /// Maximum entries allowed.
    pub max_entries: usize,
}

impl CacheMetrics {
    /// Create a new metrics instance.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a cache hit.
    pub fn record_hit(&mut self) {
        self.hits = self.hits.saturating_add(1);
    }

    /// Record a cache miss.
    pub fn record_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
    }

    /// Record an eviction.
    pub fn record_eviction(&mut self) {
        self.evictions = self.evictions.saturating_add(1);
    }

    /// Record a cache clear.
    pub fn record_clear(&mut self) {
        self.clears = self.clears.saturating_add(1);
    }

    /// Total lookups (hits + misses).
    #[must_use]
    pub fn total_lookups(&self) -> u64 {
        self.hits.saturating_add(self.misses)
    }

    /// Hit rate as a fraction in `[0.0, 1.0]`.
    ///
    /// Returns 0.0 if no lookups have been performed.
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let total = self.total_lookups();
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    /// Cache utilization as a fraction in `[0.0, 1.0]`.
    ///
    /// Returns 0.0 if `max_entries` is 0.
    #[must_use]
    pub fn utilization(&self) -> f64 {
        if self.max_entries == 0 {
            return 0.0;
        }
        self.current_entries as f64 / self.max_entries as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_rate_empty() {
        let m = CacheMetrics::new();
        assert!((m.hit_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn hit_rate_computed() {
        let mut m = CacheMetrics::new();
        m.record_hit();
        m.record_hit();
        m.record_miss();
        // 2 / 3 ~ 0.666...
        let rate = m.hit_rate();
        assert!((rate - 2.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn utilization_computed() {
        let m = CacheMetrics {
            current_entries: 50,
            max_entries: 100,
            ..Default::default()
        };
        assert!((m.utilization() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn utilization_zero_max() {
        let m = CacheMetrics {
            current_entries: 0,
            max_entries: 0,
            ..Default::default()
        };
        assert!((m.utilization() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn total_lookups() {
        let mut m = CacheMetrics::new();
        m.record_hit();
        m.record_miss();
        m.record_miss();
        assert_eq!(m.total_lookups(), 3);
    }

    #[test]
    fn eviction_counter() {
        let mut m = CacheMetrics::new();
        m.record_eviction();
        m.record_eviction();
        assert_eq!(m.evictions, 2);
    }

    #[test]
    fn clear_counter() {
        let mut m = CacheMetrics::new();
        m.record_clear();
        assert_eq!(m.clears, 1);
    }
}
