//! Synchronization markers for coordinating steps across sessions.
//!
//! Markers allow one session's step to signal a named event and
//! another session's step to wait for that event before proceeding.
//! This enables deterministic ordering of concurrent operations
//! without relying on timing.

use std::collections::{HashMap, HashSet};

/// Tracks signaled and waited-on markers across all sessions.
#[derive(Debug, Clone, Default)]
pub struct Marker {
    signaled: HashSet<String>,
    waiters: HashMap<String, Vec<String>>,
}

/// Error from marker operations.
#[derive(Debug, thiserror::Error)]
pub enum MarkerError {
    /// A wait timed out because the marker was never signaled.
    #[error("marker '{name}' was never signaled (waited by session '{session}')")]
    WaitTimeout {
        /// The marker name.
        name: String,
        /// The session that was waiting.
        session: String,
    },
}

impl Marker {
    /// Create a new marker tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Signal a named marker from a session.
    pub fn signal(&mut self, name: &str) {
        self.signaled.insert(name.to_owned());
    }

    /// Check whether a marker has been signaled.
    #[must_use]
    pub fn is_signaled(&self, name: &str) -> bool {
        self.signaled.contains(name)
    }

    /// Register a session as waiting for a marker.
    pub fn register_waiter(
        &mut self,
        marker_name: &str,
        session_name: &str,
    ) {
        self.waiters
            .entry(marker_name.to_owned())
            .or_default()
            .push(session_name.to_owned());
    }

    /// Return sessions waiting on a marker.
    #[must_use]
    pub fn waiters_for(&self, marker_name: &str) -> &[String] {
        self.waiters
            .get(marker_name)
            .map_or(&[], Vec::as_slice)
    }

    /// Return all signaled marker names.
    #[must_use]
    pub fn signaled_markers(&self) -> &HashSet<String> {
        &self.signaled
    }

    /// Reset all marker state.
    pub fn reset(&mut self) {
        self.signaled.clear();
        self.waiters.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_and_check() {
        let mut m = Marker::new();
        assert!(!m.is_signaled("lock_acquired"));
        m.signal("lock_acquired");
        assert!(m.is_signaled("lock_acquired"));
    }

    #[test]
    fn register_waiters() {
        let mut m = Marker::new();
        m.register_waiter("ready", "s1");
        m.register_waiter("ready", "s2");
        assert_eq!(m.waiters_for("ready").len(), 2);
        assert!(m.waiters_for("nonexistent").is_empty());
    }

    #[test]
    fn reset_clears_state() {
        let mut m = Marker::new();
        m.signal("x");
        m.register_waiter("x", "s1");
        m.reset();
        assert!(!m.is_signaled("x"));
        assert!(m.waiters_for("x").is_empty());
    }
}
