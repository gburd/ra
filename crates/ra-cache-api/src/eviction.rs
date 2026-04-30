//! Eviction policy types.

use serde::{Deserialize, Serialize};

/// Cache eviction strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lru => write!(f, "LRU"),
            Self::Lfu => write!(f, "LFU"),
            Self::Adaptive => write!(f, "Adaptive"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviction_policy_display() {
        assert_eq!(EvictionPolicy::Lru.to_string(), "LRU");
        assert_eq!(EvictionPolicy::Lfu.to_string(), "LFU");
        assert_eq!(EvictionPolicy::Adaptive.to_string(), "Adaptive");
    }
}
