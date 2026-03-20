//! Rule metadata parsing and pre-condition evaluation.
//!
//! Parses .rra files (Relational Rewrite Rules) with YAML frontmatter
//! containing preconditions, and filters rules based on runtime facts.

use anyhow::{anyhow, Context, Result};
use ra_core::facts::FactsProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{debug, warn};

/// Metadata for a rewrite rule from .rra file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMetadata {
    /// Unique rule identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Rule category (e.g., "logical/predicate-pushdown")
    pub category: String,
    /// Databases this rule applies to
    #[serde(default)]
    pub databases: Vec<String>,
    /// SQL standard version (e.g., "sql:1992")
    #[serde(default)]
    pub standard: Option<String>,
    /// Rule version
    #[serde(default)]
    pub version: String,
    /// Rule authors
    #[serde(default)]
    pub authors: Vec<String>,
    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,
    /// Pre-conditions that must be satisfied
    #[serde(default)]
    pub preconditions: Vec<Precondition>,
}

/// A precondition that must be satisfied for a rule to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Precondition {
    /// Pattern must match the expression tree
    Pattern {
        must_match: String,
        description: String,
    },
    /// Runtime predicate check
    Predicate {
        condition: String,
        description: String,
    },
    /// Hardware requirement
    Hardware {
        requirement: String,
        description: String,
    },
    /// Database system requirement
    Database {
        system: String,
        description: String,
    },
    /// Feature flag requirement
    Feature {
        flag: String,
        description: String,
    },
}

/// Parsed rule metadata with content.
#[derive(Debug, Clone)]
pub struct ParsedRule {
    pub metadata: RuleMetadata,
    pub content: String,
}

/// Parse a .rra file with YAML frontmatter.
///
/// # Format
/// ```text
/// ---
/// id: rule-name
/// name: Rule Name
/// category: logical/predicate-pushdown
/// preconditions:
///   - type: database
///     system: postgresql
///     description: "PostgreSQL-specific optimization"
/// ---
///
/// # Rule documentation...
/// ```
pub fn parse_rra_file(path: &Path) -> Result<ParsedRule> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read rule file: {}", path.display()))?;

    // Split frontmatter and content
    let parts: Vec<&str> = contents.splitn(3, "---").collect();
    if parts.len() < 3 {
        return Err(anyhow!(
            "Invalid .rra file format (expected YAML frontmatter): {}",
            path.display()
        ));
    }

    let frontmatter = parts[1].trim();
    let content = parts[2].trim();

    // Parse YAML frontmatter
    let metadata: RuleMetadata = serde_yaml::from_str(frontmatter)
        .with_context(|| format!("Failed to parse YAML frontmatter in {}", path.display()))?;

    Ok(ParsedRule {
        metadata,
        content: content.to_string(),
    })
}

/// Load all .rra files from a directory recursively.
pub fn load_rules_from_directory(dir: &Path) -> Result<Vec<ParsedRule>> {
    let mut rules = Vec::new();

    if !dir.exists() {
        warn!("Rules directory not found: {}", dir.display());
        return Ok(rules);
    }

    for entry in walkdir::WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "rra" {
                    match parse_rra_file(entry.path()) {
                        Ok(rule) => {
                            debug!("Loaded rule: {} from {}", rule.metadata.id, entry.path().display());
                            rules.push(rule);
                        }
                        Err(e) => {
                            warn!("Failed to parse {}: {}", entry.path().display(), e);
                        }
                    }
                }
            }
        }
    }

    debug!("Loaded {} rules from {}", rules.len(), dir.display());
    Ok(rules)
}

/// Filter rules based on runtime facts and preconditions.
pub fn filter_rules_by_preconditions(
    rules: &[ParsedRule],
    facts: &dyn FactsProvider,
) -> Vec<String> {
    let mut applicable_rules = Vec::new();

    for rule in rules {
        if is_rule_applicable(&rule.metadata, facts) {
            applicable_rules.push(rule.metadata.id.clone());
        }
    }

    debug!(
        "Filtered to {}/{} applicable rules",
        applicable_rules.len(),
        rules.len()
    );

    applicable_rules
}

/// Check if a rule is applicable given the current facts.
fn is_rule_applicable(metadata: &RuleMetadata, facts: &dyn FactsProvider) -> bool {
    // Check database compatibility
    if !metadata.databases.is_empty() {
        let db_name = facts.database_name();
        let db_matches = metadata.databases.iter().any(|db| {
            db.eq_ignore_ascii_case(&db_name) || db == "all"
        });
        if !db_matches {
            debug!(
                "Rule {} filtered: database {} not in {:?}",
                metadata.id,
                db_name,
                metadata.databases
            );
            return false;
        }
    }

    // Evaluate each precondition
    for precond in &metadata.preconditions {
        if !evaluate_precondition(precond, facts) {
            debug!(
                "Rule {} filtered: precondition failed: {:?}",
                metadata.id, precond
            );
            return false;
        }
    }

    true
}

/// Evaluate a single precondition against runtime facts.
fn evaluate_precondition(precond: &Precondition, facts: &dyn FactsProvider) -> bool {
    match precond {
        Precondition::Pattern { .. } => {
            // Pattern matching happens at rewrite time, not pre-filtering
            true
        }
        Precondition::Predicate { condition, .. } => {
            // Parse and evaluate runtime predicates
            evaluate_predicate_condition(condition, facts)
        }
        Precondition::Hardware { requirement, .. } => {
            evaluate_hardware_requirement(requirement, facts)
        }
        Precondition::Database { system, .. } => {
            let db_name = facts.database_name();
            system.eq_ignore_ascii_case(&db_name)
        }
        Precondition::Feature { flag, .. } => {
            // Feature flags evaluation
            evaluate_feature_flag(flag, facts)
        }
    }
}

/// Evaluate a predicate condition string.
///
/// Supports simple expressions like:
/// - "has_gpu()"
/// - "cpu_cores() >= 4"
/// - "available_memory() > 8000000000"
fn evaluate_predicate_condition(condition: &str, facts: &dyn FactsProvider) -> bool {
    let condition = condition.trim();

    // Check for function calls
    if condition.starts_with("has_gpu()") {
        return facts.has_gpu();
    }

    if condition.starts_with("has_fpga()") {
        // TODO: Add FPGA support to FactsProvider
        warn!("FPGA hardware detection not yet implemented, assuming false");
        return false;
    }

    // Parse comparison expressions
    if let Some((func, op, value)) = parse_comparison(condition) {
        return evaluate_comparison(&func, &op, &value, facts);
    }

    // Default: assume true for unknown predicates (conservative)
    warn!("Unknown predicate condition: {}, assuming true", condition);
    true
}

/// Parse a comparison expression like "cpu_cores() >= 4".
fn parse_comparison(expr: &str) -> Option<(String, String, String)> {
    let operators = [">=", "<=", "==", "!=", ">", "<"];
    for op in &operators {
        if let Some(idx) = expr.find(op) {
            let func = expr[..idx].trim().to_string();
            let value = expr[idx + op.len()..].trim().to_string();
            return Some((func, op.to_string(), value));
        }
    }
    None
}

/// Evaluate a comparison expression.
fn evaluate_comparison(func: &str, op: &str, value_str: &str, facts: &dyn FactsProvider) -> bool {
    // Get the actual value from facts
    let actual_value: f64 = match func {
        "cpu_cores()" => facts.cpu_cores() as f64,
        "available_memory()" => facts.available_memory() as f64,
        _ => {
            warn!("Unknown function in comparison: {}", func);
            return true;
        }
    };

    // Parse expected value
    let expected_value: f64 = match value_str.parse() {
        Ok(v) => v,
        Err(_) => {
            warn!("Failed to parse comparison value: {}", value_str);
            return true;
        }
    };

    // Perform comparison
    match op {
        ">=" => actual_value >= expected_value,
        "<=" => actual_value <= expected_value,
        "==" => (actual_value - expected_value).abs() < f64::EPSILON,
        "!=" => (actual_value - expected_value).abs() >= f64::EPSILON,
        ">" => actual_value > expected_value,
        "<" => actual_value < expected_value,
        _ => {
            warn!("Unknown comparison operator: {}", op);
            true
        }
    }
}

/// Evaluate hardware requirements.
fn evaluate_hardware_requirement(requirement: &str, facts: &dyn FactsProvider) -> bool {
    let req = requirement.to_lowercase();
    if req.contains("gpu") {
        facts.has_gpu()
    } else if req.contains("fpga") {
        // TODO: Add FPGA support to FactsProvider
        warn!("FPGA hardware detection not yet implemented, assuming false");
        false
    } else if req.contains("cpu") {
        // CPU always available
        true
    } else {
        warn!("Unknown hardware requirement: {}", requirement);
        true
    }
}

/// Evaluate feature flags.
fn evaluate_feature_flag(flag: &str, _facts: &dyn FactsProvider) -> bool {
    // For now, assume all features are enabled
    // TODO: Add feature flag configuration
    debug!("Feature flag check: {} (assuming enabled)", flag);
    true
}

/// Build a rule ID -> RuleMetadata map for quick lookup.
pub fn build_metadata_index(rules: &[ParsedRule]) -> HashMap<String, RuleMetadata> {
    rules
        .iter()
        .map(|r| (r.metadata.id.clone(), r.metadata.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::facts::StaticFacts;
    use ra_core::SQLDialect;

    #[test]
    fn test_parse_comparison() {
        assert_eq!(
            parse_comparison("cpu_cores() >= 4"),
            Some(("cpu_cores()".to_string(), ">=".to_string(), "4".to_string()))
        );
        assert_eq!(
            parse_comparison("available_memory() > 8000000000"),
            Some((
                "available_memory()".to_string(),
                ">".to_string(),
                "8000000000".to_string()
            ))
        );
    }

    #[test]
    fn test_evaluate_comparison() {
        let facts = StaticFacts::new(
            "test".to_string(),
            SQLDialect::PostgreSQL,
            4,
            8_000_000_000,
            false,
            false,
        );

        assert!(evaluate_comparison("cpu_cores()", ">=", "4", &facts));
        assert!(!evaluate_comparison("cpu_cores()", ">", "4", &facts));
        assert!(evaluate_comparison("available_memory()", ">=", "8000000000", &facts));
    }

    #[test]
    fn test_filter_by_database() {
        let metadata = RuleMetadata {
            id: "test-rule".to_string(),
            name: "Test Rule".to_string(),
            category: "logical".to_string(),
            databases: vec!["postgresql".to_string()],
            standard: None,
            version: "1.0.0".to_string(),
            authors: vec![],
            tags: vec![],
            preconditions: vec![],
        };

        let facts_pg = StaticFacts::new(
            "PostgreSQL".to_string(),
            SQLDialect::PostgreSQL,
            4,
            8_000_000_000,
            false,
            false,
        );
        assert!(is_rule_applicable(&metadata, &facts_pg));

        let facts_mysql = StaticFacts::new(
            "MySQL".to_string(),
            SQLDialect::MySQL,
            4,
            8_000_000_000,
            false,
            false,
        );
        assert!(!is_rule_applicable(&metadata, &facts_mysql));
    }
}
