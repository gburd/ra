//! Cost history tracking for regression detection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single query entry in the cost history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryEntry {
    /// Unique identifier for the query.
    pub query_id: String,
    /// The SQL query text.
    pub sql: String,
    /// Hash of the plan structure.
    pub plan_hash: String,
    /// Estimated cost of the query.
    pub cost: f64,
    /// When this entry was recorded.
    pub timestamp: DateTime<Utc>,
    /// Optional metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl QueryEntry {
    /// Create a new query entry.
    pub fn new(
        query_id: String,
        sql: String,
        plan_hash: String,
        cost: f64,
    ) -> Self {
        Self {
            query_id,
            sql,
            plan_hash,
            cost,
            timestamp: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the entry.
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Manages cost history for queries.
#[derive(Debug, Default)]
pub struct CostHistory {
    /// Map from query_id to list of historical entries.
    entries: HashMap<String, Vec<QueryEntry>>,
}

impl CostHistory {
    /// Create a new empty cost history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new entry to the history.
    pub fn add_entry(&mut self, entry: QueryEntry) {
        self.entries
            .entry(entry.query_id.clone())
            .or_default()
            .push(entry);
    }

    /// Get all entries for a specific query.
    pub fn get_entries(&self, query_id: &str) -> Option<&[QueryEntry]> {
        self.entries.get(query_id).map(Vec::as_slice)
    }

    /// Get the most recent entry for a query.
    pub fn get_latest(&self, query_id: &str) -> Option<&QueryEntry> {
        self.entries
            .get(query_id)
            .and_then(|entries| entries.last())
    }

    /// Get the baseline (first) entry for a query.
    pub fn get_baseline(&self, query_id: &str) -> Option<&QueryEntry> {
        self.entries
            .get(query_id)
            .and_then(|entries| entries.first())
    }

    /// Get average cost over a time window.
    pub fn get_average_cost(&self, query_id: &str, last_n: usize) -> Option<f64> {
        let entries = self.entries.get(query_id)?;
        if entries.is_empty() {
            return None;
        }

        let start_idx = entries.len().saturating_sub(last_n);
        let window = &entries[start_idx..];

        let sum: f64 = window.iter().map(|e| e.cost).sum();
        Some(sum / window.len() as f64)
    }

    /// Get all query IDs in the history.
    pub fn query_ids(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// Remove old entries beyond a certain count per query.
    pub fn prune(&mut self, max_entries_per_query: usize) {
        for entries in self.entries.values_mut() {
            if entries.len() > max_entries_per_query {
                let drain_count = entries.len() - max_entries_per_query;
                entries.drain(..drain_count);
            }
        }
    }

    /// Merge another history into this one.
    pub fn merge(&mut self, other: CostHistory) {
        for (query_id, entries) in other.entries {
            self.entries
                .entry(query_id)
                .or_default()
                .extend(entries);
        }
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Get the total number of entries across all queries.
    pub fn total_entries(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_history_basic_operations() {
        let mut history = CostHistory::new();

        let entry1 = QueryEntry::new(
            "q1".to_string(),
            "SELECT * FROM t".to_string(),
            "hash1".to_string(),
            100.0,
        );

        let entry2 = QueryEntry::new(
            "q1".to_string(),
            "SELECT * FROM t".to_string(),
            "hash1".to_string(),
            120.0,
        );

        history.add_entry(entry1.clone());
        history.add_entry(entry2.clone());

        assert_eq!(history.get_entries("q1").unwrap().len(), 2);
        assert_eq!(history.get_baseline("q1").unwrap().cost, 100.0);
        assert_eq!(history.get_latest("q1").unwrap().cost, 120.0);
        assert_eq!(history.get_average_cost("q1", 2).unwrap(), 110.0);
    }

    #[test]
    fn test_cost_history_pruning() {
        let mut history = CostHistory::new();

        for i in 0..5 {
            let entry = QueryEntry::new(
                "q1".to_string(),
                "SELECT * FROM t".to_string(),
                "hash1".to_string(),
                100.0 + i as f64,
            );
            history.add_entry(entry);
        }

        assert_eq!(history.get_entries("q1").unwrap().len(), 5);

        history.prune(3);

        assert_eq!(history.get_entries("q1").unwrap().len(), 3);
        // Should keep the most recent 3
        assert_eq!(history.get_entries("q1").unwrap()[0].cost, 102.0);
    }
}