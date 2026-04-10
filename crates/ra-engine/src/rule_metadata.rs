//! Rule metadata parsing and pre-condition evaluation.
//!
//! Parses .rra files (Relational Rewrite Rules) with YAML frontmatter
//! containing preconditions, and filters rules based on runtime facts.

use anyhow::{anyhow, Context, Result};
use ra_core::facts::FactsProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;
use tracing::{debug, warn};

/// Algorithmic complexity class for a rewrite rule.
///
/// Used to weight rule application priority: cheaper rules
/// should fire before expensive ones when both have similar
/// expected benefit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(Serialize, Deserialize)]
pub enum ComplexityClass {
    /// Constant-time pattern match and rewrite.
    #[serde(rename = "O(1)")]
    O1,
    /// Linear in the number of matched nodes.
    #[serde(rename = "O(n)")]
    On,
    /// Quadratic (e.g., nested join reordering).
    #[serde(rename = "O(n^2)")]
    On2,
    /// Exponential (e.g., full enumeration).
    #[serde(rename = "O(exp)")]
    Oexp,
}

impl ComplexityClass {
    /// Numeric weight for priority scoring.
    /// Lower complexity gets lower weight, yielding higher priority
    /// when used as the denominator in `benefit / weight`.
    #[must_use]
    pub fn weight(self) -> f64 {
        match self {
            Self::O1 => 1.0,
            Self::On => 2.0,
            Self::On2 => 4.0,
            Self::Oexp => 8.0,
        }
    }
}

impl fmt::Display for ComplexityClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::O1 => write!(f, "O(1)"),
            Self::On => write!(f, "O(n)"),
            Self::On2 => write!(f, "O(n^2)"),
            Self::Oexp => write!(f, "O(exp)"),
        }
    }
}

/// Expected benefit range `(min, max)` on a 0.0..=1.0 scale.
///
/// - 0.0 = no improvement expected
/// - 1.0 = order-of-magnitude improvement possible
///
/// The midpoint `(min + max) / 2` is used for priority scoring.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenefitRange {
    pub min: f64,
    pub max: f64,
}

impl BenefitRange {
    /// Create a new benefit range, clamping to `[0.0, 1.0]`.
    #[must_use]
    pub fn new(min: f64, max: f64) -> Self {
        Self {
            min: min.clamp(0.0, 1.0),
            max: max.clamp(0.0, 1.0),
        }
    }

    /// Expected benefit (midpoint of range).
    #[must_use]
    pub fn expected(&self) -> f64 {
        (self.min + self.max) / 2.0
    }
}

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
    /// Algorithmic complexity of applying this rule
    #[serde(default)]
    pub complexity: Option<ComplexityClass>,
    /// Expected benefit range [min, max] on a 0.0..=1.0 scale
    #[serde(default)]
    pub benefit_range: Option<BenefitRange>,
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
#[cfg(feature = "file-discovery")]
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

#[cfg(not(feature = "file-discovery"))]
pub fn load_rules_from_directory(_dir: &Path) -> Result<Vec<ParsedRule>> {
    Ok(Vec::new())
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
            db.eq_ignore_ascii_case(db_name) || db == "all"
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
            system.eq_ignore_ascii_case(db_name)
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ra_core::facts::{
        FactsProvider, HardwareProfile as CoreHardwareProfile, SqlDialect,
        TableInfo, TableStats as CoreTableStats,
    };
    use ra_core::statistics::ColumnStats;
    use std::time::Duration;

    /// Configurable facts provider for testing rule_metadata.
    struct TestFacts {
        db_name: &'static str,
        hw: CoreHardwareProfile,
    }

    impl TestFacts {
        fn new(db_name: &'static str, cpu: u32, memory: u64, gpu: bool) -> Self {
            Self {
                db_name,
                hw: CoreHardwareProfile {
                    cpu_cores: cpu,
                    available_memory: memory,
                    total_memory: memory,
                    simd_width: 256,
                    has_gpu: gpu,
                    gpu_memory: None,
                    l1_cache_size: 32 * 1024,
                    l2_cache_size: 256 * 1024,
                    l3_cache_size: 8 * 1024 * 1024,
                },
            }
        }
    }

    impl FactsProvider for TestFacts {
        fn get_table_stats(&self, _table: &str) -> Option<&CoreTableStats> {
            None
        }
        fn get_column_stats(
            &self,
            _table: &str,
            _column: &str,
        ) -> Option<&ColumnStats> {
            None
        }
        fn hardware_profile(&self) -> &CoreHardwareProfile {
            &self.hw
        }
        fn get_schema(&self, _table: &str) -> Option<&TableInfo> {
            None
        }
        fn runtime_stats(
            &self,
            _id: &str,
        ) -> Option<&ra_core::facts::OperatorStats> {
            None
        }
        fn database_name(&self) -> &'static str {
            self.db_name
        }
        fn supports_feature(&self, _feature: &str) -> bool {
            true
        }
        fn sql_dialect(&self) -> SqlDialect {
            SqlDialect::Generic
        }
        fn memory_limit(&self) -> Option<u64> {
            None
        }
        fn optimizer_timeout(&self) -> Duration {
            Duration::from_secs(60)
        }
    }

    fn make_metadata(
        id: &str,
        databases: Vec<&str>,
        preconditions: Vec<Precondition>,
    ) -> RuleMetadata {
        RuleMetadata {
            id: id.to_string(),
            name: format!("Rule {id}"),
            category: "logical".to_string(),
            databases: databases.into_iter().map(String::from).collect(),
            standard: None,
            version: "1.0.0".to_string(),
            authors: vec![],
            tags: vec![],
            preconditions,
            complexity: None,
            benefit_range: None,
        }
    }

    // ---- parse_comparison tests ----

    #[test]
    fn parse_comparison_gte() {
        assert_eq!(
            parse_comparison("cpu_cores() >= 4"),
            Some((
                "cpu_cores()".to_string(),
                ">=".to_string(),
                "4".to_string(),
            ))
        );
    }

    #[test]
    fn parse_comparison_gt() {
        assert_eq!(
            parse_comparison("available_memory() > 8000000000"),
            Some((
                "available_memory()".to_string(),
                ">".to_string(),
                "8000000000".to_string(),
            ))
        );
    }

    #[test]
    fn parse_comparison_lte() {
        assert_eq!(
            parse_comparison("cpu_cores() <= 16"),
            Some((
                "cpu_cores()".to_string(),
                "<=".to_string(),
                "16".to_string(),
            ))
        );
    }

    #[test]
    fn parse_comparison_eq() {
        assert_eq!(
            parse_comparison("cpu_cores() == 8"),
            Some((
                "cpu_cores()".to_string(),
                "==".to_string(),
                "8".to_string(),
            ))
        );
    }

    #[test]
    fn parse_comparison_neq() {
        assert_eq!(
            parse_comparison("cpu_cores() != 0"),
            Some((
                "cpu_cores()".to_string(),
                "!=".to_string(),
                "0".to_string(),
            ))
        );
    }

    #[test]
    fn parse_comparison_lt() {
        assert_eq!(
            parse_comparison("available_memory() < 4000000000"),
            Some((
                "available_memory()".to_string(),
                "<".to_string(),
                "4000000000".to_string(),
            ))
        );
    }

    #[test]
    fn parse_comparison_no_operator_returns_none() {
        assert_eq!(parse_comparison("has_gpu()"), None);
    }

    // ---- evaluate_comparison tests ----

    #[test]
    fn evaluate_comparison_cpu_gte() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("cpu_cores()", ">=", "4", &facts));
        assert!(!evaluate_comparison("cpu_cores()", ">", "4", &facts));
    }

    #[test]
    fn evaluate_comparison_memory_gte() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison(
            "available_memory()",
            ">=",
            "8000000000",
            &facts,
        ));
    }

    #[test]
    fn evaluate_comparison_cpu_eq() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("cpu_cores()", "==", "4", &facts));
        assert!(!evaluate_comparison("cpu_cores()", "==", "5", &facts));
    }

    #[test]
    fn evaluate_comparison_cpu_neq() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("cpu_cores()", "!=", "5", &facts));
        assert!(!evaluate_comparison("cpu_cores()", "!=", "4", &facts));
    }

    #[test]
    fn evaluate_comparison_cpu_lt() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("cpu_cores()", "<", "5", &facts));
        assert!(!evaluate_comparison("cpu_cores()", "<", "4", &facts));
    }

    #[test]
    fn evaluate_comparison_cpu_lte() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("cpu_cores()", "<=", "4", &facts));
        assert!(evaluate_comparison("cpu_cores()", "<=", "5", &facts));
        assert!(!evaluate_comparison("cpu_cores()", "<=", "3", &facts));
    }

    #[test]
    fn evaluate_comparison_unknown_function_returns_true() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("disk_speed()", ">", "100", &facts));
    }

    #[test]
    fn evaluate_comparison_invalid_value_returns_true() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("cpu_cores()", ">", "abc", &facts));
    }

    #[test]
    fn evaluate_comparison_unknown_operator_returns_true() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_comparison("cpu_cores()", "~=", "4", &facts));
    }

    // ---- evaluate_predicate_condition tests ----

    #[test]
    fn predicate_has_gpu_true() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, true);
        assert!(evaluate_predicate_condition("has_gpu()", &facts));
    }

    #[test]
    fn predicate_has_gpu_false() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(!evaluate_predicate_condition("has_gpu()", &facts));
    }

    #[test]
    fn predicate_has_fpga_always_false() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, true);
        assert!(!evaluate_predicate_condition("has_fpga()", &facts));
    }

    #[test]
    fn predicate_comparison_expression() {
        let facts = TestFacts::new("test", 8, 16_000_000_000, false);
        assert!(evaluate_predicate_condition(
            "cpu_cores() >= 4",
            &facts,
        ));
    }

    #[test]
    fn predicate_unknown_returns_true() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_predicate_condition(
            "some_unknown_check()",
            &facts,
        ));
    }

    // ---- evaluate_hardware_requirement tests ----

    #[test]
    fn hardware_gpu_required_present() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, true);
        assert!(evaluate_hardware_requirement("GPU acceleration", &facts));
    }

    #[test]
    fn hardware_gpu_required_absent() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(!evaluate_hardware_requirement("GPU acceleration", &facts));
    }

    #[test]
    fn hardware_fpga_always_false() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, true);
        assert!(!evaluate_hardware_requirement("FPGA required", &facts));
    }

    #[test]
    fn hardware_cpu_always_true() {
        let facts = TestFacts::new("test", 1, 1_000_000, false);
        assert!(evaluate_hardware_requirement("CPU only", &facts));
    }

    #[test]
    fn hardware_unknown_returns_true() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_hardware_requirement("quantum coprocessor", &facts));
    }

    // ---- evaluate_feature_flag tests ----

    #[test]
    fn feature_flag_always_true() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(evaluate_feature_flag("any_flag", &facts));
    }

    // ---- evaluate_precondition tests ----

    #[test]
    fn precondition_pattern_always_passes() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        let pc = Precondition::Pattern {
            must_match: "some pattern".to_string(),
            description: "desc".to_string(),
        };
        assert!(evaluate_precondition(&pc, &facts));
    }

    #[test]
    fn precondition_database_match() {
        let facts = TestFacts::new("PostgreSQL", 4, 8_000_000_000, false);
        let pc = Precondition::Database {
            system: "postgresql".to_string(),
            description: "desc".to_string(),
        };
        assert!(evaluate_precondition(&pc, &facts));
    }

    #[test]
    fn precondition_database_no_match() {
        let facts = TestFacts::new("MySQL", 4, 8_000_000_000, false);
        let pc = Precondition::Database {
            system: "postgresql".to_string(),
            description: "desc".to_string(),
        };
        assert!(!evaluate_precondition(&pc, &facts));
    }

    #[test]
    fn precondition_hardware_gpu() {
        let facts_gpu = TestFacts::new("test", 4, 8_000_000_000, true);
        let facts_no_gpu = TestFacts::new("test", 4, 8_000_000_000, false);
        let pc = Precondition::Hardware {
            requirement: "GPU required".to_string(),
            description: "desc".to_string(),
        };
        assert!(evaluate_precondition(&pc, &facts_gpu));
        assert!(!evaluate_precondition(&pc, &facts_no_gpu));
    }

    #[test]
    fn precondition_predicate_comparison() {
        let facts = TestFacts::new("test", 8, 16_000_000_000, false);
        let pc = Precondition::Predicate {
            condition: "cpu_cores() >= 4".to_string(),
            description: "desc".to_string(),
        };
        assert!(evaluate_precondition(&pc, &facts));
    }

    #[test]
    fn precondition_feature_flag() {
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        let pc = Precondition::Feature {
            flag: "vectorized_exec".to_string(),
            description: "desc".to_string(),
        };
        assert!(evaluate_precondition(&pc, &facts));
    }

    // ---- is_rule_applicable tests ----

    #[test]
    fn rule_applicable_matching_database() {
        let metadata = make_metadata("r1", vec!["postgresql"], vec![]);
        let facts = TestFacts::new("PostgreSQL", 4, 8_000_000_000, false);
        assert!(is_rule_applicable(&metadata, &facts));
    }

    #[test]
    fn rule_not_applicable_wrong_database() {
        let metadata = make_metadata("r1", vec!["postgresql"], vec![]);
        let facts = TestFacts::new("MySQL", 4, 8_000_000_000, false);
        assert!(!is_rule_applicable(&metadata, &facts));
    }

    #[test]
    fn rule_applicable_all_databases() {
        let metadata = make_metadata("r1", vec!["all"], vec![]);
        let facts = TestFacts::new("MySQL", 4, 8_000_000_000, false);
        assert!(is_rule_applicable(&metadata, &facts));
    }

    #[test]
    fn rule_applicable_empty_databases_list() {
        let metadata = make_metadata("r1", vec![], vec![]);
        let facts = TestFacts::new("MySQL", 4, 8_000_000_000, false);
        assert!(is_rule_applicable(&metadata, &facts));
    }

    #[test]
    fn rule_not_applicable_precondition_fails() {
        let metadata = make_metadata(
            "r1",
            vec![],
            vec![Precondition::Hardware {
                requirement: "GPU required".to_string(),
                description: "needs GPU".to_string(),
            }],
        );
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(!is_rule_applicable(&metadata, &facts));
    }

    #[test]
    fn rule_applicable_all_preconditions_pass() {
        let metadata = make_metadata(
            "r1",
            vec![],
            vec![
                Precondition::Hardware {
                    requirement: "CPU only".to_string(),
                    description: "needs CPU".to_string(),
                },
                Precondition::Predicate {
                    condition: "cpu_cores() >= 2".to_string(),
                    description: "needs 2+ cores".to_string(),
                },
            ],
        );
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        assert!(is_rule_applicable(&metadata, &facts));
    }

    // ---- filter_rules_by_preconditions tests ----

    #[test]
    fn filter_rules_selects_applicable() {
        let rules = vec![
            ParsedRule {
                metadata: make_metadata("r1", vec!["postgresql"], vec![]),
                content: "rule 1".to_string(),
            },
            ParsedRule {
                metadata: make_metadata("r2", vec!["mysql"], vec![]),
                content: "rule 2".to_string(),
            },
            ParsedRule {
                metadata: make_metadata("r3", vec![], vec![]),
                content: "rule 3".to_string(),
            },
        ];

        let facts = TestFacts::new("PostgreSQL", 4, 8_000_000_000, false);
        let result = filter_rules_by_preconditions(&rules, &facts);
        assert_eq!(result, vec!["r1", "r3"]);
    }

    #[test]
    fn filter_rules_empty_input() {
        let rules: Vec<ParsedRule> = vec![];
        let facts = TestFacts::new("test", 4, 8_000_000_000, false);
        let result = filter_rules_by_preconditions(&rules, &facts);
        assert!(result.is_empty());
    }

    // ---- build_metadata_index tests ----

    #[test]
    fn build_metadata_index_maps_by_id() {
        let rules = vec![
            ParsedRule {
                metadata: make_metadata("r1", vec![], vec![]),
                content: String::new(),
            },
            ParsedRule {
                metadata: make_metadata("r2", vec![], vec![]),
                content: String::new(),
            },
        ];
        let index = build_metadata_index(&rules);
        assert_eq!(index.len(), 2);
        assert!(index.contains_key("r1"));
        assert!(index.contains_key("r2"));
    }

    // ---- parse_rra_file tests ----

    fn write_temp_rra(
        dir: &std::path::Path,
        name: &str,
        content: &str,
    ) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parse_rra_file_valid() {
        let dir = std::env::temp_dir().join(
            format!("ra_test_rra_{}", std::process::id()),
        );
        fs::create_dir_all(&dir).unwrap();

        let rra = write_temp_rra(
            &dir,
            "test_rule.rra",
            "---\n\
             id: push-filter\n\
             name: Push Filter Down\n\
             category: logical/predicate-pushdown\n\
             databases:\n\
               - postgresql\n\
             ---\n\
             # Push filter below join\n\
             This rule pushes filters.\n",
        );

        let result = parse_rra_file(&rra);
        assert!(result.is_ok(), "{:?}", result.err());
        let parsed = result.unwrap();
        assert_eq!(parsed.metadata.id, "push-filter");
        assert_eq!(parsed.metadata.name, "Push Filter Down");
        assert_eq!(
            parsed.metadata.category,
            "logical/predicate-pushdown",
        );
        assert_eq!(
            parsed.metadata.databases,
            vec!["postgresql"],
        );
        assert!(parsed.content.contains("Push filter"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_rra_file_missing_frontmatter() {
        let dir = std::env::temp_dir().join(
            format!("ra_test_nofm_{}", std::process::id()),
        );
        fs::create_dir_all(&dir).unwrap();

        let rra = write_temp_rra(
            &dir,
            "bad.rra",
            "No frontmatter here, just content.",
        );

        let result = parse_rra_file(&rra);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Invalid .rra file format"),
            "got: {msg}",
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_rra_file_invalid_yaml() {
        let dir = std::env::temp_dir().join(
            format!("ra_test_badyml_{}", std::process::id()),
        );
        fs::create_dir_all(&dir).unwrap();

        let rra = write_temp_rra(
            &dir,
            "bad_yaml.rra",
            "---\n\
             [invalid yaml content\n\
             ---\n\
             body\n",
        );

        let result = parse_rra_file(&rra);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Failed to parse YAML"),
            "got: {msg}",
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_rra_file_nonexistent() {
        let path =
            std::path::Path::new("/tmp/nonexistent_rule.rra");
        let result = parse_rra_file(path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Failed to read rule file"),
            "got: {msg}",
        );
    }

    // ---- load_rules_from_directory tests ----

    #[test]
    fn load_rules_nonexistent_dir_returns_empty() {
        let dir =
            std::path::Path::new("/tmp/nonexistent_rules_dir");
        let result = load_rules_from_directory(dir);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn load_rules_from_dir_with_valid_rra() {
        let dir = std::env::temp_dir().join(
            format!("ra_test_loaddir_{}", std::process::id()),
        );
        fs::create_dir_all(&dir).unwrap();

        write_temp_rra(
            &dir,
            "rule1.rra",
            "---\n\
             id: rule-one\n\
             name: Rule One\n\
             category: logical\n\
             ---\n\
             Content one.\n",
        );

        write_temp_rra(
            &dir,
            "rule2.rra",
            "---\n\
             id: rule-two\n\
             name: Rule Two\n\
             category: physical\n\
             ---\n\
             Content two.\n",
        );

        // Non-rra file should be ignored
        write_temp_rra(
            &dir,
            "notes.txt",
            "Not a rule file.",
        );

        let result = load_rules_from_directory(&dir);
        assert!(result.is_ok(), "{:?}", result.err());
        let rules = result.unwrap();
        assert_eq!(rules.len(), 2);

        let ids: Vec<&str> =
            rules.iter().map(|r| r.metadata.id.as_str()).collect();
        assert!(ids.contains(&"rule-one"));
        assert!(ids.contains(&"rule-two"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_rules_skips_malformed_rra() {
        let dir = std::env::temp_dir().join(
            format!("ra_test_malformed_{}", std::process::id()),
        );
        fs::create_dir_all(&dir).unwrap();

        write_temp_rra(
            &dir,
            "good.rra",
            "---\n\
             id: good-rule\n\
             name: Good Rule\n\
             category: logical\n\
             ---\n\
             Good content.\n",
        );

        write_temp_rra(
            &dir,
            "bad.rra",
            "no frontmatter at all",
        );

        let result = load_rules_from_directory(&dir);
        assert!(result.is_ok());
        let rules = result.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].metadata.id, "good-rule");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_rules_recursive_subdirectory() {
        let dir = std::env::temp_dir().join(
            format!("ra_test_recursive_{}", std::process::id()),
        );
        let subdir = dir.join("subdir");
        fs::create_dir_all(&subdir).unwrap();

        write_temp_rra(
            &dir,
            "top.rra",
            "---\n\
             id: top-rule\n\
             name: Top Rule\n\
             category: logical\n\
             ---\n\
             Top.\n",
        );

        write_temp_rra(
            &subdir,
            "nested.rra",
            "---\n\
             id: nested-rule\n\
             name: Nested Rule\n\
             category: physical\n\
             ---\n\
             Nested.\n",
        );

        let result = load_rules_from_directory(&dir);
        assert!(result.is_ok());
        let rules = result.unwrap();
        assert_eq!(rules.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    // ---- parse_rra_file with preconditions ----

    #[test]
    fn parse_rra_file_with_preconditions() {
        let dir = std::env::temp_dir().join(
            format!("ra_test_precond_{}", std::process::id()),
        );
        fs::create_dir_all(&dir).unwrap();

        let rra = write_temp_rra(
            &dir,
            "precond.rra",
            "\
---
id: gpu-rule
name: GPU Rule
category: physical
preconditions:
  - type: hardware
    requirement: GPU acceleration
    description: Needs GPU
  - type: database
    system: postgresql
    description: PG only
---
GPU optimization rule.
",
        );

        let parsed = parse_rra_file(&rra).unwrap();
        assert_eq!(parsed.metadata.preconditions.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }
}
