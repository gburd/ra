//! Storage backends for cost history.

use crate::history::{CostHistory, QueryEntry};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] toml::ser::Error),
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] toml::de::Error),
    #[error("Storage not found: {0}")]
    NotFound(PathBuf),
}

/// Storage backend for cost history.
pub trait Storage: Send + Sync {
    /// Load cost history from storage.
    fn load(&self) -> Result<CostHistory, StorageError>;

    /// Save cost history to storage.
    fn save(&self, history: &CostHistory) -> Result<(), StorageError>;

    /// Add a single entry without loading full history.
    fn add_entry(&self, entry: QueryEntry) -> Result<(), StorageError>;

    /// Get entries for a specific query.
    fn get_entries(&self, query_id: &str) -> Result<Vec<QueryEntry>, StorageError>;
}

/// SQLite-based storage backend.
pub struct SqliteStorage {
    path: PathBuf,
}

impl SqliteStorage {
    /// Create a new SQLite storage backend.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Initialize the database schema.
    fn init_schema(&self, conn: &Connection) -> Result<(), StorageError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS query_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                query_id TEXT NOT NULL,
                sql TEXT NOT NULL,
                plan_hash TEXT NOT NULL,
                cost REAL NOT NULL,
                timestamp TEXT NOT NULL,
                metadata TEXT
            )",
            [],
        )?;

        // Create indexes separately (SQLite doesn't support inline INDEX in CREATE TABLE)
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_query_id ON query_history (query_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON query_history (timestamp)",
            [],
        )?;

        Ok(())
    }

    /// Open a connection to the database.
    fn open_connection(&self) -> Result<Connection, StorageError> {
        let conn = Connection::open(&self.path)?;
        self.init_schema(&conn)?;
        Ok(conn)
    }
}

impl Storage for SqliteStorage {
    fn load(&self) -> Result<CostHistory, StorageError> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "SELECT query_id, sql, plan_hash, cost, timestamp, metadata
             FROM query_history
             ORDER BY query_id, timestamp",
        )?;

        let mut history = CostHistory::new();

        let entries = stmt.query_map([], |row| {
            let metadata_json: Option<String> = row.get(5)?;
            let metadata: HashMap<String, String> = metadata_json
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();

            Ok(QueryEntry {
                query_id: row.get(0)?,
                sql: row.get(1)?,
                plan_hash: row.get(2)?,
                cost: row.get(3)?,
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))?
                    .with_timezone(&Utc),
                metadata,
            })
        })?;

        for entry in entries {
            history.add_entry(entry?);
        }

        Ok(history)
    }

    fn save(&self, history: &CostHistory) -> Result<(), StorageError> {
        let conn = self.open_connection()?;

        // Clear existing data
        conn.execute("DELETE FROM query_history", [])?;

        // Insert all entries
        let mut stmt = conn.prepare(
            "INSERT INTO query_history (query_id, sql, plan_hash, cost, timestamp, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        for query_id in history.query_ids() {
            if let Some(entries) = history.get_entries(&query_id) {
                for entry in entries {
                    let metadata_json = if entry.metadata.is_empty() {
                        None
                    } else {
                        Some(serde_json::to_string(&entry.metadata).unwrap_or_default())
                    };

                    stmt.execute(params![
                        entry.query_id,
                        entry.sql,
                        entry.plan_hash,
                        entry.cost,
                        entry.timestamp.to_rfc3339(),
                        metadata_json,
                    ])?;
                }
            }
        }

        Ok(())
    }

    fn add_entry(&self, entry: QueryEntry) -> Result<(), StorageError> {
        let conn = self.open_connection()?;

        let metadata_json = if entry.metadata.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&entry.metadata).unwrap_or_default())
        };

        conn.execute(
            "INSERT INTO query_history (query_id, sql, plan_hash, cost, timestamp, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                entry.query_id,
                entry.sql,
                entry.plan_hash,
                entry.cost,
                entry.timestamp.to_rfc3339(),
                metadata_json,
            ],
        )?;

        Ok(())
    }

    fn get_entries(&self, query_id: &str) -> Result<Vec<QueryEntry>, StorageError> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "SELECT query_id, sql, plan_hash, cost, timestamp, metadata
             FROM query_history
             WHERE query_id = ?1
             ORDER BY timestamp",
        )?;

        let entries = stmt.query_map([query_id], |row| {
            let metadata_json: Option<String> = row.get(5)?;
            let metadata: HashMap<String, String> = metadata_json
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();

            Ok(QueryEntry {
                query_id: row.get(0)?,
                sql: row.get(1)?,
                plan_hash: row.get(2)?,
                cost: row.get(3)?,
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))?
                    .with_timezone(&Utc),
                metadata,
            })
        })?;

        entries.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

/// TOML file-based storage backend.
pub struct TomlStorage {
    path: PathBuf,
}

impl TomlStorage {
    /// Create a new TOML storage backend.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TomlHistory {
    queries: HashMap<String, Vec<QueryEntry>>,
}

impl Storage for TomlStorage {
    fn load(&self) -> Result<CostHistory, StorageError> {
        if !self.path.exists() {
            return Ok(CostHistory::new());
        }

        let contents = std::fs::read_to_string(&self.path)?;
        let toml_history: TomlHistory = toml::from_str(&contents)?;

        let mut history = CostHistory::new();
        for (_query_id, entries) in toml_history.queries {
            for entry in entries {
                history.add_entry(entry);
            }
        }

        Ok(history)
    }

    fn save(&self, history: &CostHistory) -> Result<(), StorageError> {
        let mut queries = HashMap::new();

        for query_id in history.query_ids() {
            if let Some(entries) = history.get_entries(&query_id) {
                queries.insert(query_id, entries.to_vec());
            }
        }

        let toml_history = TomlHistory { queries };
        let contents = toml::to_string_pretty(&toml_history)?;
        std::fs::write(&self.path, contents)?;

        Ok(())
    }

    fn add_entry(&self, entry: QueryEntry) -> Result<(), StorageError> {
        let mut history = self.load()?;
        history.add_entry(entry);
        self.save(&history)
    }

    fn get_entries(&self, query_id: &str) -> Result<Vec<QueryEntry>, StorageError> {
        let history = self.load()?;
        Ok(history
            .get_entries(query_id)
            .map(|entries| entries.to_vec())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_entry(id: &str, sql: &str, hash: &str, cost: f64) -> QueryEntry {
        QueryEntry::new(
            id.to_string(),
            sql.to_string(),
            hash.to_string(),
            cost,
        )
    }

    #[test]
    fn test_sqlite_storage() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = SqliteStorage::new(&path);

        let entry = make_entry("q1", "SELECT * FROM t", "hash1", 100.0);
        storage.add_entry(entry).unwrap();

        let entries = storage.get_entries("q1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].query_id, "q1");
        assert_eq!(entries[0].cost, 100.0);
    }

    #[test]
    fn test_toml_storage() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.toml");
        let storage = TomlStorage::new(&path);

        let entry = make_entry("q1", "SELECT * FROM t", "hash1", 100.0);
        storage.add_entry(entry).unwrap();

        let entries = storage.get_entries("q1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].query_id, "q1");
        assert_eq!(entries[0].cost, 100.0);
    }

    // -- Error types --

    #[test]
    fn storage_error_io_display() {
        let io_err = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        );
        let err: StorageError = io_err.into();
        let msg = format!("{err}");
        assert!(msg.contains("IO error"));
    }

    #[test]
    fn storage_error_not_found_display() {
        let err = StorageError::NotFound(PathBuf::from("/tmp/missing"));
        let msg = format!("{err}");
        assert!(msg.contains("Storage not found"));
        assert!(msg.contains("missing"));
    }

    // -- TOML: load from nonexistent returns empty --

    #[test]
    fn toml_load_nonexistent_returns_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let storage = TomlStorage::new(&path);

        let history = storage.load().unwrap();
        assert!(history.query_ids().is_empty());
    }

    // -- TOML: multiple entries for same query --

    #[test]
    fn toml_multiple_entries_same_query() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("multi.toml");
        let storage = TomlStorage::new(&path);

        storage.add_entry(make_entry("q1", "SELECT 1", "h1", 10.0)).unwrap();
        storage.add_entry(make_entry("q1", "SELECT 1", "h2", 20.0)).unwrap();
        storage.add_entry(make_entry("q1", "SELECT 1", "h3", 30.0)).unwrap();

        let entries = storage.get_entries("q1").unwrap();
        assert_eq!(entries.len(), 3);
    }

    // -- TOML: multiple different queries --

    #[test]
    fn toml_multiple_queries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("multi_q.toml");
        let storage = TomlStorage::new(&path);

        storage.add_entry(make_entry("q1", "SELECT 1", "h1", 10.0)).unwrap();
        storage.add_entry(make_entry("q2", "SELECT 2", "h2", 20.0)).unwrap();

        let history = storage.load().unwrap();
        assert_eq!(history.query_ids().len(), 2);
    }

    // -- TOML: get_entries for missing query returns empty --

    #[test]
    fn toml_get_entries_missing_query() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty_get.toml");
        let storage = TomlStorage::new(&path);

        let entries = storage.get_entries("no_such_query").unwrap();
        assert!(entries.is_empty());
    }

    // -- TOML: save and load round-trip --

    #[test]
    fn toml_save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("roundtrip.toml");
        let storage = TomlStorage::new(&path);

        let mut history = CostHistory::new();
        history.add_entry(make_entry("q1", "SELECT 1", "h1", 10.0));
        history.add_entry(make_entry("q2", "SELECT 2", "h2", 20.0));
        storage.save(&history).unwrap();

        let loaded = storage.load().unwrap();
        let q1 = loaded.get_entries("q1").unwrap();
        assert_eq!(q1.len(), 1);
        assert!((q1[0].cost - 10.0).abs() < f64::EPSILON);

        let q2 = loaded.get_entries("q2").unwrap();
        assert_eq!(q2.len(), 1);
        assert!((q2[0].cost - 20.0).abs() < f64::EPSILON);
    }

    // -- TOML: entry with metadata --

    #[test]
    fn toml_entry_with_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("meta.toml");
        let storage = TomlStorage::new(&path);

        let mut entry = make_entry("q1", "SELECT 1", "h1", 10.0);
        entry.metadata.insert("optimizer".into(), "Ra".into());
        entry.metadata.insert("version".into(), "0.1".into());
        storage.add_entry(entry).unwrap();

        let entries = storage.get_entries("q1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata.get("optimizer").unwrap(), "Ra");
        assert_eq!(entries[0].metadata.get("version").unwrap(), "0.1");
    }

    // -- TOML: save overwrites previous content --

    #[test]
    fn toml_save_overwrites() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("overwrite.toml");
        let storage = TomlStorage::new(&path);

        let mut h1 = CostHistory::new();
        h1.add_entry(make_entry("old", "SELECT old", "h", 1.0));
        storage.save(&h1).unwrap();

        let mut h2 = CostHistory::new();
        h2.add_entry(make_entry("new", "SELECT new", "h", 2.0));
        storage.save(&h2).unwrap();

        let loaded = storage.load().unwrap();
        assert!(loaded.get_entries("old").is_none());
        assert!(loaded.get_entries("new").is_some());
    }

    // -- QueryEntry: new sets timestamp --

    #[test]
    fn query_entry_new_has_timestamp() {
        let entry = make_entry("q", "sql", "h", 1.0);
        let now = Utc::now();
        let diff = now - entry.timestamp;
        assert!(diff.num_seconds() < 2);
    }

    // -- QueryEntry: metadata starts empty --

    #[test]
    fn query_entry_new_metadata_empty() {
        let entry = make_entry("q", "sql", "h", 1.0);
        assert!(entry.metadata.is_empty());
    }
}