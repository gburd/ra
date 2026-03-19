//! Eviction policy implementations.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::key::QueryKey;
use crate::plan::CachedPlan;

/// Cache eviction strategy.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum EvictionPolicy {
    /// Least Recently Used: evicts the entry accessed longest ago.
    Lru,
    /// Least Frequently Used: evicts the entry with the fewest accesses.
    Lfu,
    /// Adaptive: combines recency and frequency, favoring eviction
    /// of entries that are both infrequently used and stale.
    Adaptive,
}

impl std::fmt::Display for EvictionPolicy {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Lru => write!(f, "LRU"),
            Self::Lfu => write!(f, "LFU"),
            Self::Adaptive => write!(f, "Adaptive"),
        }
    }
}

/// Find the LRU victim (oldest `last_accessed`).
pub(crate) fn find_lru_victim(
    entries: &HashMap<QueryKey, CachedPlan>,
) -> Option<QueryKey> {
    entries
        .iter()
        .min_by_key(|(_, plan)| plan.last_accessed)
        .map(|(key, _)| key.clone())
}

/// Find the LFU victim (lowest `use_count`, tie-break by LRU).
pub(crate) fn find_lfu_victim(
    entries: &HashMap<QueryKey, CachedPlan>,
) -> Option<QueryKey> {
    entries
        .iter()
        .min_by(|(_, a), (_, b)| {
            a.use_count
                .cmp(&b.use_count)
                .then_with(|| a.last_accessed.cmp(&b.last_accessed))
        })
        .map(|(key, _)| key.clone())
}

/// Adaptive victim selection: score = `use_count` / staleness.
///
/// Entries with a low score (infrequent access, long since
/// optimization) are evicted first.
pub(crate) fn find_adaptive_victim(
    entries: &HashMap<QueryKey, CachedPlan>,
) -> Option<QueryKey> {
    entries
        .iter()
        .min_by(|(_, a), (_, b)| {
            let score_a = adaptive_score(a);
            let score_b = adaptive_score(b);
            score_a
                .partial_cmp(&score_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(key, _)| key.clone())
}

/// Compute an adaptive retention score.
///
/// Higher score = more worth keeping. Combines frequency (`use_count`)
/// with recency (seconds since last access, inverted).
fn adaptive_score(plan: &CachedPlan) -> f64 {
    let recency_secs = plan.last_accessed.elapsed().as_secs_f64();
    let recency_factor = if recency_secs < 1.0 {
        1.0
    } else {
        1.0 / recency_secs
    };

    #[allow(clippy::cast_precision_loss)]
    let frequency = (plan.use_count as f64).max(1.0);

    frequency * recency_factor
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::cost::Cost;
    use ra_core::statistics::Statistics;
    use std::collections::HashMap;
    use std::time::Instant;

    fn make_entry(
        use_count: u64,
        age_ms: u64,
    ) -> CachedPlan {
        let now = Instant::now();
        let accessed =
            now - std::time::Duration::from_millis(age_ms);
        let mut snapshot = HashMap::new();
        snapshot.insert("t".to_owned(), Statistics::new(100.0));
        CachedPlan {
            plan: RelExpr::scan("t"),
            cost: Cost::ZERO,
            statistics_snapshot: snapshot,
            original_sql: "SELECT 1".to_owned(),
            created_at: now,
            optimized_at: now,
            last_accessed: accessed,
            use_count,
            reoptimization_count: 0,
        }
    }

    fn make_key(id: &str) -> QueryKey {
        QueryKey::new(id.to_owned(), "auto".to_owned(), vec![])
    }

    #[test]
    fn lru_picks_oldest() {
        let mut entries = HashMap::new();
        entries.insert(
            make_key("new"),
            make_entry(1, 10),
        );
        entries.insert(
            make_key("old"),
            make_entry(1, 1000),
        );

        let victim = find_lru_victim(&entries)
            .expect("should find victim");
        assert_eq!(victim.sql, "old");
    }

    #[test]
    fn lfu_picks_least_used() {
        let mut entries = HashMap::new();
        entries.insert(
            make_key("hot"),
            make_entry(100, 10),
        );
        entries.insert(
            make_key("cold"),
            make_entry(1, 10),
        );

        let victim = find_lfu_victim(&entries)
            .expect("should find victim");
        assert_eq!(victim.sql, "cold");
    }

    #[test]
    fn eviction_policy_display() {
        assert_eq!(EvictionPolicy::Lru.to_string(), "LRU");
        assert_eq!(EvictionPolicy::Lfu.to_string(), "LFU");
        assert_eq!(
            EvictionPolicy::Adaptive.to_string(),
            "Adaptive"
        );
    }
}
