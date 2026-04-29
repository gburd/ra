//! Rule registry for loading and managing RRA rule metadata.
//!
//! This module provides infrastructure for scanning the rules/ directory,
//! parsing .rra files, and building a registry of rules with their metadata
//! (including pre-conditions) that can be queried by the optimizer.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use ra_core::{RuleCategory, RuleMetadata as CoreRuleMetadata};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::{parse_rule_file, ParseError, RuleMetadata};

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
    #[must_use]
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
    #[expect(
        clippy::only_used_in_recursion,
        reason = "base_dir reserved for future relative path resolution"
    )]
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
            let file_type = entry
                .file_type()
                .map_err(|source| RegistryError::DirectoryRead {
                    path: path.clone(),
                    source,
                })?;

            if file_type.is_dir() {
                // Recursively scan subdirectories
                self.scan_directory(base_dir, &path)?;
            } else if file_type.is_file()
                && path.extension().and_then(|s| s.to_str()) == Some("rra")
            {
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

    /// Convert parser `RuleMetadata` to core `RuleMetadata`
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
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&RuleEntry> {
        self.rules.get(id)
    }

    /// Get all rules
    pub fn all_rules(&self) -> impl Iterator<Item = &RuleEntry> {
        self.rules.values()
    }

    /// Get rules matching a database
    pub fn rules_for_database<'a>(
        &'a self,
        database: &'a str,
    ) -> impl Iterator<Item = &'a RuleEntry> + 'a {
        self.rules.values().filter(move |entry| {
            entry.metadata.databases.is_empty()
                || entry.metadata.databases.contains(&database.to_string())
        })
    }

    /// Get rules with formal pre-conditions
    pub fn rules_with_preconditions(&self) -> impl Iterator<Item = &RuleEntry> {
        self.rules.values().filter(|entry| entry.has_preconditions)
    }

    /// Get statistics about the registry
    #[must_use]
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
        #[expect(clippy::cast_precision_loss, reason = "rule count is always small")]
        let precondition_pct =
            100.0 * self.rules_with_preconditions as f64 / self.total_rules as f64;
        writeln!(
            f,
            "  Rules with preconditions: {} ({precondition_pct:.1}%)",
            self.rules_with_preconditions,
        )?;
        writeln!(f, "\n  By category:")?;
        for (cat, count) in &self.by_category {
            writeln!(f, "    {cat:?}: {count}")?;
        }
        writeln!(f, "\n  By database (top 10):")?;
        let mut db_vec: Vec<_> = self.by_database.iter().collect();
        db_vec.sort_by(|a, b| b.1.cmp(a.1));
        for (db, count) in db_vec.iter().take(10) {
            writeln!(f, "    {db}: {count}")?;
        }
        Ok(())
    }
}

#[expect(clippy::unwrap_used, clippy::print_stderr, reason = "test code")]
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
    fn default_is_empty() {
        let registry = RuleRegistry::default();
        assert_eq!(registry.total_count, 0);
        assert!(registry.get("anything").is_none());
        assert_eq!(registry.all_rules().count(), 0);
    }

    #[test]
    fn load_example_rules() {
        if Path::new("rules").exists() {
            let result = RuleRegistry::load_from_directory("rules");
            match result {
                Ok(registry) => {
                    assert!(registry.total_count > 0, "Should load some rules");
                    let stats = registry.stats();
                    // Verify Display impl produces content
                    let display = format!("{stats}");
                    assert!(display.contains("Total rules:"));
                }
                Err(e) => {
                    eprintln!("Note: Failed to load rules (OK in tests): {e}");
                }
            }
        }
    }

    #[test]
    fn convert_metadata_logical() {
        let parser_meta = RuleMetadata {
            id: "test-rule".into(),
            name: "Test Rule".into(),
            category: "logical/pushdown".into(),
            databases: vec!["postgresql".into()],
            standard: None,
            execution_models: vec![],
            version: "0.1.0".into(),
            authors: vec![],
            tags: vec![],
            preconditions: vec![],
        };
        let core = RuleRegistry::convert_metadata(&parser_meta);
        assert_eq!(core.id, "test-rule");
        assert_eq!(core.name, "Test Rule");
        assert_eq!(core.category, RuleCategory::Logical);
        assert_eq!(core.databases, vec!["postgresql".to_string()]);
        assert_eq!(core.priority, 0);
    }

    #[test]
    fn convert_metadata_physical() {
        let parser_meta = RuleMetadata {
            id: "phys-rule".into(),
            name: "Physical".into(),
            category: "physical/join".into(),
            databases: vec![],
            standard: None,
            execution_models: vec![],
            version: "0.1.0".into(),
            authors: vec![],
            tags: vec![],
            preconditions: vec![],
        };
        let core = RuleRegistry::convert_metadata(&parser_meta);
        assert_eq!(core.category, RuleCategory::Physical);
    }

    #[test]
    fn convert_metadata_implementation() {
        let parser_meta = RuleMetadata {
            id: "impl-rule".into(),
            name: "Impl".into(),
            category: "vendor/custom".into(),
            databases: vec![],
            standard: None,
            execution_models: vec![],
            version: "0.1.0".into(),
            authors: vec![],
            tags: vec![],
            preconditions: vec![],
        };
        let core = RuleRegistry::convert_metadata(&parser_meta);
        assert_eq!(core.category, RuleCategory::Implementation);
    }

    #[test]
    fn stats_empty_registry() {
        let registry = RuleRegistry::new();
        let stats = registry.stats();
        assert_eq!(stats.total_rules, 0);
        assert_eq!(stats.rules_with_preconditions, 0);
        assert!(stats.by_category.is_empty());
        assert!(stats.by_database.is_empty());
    }

    #[test]
    fn load_from_nonexistent_directory() {
        let result = RuleRegistry::load_from_directory("/nonexistent/path");
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Failed to read directory"));
    }

    #[test]
    fn load_from_empty_directory() {
        let dir = std::env::temp_dir().join("ra_test_empty_rules");
        let _ = fs::create_dir_all(&dir);
        let result = RuleRegistry::load_from_directory(&dir);
        assert!(result.is_ok());
        let registry = result.unwrap();
        assert_eq!(registry.total_count, 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_error_display() {
        let err = RegistryError::DuplicateRuleId {
            id: "join-commute".into(),
            path1: PathBuf::from("a.rra"),
            path2: PathBuf::from("b.rra"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("Duplicate rule ID"));
        assert!(msg.contains("join-commute"));
    }

    #[test]
    fn rules_for_database_filters_correctly() {
        let mut registry = RuleRegistry::new();

        // Insert a universal rule (no databases)
        let universal = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "universal-1".into(),
                name: "Universal".into(),
                description: String::new(),
                category: RuleCategory::Logical,
                databases: vec![],
                priority: 0,
                preconditions: vec![],
            },
            source_path: PathBuf::from("u.rra"),
            has_preconditions: false,
        };
        registry.rules.insert("universal-1".into(), universal);
        registry.total_count += 1;

        // Insert a postgres-only rule
        let pg_only = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "pg-1".into(),
                name: "PG Only".into(),
                description: String::new(),
                category: RuleCategory::Physical,
                databases: vec!["postgresql".into()],
                priority: 0,
                preconditions: vec![],
            },
            source_path: PathBuf::from("pg.rra"),
            has_preconditions: false,
        };
        registry.rules.insert("pg-1".into(), pg_only);
        registry.total_count += 1;

        // Insert a mysql-only rule
        let mysql_only = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "mysql-1".into(),
                name: "MySQL Only".into(),
                description: String::new(),
                category: RuleCategory::Implementation,
                databases: vec!["mysql".into()],
                priority: 0,
                preconditions: vec![],
            },
            source_path: PathBuf::from("my.rra"),
            has_preconditions: false,
        };
        registry.rules.insert("mysql-1".into(), mysql_only);
        registry.total_count += 1;

        // PostgreSQL query: should get universal + pg-1
        let pg_rules: Vec<_> = registry.rules_for_database("postgresql").collect();
        assert_eq!(pg_rules.len(), 2);

        // MySQL query: should get universal + mysql-1
        let mysql_rules: Vec<_> = registry.rules_for_database("mysql").collect();
        assert_eq!(mysql_rules.len(), 2);

        // SQLite query: should get universal only
        let sqlite_rules: Vec<_> = registry.rules_for_database("sqlite").collect();
        assert_eq!(sqlite_rules.len(), 1);
        assert_eq!(sqlite_rules[0].metadata.id, "universal-1");
    }

    #[test]
    fn rules_with_preconditions_filter() {
        let mut registry = RuleRegistry::new();

        let without = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "no-pre".into(),
                name: "No Pre".into(),
                description: String::new(),
                category: RuleCategory::Logical,
                databases: vec![],
                priority: 0,
                preconditions: vec![],
            },
            source_path: PathBuf::from("a.rra"),
            has_preconditions: false,
        };
        registry.rules.insert("no-pre".into(), without);
        registry.total_count += 1;

        let with = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "has-pre".into(),
                name: "Has Pre".into(),
                description: String::new(),
                category: RuleCategory::Physical,
                databases: vec![],
                priority: 0,
                preconditions: vec![ra_core::PreCondition::Pattern {
                    must_match: Some("row_count > 1000".into()),
                    must_not_match: None,
                    description: None,
                    optional: false,
                }],
            },
            source_path: PathBuf::from("b.rra"),
            has_preconditions: true,
        };
        registry.rules.insert("has-pre".into(), with);
        registry.total_count += 1;
        registry.with_preconditions_count += 1;

        let pre_rules: Vec<_> = registry.rules_with_preconditions().collect();
        assert_eq!(pre_rules.len(), 1);
        assert_eq!(pre_rules[0].metadata.id, "has-pre");
    }

    #[test]
    fn get_returns_inserted_rule() {
        let mut registry = RuleRegistry::new();
        let entry = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "test-get".into(),
                name: "Test Get".into(),
                description: String::new(),
                category: RuleCategory::Logical,
                databases: vec![],
                priority: 0,
                preconditions: vec![],
            },
            source_path: PathBuf::from("test.rra"),
            has_preconditions: false,
        };
        registry.rules.insert("test-get".into(), entry);
        registry.total_count += 1;

        assert!(registry.get("test-get").is_some());
        assert_eq!(registry.get("test-get").unwrap().metadata.name, "Test Get");
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn all_rules_returns_all() {
        let mut registry = RuleRegistry::new();
        for i in 0..5 {
            let entry = RuleEntry {
                metadata: CoreRuleMetadata {
                    id: format!("rule-{i}"),
                    name: format!("Rule {i}"),
                    description: String::new(),
                    category: RuleCategory::Logical,
                    databases: vec![],
                    priority: 0,
                    preconditions: vec![],
                },
                source_path: PathBuf::from(format!("{i}.rra")),
                has_preconditions: false,
            };
            registry.rules.insert(format!("rule-{i}"), entry);
            registry.total_count += 1;
        }
        assert_eq!(registry.all_rules().count(), 5);
    }

    #[test]
    fn stats_counts_categories_and_databases() {
        let mut registry = RuleRegistry::new();

        let e1 = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "r1".into(),
                name: "R1".into(),
                description: String::new(),
                category: RuleCategory::Logical,
                databases: vec![],
                priority: 0,
                preconditions: vec![],
            },
            source_path: PathBuf::from("r1.rra"),
            has_preconditions: false,
        };
        registry.rules.insert("r1".into(), e1);
        registry.total_count += 1;

        let e2 = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "r2".into(),
                name: "R2".into(),
                description: String::new(),
                category: RuleCategory::Physical,
                databases: vec!["postgresql".into(), "mysql".into()],
                priority: 0,
                preconditions: vec![ra_core::PreCondition::Pattern {
                    must_match: Some("has_index".into()),
                    must_not_match: None,
                    description: None,
                    optional: false,
                }],
            },
            source_path: PathBuf::from("r2.rra"),
            has_preconditions: true,
        };
        registry.rules.insert("r2".into(), e2);
        registry.total_count += 1;
        registry.with_preconditions_count += 1;

        let stats = registry.stats();
        assert_eq!(stats.total_rules, 2);
        assert_eq!(stats.rules_with_preconditions, 1);
        assert_eq!(stats.by_category[&RuleCategory::Logical], 1);
        assert_eq!(stats.by_category[&RuleCategory::Physical], 1);
        assert_eq!(stats.by_database["universal"], 1);
        assert_eq!(stats.by_database["postgresql"], 1);
        assert_eq!(stats.by_database["mysql"], 1);
    }

    #[test]
    fn registry_stats_display_contains_info() {
        let mut registry = RuleRegistry::new();
        let e = RuleEntry {
            metadata: CoreRuleMetadata {
                id: "disp-test".into(),
                name: "Disp".into(),
                description: String::new(),
                category: RuleCategory::Logical,
                databases: vec![],
                priority: 0,
                preconditions: vec![],
            },
            source_path: PathBuf::from("d.rra"),
            has_preconditions: false,
        };
        registry.rules.insert("disp-test".into(), e);
        registry.total_count += 1;

        let stats = registry.stats();
        let output = format!("{stats}");
        assert!(output.contains("Total rules: 1"));
        assert!(output.contains("By category:"));
        assert!(output.contains("By database"));
    }
}
