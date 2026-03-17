//! Lock monitoring and deadlock detection.
//!
//! Provides utilities to query lock state from database adapters and
//! detect deadlocks by analyzing wait-for graphs across sessions.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::adapter::{AdapterError, LockState};
use crate::session::Session;

/// Type of lock held or requested.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash,
)]
pub enum LockType {
    /// Shared/read lock.
    Shared,
    /// Exclusive/write lock.
    Exclusive,
    /// Intent shared lock.
    IntentShared,
    /// Intent exclusive lock.
    IntentExclusive,
}

impl LockType {
    /// Parse a lock type from a database-specific mode string.
    #[must_use]
    pub fn from_mode(mode: &str) -> Self {
        let lower = mode.to_lowercase();
        if lower.contains("exclusive") && lower.contains("intent") {
            Self::IntentExclusive
        } else if lower.contains("exclusive") {
            Self::Exclusive
        } else if lower.contains("shared") && lower.contains("intent")
        {
            Self::IntentShared
        } else {
            Self::Shared
        }
    }

    /// Check whether two lock types conflict.
    #[must_use]
    pub fn conflicts_with(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Exclusive, _)
                | (_, Self::Exclusive)
                | (Self::IntentExclusive,
                    Self::Shared | Self::IntentExclusive)
                | (Self::Shared, Self::IntentExclusive)
        )
    }
}

/// Information about a lock for reporting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockInfo {
    /// The session holding or waiting for this lock.
    pub session: String,
    /// The resource being locked.
    pub resource: String,
    /// The lock type.
    pub lock_type: LockType,
    /// Whether the lock has been granted.
    pub granted: bool,
}

/// Monitors locks across all sessions and detects deadlocks.
#[derive(Debug, Default)]
pub struct LockMonitor {
    last_states: HashMap<String, LockState>,
}

impl LockMonitor {
    /// Create a new lock monitor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh lock state for all sessions.
    ///
    /// # Errors
    ///
    /// Returns `AdapterError` if any session's lock query fails.
    pub fn refresh(
        &mut self,
        sessions: &[Session],
    ) -> Result<(), AdapterError> {
        self.last_states.clear();
        for session in sessions {
            let state = session.adapter().lock_state()?;
            self.last_states
                .insert(session.name().to_owned(), state);
        }
        Ok(())
    }

    /// Return all lock info across sessions.
    #[must_use]
    pub fn all_locks(&self) -> Vec<LockInfo> {
        let mut locks = Vec::new();
        for (session_name, state) in &self.last_states {
            for detail in &state.held {
                locks.push(LockInfo {
                    session: session_name.clone(),
                    resource: detail.resource.clone(),
                    lock_type: LockType::from_mode(&detail.mode),
                    granted: true,
                });
            }
            for detail in &state.waiting {
                locks.push(LockInfo {
                    session: session_name.clone(),
                    resource: detail.resource.clone(),
                    lock_type: LockType::from_mode(&detail.mode),
                    granted: false,
                });
            }
        }
        locks
    }

    /// Detect blocked sessions (those waiting for locks).
    #[must_use]
    pub fn blocked_sessions(&self) -> Vec<String> {
        self.last_states
            .iter()
            .filter(|(_, state)| !state.waiting.is_empty())
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Detect deadlocks by finding cycles in the wait-for graph.
    ///
    /// Returns groups of session names involved in each deadlock cycle.
    #[must_use]
    pub fn detect_deadlocks(&self) -> Vec<Vec<String>> {
        let wait_for = self.build_wait_for_graph();
        find_cycles(&wait_for)
    }

    fn build_wait_for_graph(&self) -> HashMap<String, HashSet<String>> {
        let mut graph: HashMap<String, HashSet<String>> =
            HashMap::new();

        let all_locks = self.all_locks();
        let held_locks: Vec<&LockInfo> =
            all_locks.iter().filter(|l| l.granted).collect();
        let waiting_locks: Vec<&LockInfo> =
            all_locks.iter().filter(|l| !l.granted).collect();

        for waiting in &waiting_locks {
            for held in &held_locks {
                if waiting.resource == held.resource
                    && waiting.session != held.session
                    && waiting.lock_type.conflicts_with(held.lock_type)
                {
                    graph
                        .entry(waiting.session.clone())
                        .or_default()
                        .insert(held.session.clone());
                }
            }
        }

        graph
    }
}

/// Find cycles in a directed graph using DFS.
fn find_cycles(
    graph: &HashMap<String, HashSet<String>>,
) -> Vec<Vec<String>> {
    let mut cycles = Vec::new();
    let mut visited = HashSet::new();
    let mut on_stack = HashSet::new();
    let mut stack = Vec::new();

    for node in graph.keys() {
        if !visited.contains(node) {
            dfs_cycle(
                node,
                graph,
                &mut visited,
                &mut on_stack,
                &mut stack,
                &mut cycles,
            );
        }
    }

    cycles
}

fn dfs_cycle(
    node: &str,
    graph: &HashMap<String, HashSet<String>>,
    visited: &mut HashSet<String>,
    on_stack: &mut HashSet<String>,
    stack: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    visited.insert(node.to_owned());
    on_stack.insert(node.to_owned());
    stack.push(node.to_owned());

    if let Some(neighbors) = graph.get(node) {
        for neighbor in neighbors {
            if !visited.contains(neighbor.as_str()) {
                dfs_cycle(
                    neighbor, graph, visited, on_stack, stack, cycles,
                );
            } else if on_stack.contains(neighbor.as_str()) {
                let cycle_start = stack
                    .iter()
                    .position(|n| n == neighbor)
                    .unwrap_or(0);
                cycles.push(stack[cycle_start..].to_vec());
            }
        }
    }

    stack.pop();
    on_stack.remove(node);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_type_conflicts() {
        assert!(
            LockType::Exclusive.conflicts_with(LockType::Exclusive)
        );
        assert!(LockType::Exclusive.conflicts_with(LockType::Shared));
        assert!(LockType::Shared.conflicts_with(LockType::Exclusive));
        assert!(!LockType::Shared.conflicts_with(LockType::Shared));
    }

    #[test]
    fn detect_simple_deadlock() {
        let mut graph: HashMap<String, HashSet<String>> =
            HashMap::new();
        graph
            .entry("s1".into())
            .or_default()
            .insert("s2".into());
        graph
            .entry("s2".into())
            .or_default()
            .insert("s1".into());

        let cycles = find_cycles(&graph);
        assert!(
            !cycles.is_empty(),
            "should detect deadlock cycle"
        );
    }

    #[test]
    fn no_deadlock_without_cycle() {
        let mut graph: HashMap<String, HashSet<String>> =
            HashMap::new();
        graph
            .entry("s1".into())
            .or_default()
            .insert("s2".into());
        graph
            .entry("s2".into())
            .or_default()
            .insert("s3".into());

        let cycles = find_cycles(&graph);
        assert!(cycles.is_empty(), "no cycle should be detected");
    }

    #[test]
    fn lock_type_from_mode() {
        assert_eq!(
            LockType::from_mode("RowExclusiveLock"),
            LockType::Exclusive
        );
        assert_eq!(
            LockType::from_mode("AccessShareLock"),
            LockType::Shared
        );
        assert_eq!(
            LockType::from_mode("IntentExclusive"),
            LockType::IntentExclusive
        );
    }
}
