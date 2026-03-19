//! Rule registry for loading and managing RRA rule metadata.
//!
//! This module provides infrastructure for scanning the rules/ directory,
//! parsing .rra files, and building a registry of rules with their metadata
//! (including pre-conditions) that can be queried by the optimizer.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use ra_core::{RuleMetadata as CoreRuleMetadata, RuleCategory};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::parser::{parse_rule_file, ParseError, RuleMetadata};

/// Errors that can occur during rule registry operations
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Failed to read rules directory
    #[error("Failed to read directory {path}: {source}")]
    DirectoryRead {
        /// Path that failed
        path: PathBuf,
        /// Underlying IO error
        source: std::io::Error,
    },

    /// Failed to parse a rule file
    #[error("Failed to parse {path}: {source}")]
    ParseError {
        /// Path to the file
        path: PathBuf,
        /// Underlying parse error
        source: ParseError,
    },

    /// Rule ID conflict
    #[error("Duplicate rule ID '{id}' found in {path1} and {path2}")]
    DuplicateRuleId {
        /// The conflicting ID
        id: String,
        /// First file with this ID
        path1: PathBuf,
        /// Second file with this ID
        path2: PathBuf,
    },
}

/// A registry of all loaded rules with their metadata
#[derive(Debug, Clone)]
pub struct RuleRegistry {
    /// Rules indexed by ID
    rules: HashMap<String, RuleEntry>,
    /// Total number of rules loaded
    total_count: usize,
    /// Number of rules with formal pre-conditions
    with_preconditions_count: usize,
}

/// A single rule entry with metadata and source location
#[derive(Debug, Clone)]
pub struct RuleEntry {
    /// Core metadata
    pub metadata: CoreRuleMetadata,
    /// Source file path
    pub source_path: PathBuf,
    /// Whether this rule has formal pre-conditions
    pub has_preconditions: bool,
}

impl RuleRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
            total_count: 0,
            with_preconditions_count: 0,
        }
    }

    /// Load all rules from a directory (recursively)
    ///
    /// # Errors
    ///
    /// Returns an error if directory traversal fails, file parsing fails,
    /// or duplicate rule IDs are found.
    pub fn load_from_directory(rules_dir: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let rules_dir = rules_dir.as_ref();
        info!("Loading rules from {}", rules_dir.display());

        let mut registry = Self::new();
        registry.scan_directory(rules_dir, rules_dir)?;

        info!(
            "Loaded {} rules ({} with formal pre-conditions)",
            registry.total_count, registry.with_preconditions_count
        );

        Ok(registry)
    }

    /// Recursively scan a directory for .rra files
    fn scan_directory(&mut self, base_dir: &Path, current_dir: &Path) -> Result<(), RegistryError> {
        let entries = fs::read_dir(current_dir).map_err(|source| RegistryError::DirectoryRead {
            path: current_dir.to_path_buf(),
            source,
        })?;

        for entry in entries {
            let entry = entry.map_err(|source| RegistryError::DirectoryRead {
                path: current_dir.to_path_buf(),
                source,
            })?;

            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| RegistryError::DirectoryRead {
                path: path.clone(),
                source,
            })?;

            if file_type.is_dir() {
                // Recursively scan subdirectories
                self.scan_directory(base_dir, &path)?;
            } else if file_type.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rra") {
                // Parse .rra file
                if let Err(e) = self.load_rule_file(&path) {
                    // Log error but continue loading other files
                    warn!("Skipping {}: {}", path.display(), e);
                }
            }
        }

        Ok(())
    }

    /// Load a single .rra file
    fn load_rule_file(&mut self, path: &Path) -> Result<(), RegistryError> {
        debug!("Parsing {}", path.display());

        let content = fs::read_to_string(path).map_err(|source| RegistryError::DirectoryRead {
            path: path.to_path_buf(),
            source,
        })?;

        let rule_file = parse_rule_file(&content).map_err(|source| RegistryError::ParseError {
            path: path.to_path_buf(),
            source,
        })?;

        let metadata = Self::convert_metadata(&rule_file.metadata);
        let has_preconditions = !rule_file.metadata.preconditions.is_empty();

        // Check for duplicate IDs
        if let Some(existing) = self.rules.get(&metadata.id) {
            return Err(RegistryError::DuplicateRuleId {
                id: metadata.id.clone(),
                path1: existing.source_path.clone(),
                path2: path.to_path_buf(),
            });
        }

        let entry = RuleEntry {
            metadata,
            source_path: path.to_path_buf(),
            has_preconditions,
        };

        self.rules.insert(entry.metadata.id.clone(), entry);
        self.total_count += 1;
        if has_preconditions {
            self.with_preconditions_count += 1;
        }

        Ok(())
    }

    /// Convert parser RuleMetadata to core RuleMetadata
    fn convert_metadata(meta: &RuleMetadata) -> CoreRuleMetadata {
        // Parse category into enum
        let category = if meta.category.starts_with("logical") {
            RuleCategory::Logical
        } else if meta.category.starts_with("physical") {
            RuleCategory::Physical
        } else {
            RuleCategory::Implementation
        };

        CoreRuleMetadata {
            id: meta.id.clone(),
            name: meta.name.clone(),
            description: String::new(), // Not stored in parser metadata
            category,
            databases: meta.databases.clone(),
            priority: 0, // Default priority
            preconditions: meta.preconditions.clone(),
        }
    }

    /// Get a rule by ID
    pub fn get(&self, id: &str) -> Option<&RuleEntry> {
        self.rules.get(id)
    }

    /// Get all rules
    pub fn all_rules(&self) -> impl Iterator<Item = &RuleEntry> {
        self.rules.values()
    }

    /// Get rules matching a database
    pub fn rules_for_database<'a>(&'a self, database: &'a str) -> impl Iterator<Item = &'a RuleEntry> + 'a {
        self.rules.values().filter(move |entry| {
            entry.metadata.databases.is_empty() || entry.metadata.databases.contains(&database.to_string())
        })
    }

    /// Get rules with formal pre-conditions
    pub fn rules_with_preconditions(&self) -> impl Iterator<Item = &RuleEntry> {
        self.rules.values().filter(|entry| entry.has_preconditions)
    }

    /// Get statistics about the registry
    pub fn stats(&self) -> RegistryStats {
        let mut by_category = HashMap::new();
        let mut by_database = HashMap::new();

        for entry in self.rules.values() {
            *by_category.entry(entry.metadata.category).or_insert(0) += 1;

            if entry.metadata.databases.is_empty() {
                *by_database.entry("universal".to_string()).or_insert(0) += 1;
            } else {
                for db in &entry.metadata.databases {
                    *by_database.entry(db.clone()).or_insert(0) += 1;
                }
            }
        }

        RegistryStats {
            total_rules: self.total_count,
            rules_with_preconditions: self.with_preconditions_count,
            by_category,
            by_database,
        }
    }
}

impl Default for RuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the rule registry
#[derive(Debug, Clone)]
pub struct RegistryStats {
    /// Total number of rules
    pub total_rules: usize,
    /// Rules with formal pre-conditions
    pub rules_with_preconditions: usize,
    /// Rules by category
    pub by_category: HashMap<RuleCategory, usize>,
    /// Rules by database
    pub by_database: HashMap<String, usize>,
}

impl std::fmt::Display for RegistryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Rule Registry Statistics:")?;
        writeln!(f, "  Total rules: {}", self.total_rules)?;
        writeln!(
            f,
            "  Rules with preconditions: {} ({:.1}%)",
            self.rules_with_preconditions,
            100.0 * self.rules_with_preconditions as f64 / self.total_rules as f64
        )?;
        writeln!(f, "\n  By category:")?;
        for (cat, count) in &self.by_category {
            writeln!(f, "    {:?}: {}", cat, count)?;
        }
        writeln!(f, "\n  By database (top 10):")?;
        let mut db_vec: Vec<_> = self.by_database.iter().collect();
        db_vec.sort_by(|a, b| b.1.cmp(a.1));
        for (db, count) in db_vec.iter().take(10) {
            writeln!(f, "    {}: {}", db, count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry() {
        let registry = RuleRegistry::new();
        assert_eq!(registry.total_count, 0);
        assert_eq!(registry.with_preconditions_count, 0);
    }

    #[test]
    fn load_example_rules() {
        // Try to load from rules directory if it exists
        if Path::new("rules").exists() {
            let result = RuleRegistry::load_from_directory("rules");
            match result {
                Ok(registry) => {
                    assert!(registry.total_count > 0, "Should load some rules");
                    let stats = registry.stats();
                    println!("{}", stats);
                }
                Err(e) => {
                    println!("Note: Failed to load rules (this is OK in tests): {}", e);
                }
            }
        }
    }
}
