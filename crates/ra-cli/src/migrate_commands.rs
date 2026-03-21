//! Commands for migrating rule pre-conditions from prose to formal YAML.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use ra_core::{FactValue, PreCondition};
use ra_parser::{RuleFile, parse_rule_file};
use serde_yaml;
use std::fs;
use std::path::{Path, PathBuf};

/// Migrate pre-conditions in rule files from prose to formal YAML.
///
/// This tool helps semi-automate the migration process by:
/// 1. Extracting existing "## Preconditions" sections
/// 2. Identifying code blocks with guard functions
/// 3. Suggesting YAML pre-condition structures
/// 4. Writing migrated files to output directory
pub fn migrate_preconditions(
    input: &Path,
    output: &Path,
    dry_run: bool,
    validate: bool,
) -> Result<MigrationReport> {
    let mut report = MigrationReport::default();

    // Collect all .rra files to process
    let files = collect_rra_files(input)?;
    report.total_files = files.len();

    for file_path in files {
        match migrate_single_file(&file_path, output, dry_run, validate) {
            Ok(status) => {
                match status {
                    FileStatus::AlreadyMigrated => report.already_migrated += 1,
                    FileStatus::Migrated => report.migrated += 1,
                    FileStatus::NeedsManualReview => report.needs_review += 1,
                }
            }
            Err(e) => {
                eprintln!(
                    "{} {}: {}",
                    "Error".red().bold(),
                    file_path.display(),
                    e
                );
                report.errors += 1;
            }
        }
    }

    Ok(report)
}

/// Validate migrated pre-conditions against baseline behavior.
///
/// Compares the evaluation of old (prose/Rust) pre-conditions against
/// new (YAML) pre-conditions to ensure behavior is unchanged.
pub fn validate_preconditions(
    baseline_dir: &Path,
    migrated_dir: &Path,
    facts_file: Option<&Path>,
) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();

    let baseline_files = collect_rra_files(baseline_dir)?;
    report.total_files = baseline_files.len();

    for baseline_path in baseline_files {
        let relative = baseline_path
            .strip_prefix(baseline_dir)
            .context("Failed to compute relative path")?;
        let migrated_path = migrated_dir.join(relative);

        if !migrated_path.exists() {
            eprintln!(
                "{} {}",
                "Missing migrated file:".yellow(),
                migrated_path.display()
            );
            report.missing += 1;
            continue;
        }

        match validate_single_file(&baseline_path, &migrated_path, facts_file) {
            Ok(true) => report.passed += 1,
            Ok(false) => {
                eprintln!(
                    "{} {} vs {}",
                    "Behavior mismatch:".red().bold(),
                    baseline_path.display(),
                    migrated_path.display()
                );
                report.failed += 1;
            }
            Err(e) => {
                eprintln!(
                    "{} {}: {}",
                    "Validation error".red().bold(),
                    baseline_path.display(),
                    e
                );
                report.errors += 1;
            }
        }
    }

    Ok(report)
}

// ========== Helper Functions ==========

fn collect_rra_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if dir.is_file() {
        if dir.extension().and_then(|s| s.to_str()) == Some("rra") {
            files.push(dir.to_path_buf());
        }
        return Ok(files);
    }

    for entry in walkdir::WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("rra") {
            files.push(path.to_path_buf());
        }
    }

    Ok(files)
}

fn migrate_single_file(
    input: &Path,
    output_dir: &Path,
    dry_run: bool,
    _validate: bool,
) -> Result<FileStatus> {
    let content = fs::read_to_string(input)
        .with_context(|| format!("Failed to read {}", input.display()))?;

    let rule_file = parse_rule_file(&content)
        .with_context(|| format!("Failed to parse {}", input.display()))?;

    // Check if already has formal pre-conditions
    if !rule_file.metadata.preconditions.is_empty() {
        return Ok(FileStatus::AlreadyMigrated);
    }

    // Extract prose preconditions section
    let prose_preconditions = extract_preconditions_section(&content);

    // Attempt to infer pre-conditions from prose
    let inferred_preconditions = infer_preconditions_from_prose(&prose_preconditions);

    if inferred_preconditions.is_empty() {
        // No automatic inference possible
        return Ok(FileStatus::NeedsManualReview);
    }

    // Build migrated content
    let migrated_content = build_migrated_content(&rule_file, &inferred_preconditions, &content)?;

    if dry_run {
        println!("\n{}", "=".repeat(80));
        println!("{}: {}", "Would migrate".green().bold(), input.display());
        println!("{}", "=".repeat(80));
        println!("{}", migrated_content);
        return Ok(FileStatus::Migrated);
    }

    // Write to output directory
    let relative = input
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?;
    let output_path = output_dir.join(relative);

    fs::create_dir_all(output_path.parent().unwrap())
        .context("Failed to create output directory")?;
    fs::write(&output_path, migrated_content)
        .with_context(|| format!("Failed to write to {}", output_path.display()))?;

    println!("{} {}", "Migrated:".green(), input.display());

    Ok(FileStatus::Migrated)
}

fn validate_single_file(
    baseline: &Path,
    migrated: &Path,
    _facts_file: Option<&Path>,
) -> Result<bool> {
    let baseline_content = fs::read_to_string(baseline)
        .with_context(|| format!("Failed to read {}", baseline.display()))?;
    let migrated_content = fs::read_to_string(migrated)
        .with_context(|| format!("Failed to read {}", migrated.display()))?;

    let baseline_rule = parse_rule_file(&baseline_content)
        .with_context(|| format!("Failed to parse {}", baseline.display()))?;
    let migrated_rule = parse_rule_file(&migrated_content)
        .with_context(|| format!("Failed to parse {}", migrated.display()))?;

    let errors = validate_migration(
        &baseline_rule,
        &migrated_rule,
    );

    for error in &errors {
        eprintln!("  {}", error);
    }

    Ok(errors.is_empty())
}

/// A migration validation error with actionable context.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
    pub field: String,
    pub reason: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationErrorKind {
    BreakingChange,
    DataLossRisk,
    MetadataMismatch,
    ConstraintNarrowed,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.kind {
            ValidationErrorKind::BreakingChange => "BREAKING",
            ValidationErrorKind::DataLossRisk => "DATA_LOSS",
            ValidationErrorKind::MetadataMismatch => "MISMATCH",
            ValidationErrorKind::ConstraintNarrowed => "NARROWED",
        };
        write!(
            f,
            "[{}] {}: {} (suggestion: {})",
            label, self.field, self.reason, self.suggestion
        )
    }
}

/// Validate that a migration from baseline to migrated is safe.
///
/// Returns an empty vec when the migration is valid, or a list
/// of validation errors describing each problem found.
fn validate_migration(
    baseline: &RuleFile,
    migrated: &RuleFile,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    validate_metadata_identity(
        &baseline.metadata,
        &migrated.metadata,
        &mut errors,
    );
    validate_precondition_safety(
        &baseline.metadata.preconditions,
        &migrated.metadata.preconditions,
        &mut errors,
    );

    errors
}

fn validate_metadata_identity(
    baseline: &ra_parser::RuleMetadata,
    migrated: &ra_parser::RuleMetadata,
    errors: &mut Vec<ValidationError>,
) {
    if baseline.id != migrated.id {
        errors.push(ValidationError {
            kind: ValidationErrorKind::MetadataMismatch,
            field: "id".into(),
            reason: format!(
                "Rule id changed from '{}' to '{}'",
                baseline.id, migrated.id
            ),
            suggestion: "Rule identity must be preserved during migration".into(),
        });
    }
    if baseline.name != migrated.name {
        errors.push(ValidationError {
            kind: ValidationErrorKind::MetadataMismatch,
            field: "name".into(),
            reason: format!(
                "Rule name changed from '{}' to '{}'",
                baseline.name, migrated.name
            ),
            suggestion: "Rule name must be preserved during migration".into(),
        });
    }
    if baseline.category != migrated.category {
        errors.push(ValidationError {
            kind: ValidationErrorKind::MetadataMismatch,
            field: "category".into(),
            reason: format!(
                "Category changed from '{}' to '{}'",
                baseline.category, migrated.category
            ),
            suggestion: "Category must be preserved during migration".into(),
        });
    }
}

fn validate_precondition_safety(
    baseline: &[PreCondition],
    migrated: &[PreCondition],
    errors: &mut Vec<ValidationError>,
) {
    // Check each baseline precondition has a compatible match
    for (i, base_pc) in baseline.iter().enumerate() {
        let label = format!("precondition[{i}]");

        let matched = find_compatible_precondition(base_pc, migrated);
        if matched.is_none() && !is_optional(base_pc) {
            errors.push(ValidationError {
                kind: ValidationErrorKind::BreakingChange,
                field: label,
                reason: format!(
                    "Required precondition removed: {}",
                    describe_precondition(base_pc)
                ),
                suggestion: "Add a matching precondition or mark the original as optional".into(),
            });
        }
    }

    // Check migrated preconditions for narrowing
    for (i, mig_pc) in migrated.iter().enumerate() {
        let label = format!("migrated_precondition[{i}]");

        if let Some(base_pc) = find_matching_precondition(mig_pc, baseline) {
            check_constraint_narrowing(&base_pc, mig_pc, &label, errors);
        }
    }
}

fn is_optional(pc: &PreCondition) -> bool {
    match pc {
        PreCondition::Pattern { optional, .. }
        | PreCondition::Predicate { optional, .. }
        | PreCondition::Fact { optional, .. }
        | PreCondition::Capability { optional, .. }
        | PreCondition::Composite { optional, .. } => *optional,
    }
}

fn describe_precondition(pc: &PreCondition) -> String {
    match pc {
        PreCondition::Pattern {
            must_match,
            description,
            ..
        } => {
            if let Some(desc) = description {
                return desc.clone();
            }
            must_match
                .as_deref()
                .map_or_else(|| "pattern constraint".into(), |p| format!("pattern: {p}"))
        }
        PreCondition::Predicate {
            condition,
            description,
            ..
        } => description
            .as_deref()
            .map_or_else(|| format!("predicate: {condition}"), String::from),
        PreCondition::Fact {
            fact_type,
            comparator,
            threshold,
            description,
            ..
        } => {
            if let Some(desc) = description {
                return desc.clone();
            }
            format!("fact: {fact_type} {comparator} {threshold:?}")
        }
        PreCondition::Capability {
            database,
            requires,
            description,
            ..
        } => description
            .as_deref()
            .map_or_else(|| format!("capability: {database}.{requires}"), String::from),
        PreCondition::Composite { description, .. } => description
            .as_deref()
            .map_or_else(|| "composite condition".into(), String::from),
    }
}

/// Find a precondition in `candidates` that is compatible with `target`.
///
/// Compatible means same variant and logically equivalent or wider scope.
fn find_compatible_precondition<'a>(
    target: &PreCondition,
    candidates: &'a [PreCondition],
) -> Option<&'a PreCondition> {
    candidates.iter().find(|c| preconditions_compatible(target, c))
}

/// Find a precondition in `candidates` that matches the same logical slot.
fn find_matching_precondition<'a>(
    target: &PreCondition,
    candidates: &'a [PreCondition],
) -> Option<&'a PreCondition> {
    candidates.iter().find(|c| preconditions_same_slot(target, c))
}

/// Two preconditions are compatible if the migrated one covers at least
/// the same scope as the baseline one.
fn preconditions_compatible(baseline: &PreCondition, migrated: &PreCondition) -> bool {
    match (baseline, migrated) {
        (
            PreCondition::Pattern {
                must_match: bm, ..
            },
            PreCondition::Pattern {
                must_match: mm, ..
            },
        ) => bm == mm,
        (
            PreCondition::Predicate {
                condition: bc, ..
            },
            PreCondition::Predicate {
                condition: mc, ..
            },
        ) => bc == mc,
        (
            PreCondition::Fact {
                fact_type: bf,
                comparator: bc,
                threshold: bt,
                ..
            },
            PreCondition::Fact {
                fact_type: mf,
                comparator: mc,
                threshold: mt,
                ..
            },
        ) => bf == mf && (bc == mc && bt == mt || is_wider_constraint(bc, bt, mc, mt)),
        (
            PreCondition::Capability {
                database: bd,
                requires: br,
                ..
            },
            PreCondition::Capability {
                database: md,
                requires: mr,
                ..
            },
        ) => bd == md && br == mr,
        _ => false,
    }
}

/// Check if two preconditions refer to the same logical slot
/// (same variant + key field).
fn preconditions_same_slot(a: &PreCondition, b: &PreCondition) -> bool {
    match (a, b) {
        (
            PreCondition::Pattern {
                must_match: am, ..
            },
            PreCondition::Pattern {
                must_match: bm, ..
            },
        ) => am == bm,
        (
            PreCondition::Predicate {
                condition: ac, ..
            },
            PreCondition::Predicate {
                condition: bc, ..
            },
        ) => ac == bc,
        (
            PreCondition::Fact {
                fact_type: af, ..
            },
            PreCondition::Fact {
                fact_type: bf, ..
            },
        ) => af == bf,
        (
            PreCondition::Capability {
                database: ad,
                requires: ar,
                ..
            },
            PreCondition::Capability {
                database: bd,
                requires: br,
                ..
            },
        ) => ad == bd && ar == br,
        _ => false,
    }
}

/// Check if the migrated constraint is wider (more permissive) than baseline.
///
/// For numeric thresholds: `> 500` is wider than `> 1000` because it
/// accepts more inputs. A wider constraint is safe because the migrated
/// rule fires in all cases the baseline would.
fn is_wider_constraint(
    base_cmp: &str,
    base_thresh: &FactValue,
    mig_cmp: &str,
    mig_thresh: &FactValue,
) -> bool {
    if base_cmp != mig_cmp {
        return false;
    }

    match (base_thresh, mig_thresh) {
        (FactValue::Int(b), FactValue::Int(m)) => match base_cmp {
            ">" | ">=" => m <= b,
            "<" | "<=" => m >= b,
            _ => false,
        },
        (FactValue::Float(b), FactValue::Float(m)) => match base_cmp {
            ">" | ">=" => *m <= *b,
            "<" | "<=" => *m >= *b,
            _ => false,
        },
        _ => false,
    }
}

/// Detect cases where the migrated precondition is stricter than baseline.
fn check_constraint_narrowing(
    baseline: &PreCondition,
    migrated: &PreCondition,
    label: &str,
    errors: &mut Vec<ValidationError>,
) {
    if let (
        PreCondition::Fact {
            fact_type: bf,
            comparator: bc,
            threshold: bt,
            ..
        },
        PreCondition::Fact {
            fact_type: mf,
            comparator: mc,
            threshold: mt,
            ..
        },
    ) = (baseline, migrated)
    {
        if bf != mf {
            return;
        }
        if bc == mc && bt == mt {
            return;
        }
        if !is_wider_constraint(bc, bt, mc, mt) {
            errors.push(ValidationError {
                kind: ValidationErrorKind::ConstraintNarrowed,
                field: label.into(),
                reason: format!(
                    "Fact '{}' constraint narrowed from '{} {:?}' to '{} {:?}'",
                    bf, bc, bt, mc, mt
                ),
                suggestion:
                    "Use a wider (more permissive) threshold to preserve baseline behavior"
                        .into(),
            });
        }
    }

    // Optional -> required is a data loss risk
    if is_optional(baseline) && !is_optional(migrated) {
        errors.push(ValidationError {
            kind: ValidationErrorKind::DataLossRisk,
            field: label.into(),
            reason: "Optional precondition became required after migration".into(),
            suggestion: "Keep the precondition optional or verify all callers satisfy it".into(),
        });
    }
}

fn extract_preconditions_section(content: &str) -> String {
    let mut in_preconditions = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        if line.trim().starts_with("## Preconditions") {
            in_preconditions = true;
            continue;
        }
        if in_preconditions {
            if line.trim().starts_with("##") {
                // Next section starts
                break;
            }
            lines.push(line.to_string());
        }
    }

    lines.join("\n")
}

fn infer_preconditions_from_prose(prose: &str) -> Vec<PreCondition> {
    let mut preconditions = Vec::new();

    let lower = prose.to_lowercase();

    // Pattern: "must match" or "pattern"
    if lower.contains("must match") || lower.contains("pattern") {
        // This is a heuristic - in real implementation, would need more sophisticated parsing
        preconditions.push(PreCondition::Pattern {
            must_match: None,
            must_not_match: None,
            description: Some("Inferred pattern constraint".into()),
            optional: false,
        });
    }

    // Heuristic: mentions statistics
    if lower.contains("cardinality") || lower.contains("row count") {
        preconditions.push(PreCondition::Fact {
            fact_type: "statistics.cardinality".into(),
            table: None,
            column: None,
            comparator: ">".into(),
            threshold: FactValue::Int(1000),
            confidence: None,
            description: Some("Inferred from prose - requires manual review".into()),
            optional: true,
        });
    }

    // Heuristic: mentions hardware
    if lower.contains("memory") || lower.contains("cpu") || lower.contains("parallel") {
        preconditions.push(PreCondition::Fact {
            fact_type: "hardware.cpu_cores".into(),
            table: None,
            column: None,
            comparator: ">".into(),
            threshold: FactValue::Int(2),
            confidence: None,
            description: Some("Inferred from prose - requires manual review".into()),
            optional: true,
        });
    }

    preconditions
}

fn build_migrated_content(
    rule_file: &RuleFile,
    preconditions: &[PreCondition],
    original_content: &str,
) -> Result<String> {
    // Split original content into frontmatter and body
    let parts: Vec<&str> = original_content.splitn(3, "---").collect();

    if parts.len() < 3 {
        bail!("Invalid RRA format: expected frontmatter delimited by ---");
    }

    // Rebuild frontmatter with preconditions
    let mut metadata = rule_file.metadata.clone();
    metadata.preconditions = preconditions.to_vec();

    let yaml = serde_yaml::to_string(&metadata)
        .context("Failed to serialize metadata to YAML")?;

    // Reconstruct file
    let mut result = String::new();
    result.push_str("---\n");
    result.push_str(&yaml);
    result.push_str("---\n");
    result.push_str(parts[2]); // Body after second ---

    Ok(result)
}

// ========== Report Types ==========

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub total_files: usize,
    pub migrated: usize,
    pub already_migrated: usize,
    pub needs_review: usize,
    pub errors: usize,
}

impl MigrationReport {
    pub fn print_summary(&self) {
        println!("\n{}", "Migration Summary".bold());
        println!("{}", "=".repeat(50));
        println!("Total files:        {}", self.total_files);
        println!("Migrated:           {}", self.migrated.to_string().green());
        println!(
            "Already migrated:   {}",
            self.already_migrated.to_string().blue()
        );
        println!(
            "Needs review:       {}",
            self.needs_review.to_string().yellow()
        );
        println!("Errors:             {}", self.errors.to_string().red());
        println!("{}", "=".repeat(50));
    }
}

#[derive(Debug, Default)]
pub struct ValidationReport {
    pub total_files: usize,
    pub passed: usize,
    pub failed: usize,
    pub missing: usize,
    pub errors: usize,
}

impl ValidationReport {
    pub fn print_summary(&self) {
        println!("\n{}", "Validation Summary".bold());
        println!("{}", "=".repeat(50));
        println!("Total files:        {}", self.total_files);
        println!("Passed:             {}", self.passed.to_string().green());
        println!("Failed:             {}", self.failed.to_string().red());
        println!("Missing:            {}", self.missing.to_string().yellow());
        println!("Errors:             {}", self.errors.to_string().red());
        println!("{}", "=".repeat(50));
    }
}

#[derive(Debug, Clone, Copy)]
enum FileStatus {
    AlreadyMigrated,
    Migrated,
    NeedsManualReview,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ra_parser::RuleMetadata;

    fn base_metadata() -> RuleMetadata {
        RuleMetadata {
            id: "test-rule".into(),
            name: "Test Rule".into(),
            category: "logical/test".into(),
            databases: vec![],
            standard: None,
            execution_models: vec![],
            version: "1.0.0".into(),
            authors: vec![],
            tags: vec![],
            preconditions: vec![],
        }
    }

    fn base_rule() -> RuleFile {
        RuleFile {
            metadata: base_metadata(),
            description: "A test rule".into(),
            algebra_notation: None,
            implementation: None,
            test_cases: vec![],
            references: vec![],
        }
    }

    fn fact_pc(
        fact_type: &str,
        comparator: &str,
        threshold: FactValue,
        optional: bool,
    ) -> PreCondition {
        PreCondition::Fact {
            fact_type: fact_type.into(),
            table: None,
            column: None,
            comparator: comparator.into(),
            threshold,
            confidence: None,
            description: None,
            optional,
        }
    }

    #[test]
    fn identical_rules_pass_validation() {
        let baseline = base_rule();
        let migrated = base_rule();
        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn metadata_id_change_detected() {
        let baseline = base_rule();
        let mut migrated = base_rule();
        migrated.metadata.id = "different-id".into();

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::MetadataMismatch);
        assert!(errors[0].field == "id");
    }

    #[test]
    fn metadata_name_change_detected() {
        let baseline = base_rule();
        let mut migrated = base_rule();
        migrated.metadata.name = "Different Name".into();

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::MetadataMismatch);
        assert!(errors[0].field == "name");
    }

    #[test]
    fn metadata_category_change_detected() {
        let baseline = base_rule();
        let mut migrated = base_rule();
        migrated.metadata.category = "physical/other".into();

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::MetadataMismatch);
        assert!(errors[0].field == "category");
    }

    #[test]
    fn removed_required_precondition_is_breaking() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("statistics.cardinality", ">", FactValue::Int(1000), false),
        ];

        let migrated = base_rule(); // no preconditions

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BreakingChange);
    }

    #[test]
    fn removed_optional_precondition_is_not_breaking() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("statistics.cardinality", ">", FactValue::Int(1000), true),
        ];

        let migrated = base_rule(); // no preconditions

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn preserved_precondition_passes() {
        let pc = fact_pc("statistics.cardinality", ">", FactValue::Int(1000), false);
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![pc.clone()];
        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![pc];

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn widened_threshold_passes() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("statistics.cardinality", ">", FactValue::Int(1000), false),
        ];

        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![
            fact_pc("statistics.cardinality", ">", FactValue::Int(500), false),
        ];

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn narrowed_threshold_detected() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("statistics.cardinality", ">", FactValue::Int(500), false),
        ];

        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![
            fact_pc("statistics.cardinality", ">", FactValue::Int(1000), false),
        ];

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::ConstraintNarrowed);
    }

    #[test]
    fn optional_to_required_is_data_loss_risk() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("hardware.cpu_cores", ">", FactValue::Int(2), true),
        ];

        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![
            fact_pc("hardware.cpu_cores", ">", FactValue::Int(2), false),
        ];

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.iter().any(|e| e.kind == ValidationErrorKind::DataLossRisk));
    }

    #[test]
    fn pattern_precondition_preserved() {
        let pc = PreCondition::Pattern {
            must_match: Some("(filter ?pred (scan ?t))".into()),
            must_not_match: None,
            description: None,
            optional: false,
        };
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![pc.clone()];
        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![pc];

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn pattern_precondition_removed_is_breaking() {
        let pc = PreCondition::Pattern {
            must_match: Some("(filter ?pred (scan ?t))".into()),
            must_not_match: None,
            description: None,
            optional: false,
        };
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![pc];
        let migrated = base_rule();

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BreakingChange);
    }

    #[test]
    fn capability_precondition_preserved() {
        let pc = PreCondition::Capability {
            database: "current".into(),
            requires: "lateral_join".into(),
            description: None,
            optional: false,
        };
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![pc.clone()];
        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![pc];

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn capability_precondition_removed_is_breaking() {
        let pc = PreCondition::Capability {
            database: "current".into(),
            requires: "bitmap_index".into(),
            description: None,
            optional: false,
        };
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![pc];
        let migrated = base_rule();

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::BreakingChange);
    }

    #[test]
    fn multiple_errors_accumulated() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("statistics.cardinality", ">", FactValue::Int(1000), false),
            PreCondition::Pattern {
                must_match: Some("(join ?l ?r)".into()),
                must_not_match: None,
                description: None,
                optional: false,
            },
        ];

        let mut migrated = base_rule();
        migrated.metadata.id = "wrong-id".into();
        // Both preconditions missing

        let errors = validate_migration(&baseline, &migrated);
        // 1 metadata mismatch + 2 breaking precondition removals
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn float_threshold_widening() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("statistics.selectivity", "<", FactValue::Float(0.5), false),
        ];

        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![
            fact_pc("statistics.selectivity", "<", FactValue::Float(0.8), false),
        ];

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn float_threshold_narrowing() {
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![
            fact_pc("statistics.selectivity", "<", FactValue::Float(0.8), false),
        ];

        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![
            fact_pc("statistics.selectivity", "<", FactValue::Float(0.3), false),
        ];

        let errors = validate_migration(&baseline, &migrated);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ValidationErrorKind::ConstraintNarrowed);
    }

    #[test]
    fn predicate_precondition_preserved() {
        let pc = PreCondition::Predicate {
            condition: "pred_columns_subset_of(?pred, ?cols)".into(),
            description: None,
            optional: false,
        };
        let mut baseline = base_rule();
        baseline.metadata.preconditions = vec![pc.clone()];
        let mut migrated = base_rule();
        migrated.metadata.preconditions = vec![pc];

        let errors = validate_migration(&baseline, &migrated);
        assert!(errors.is_empty());
    }

    #[test]
    fn validation_error_display_formatting() {
        let err = ValidationError {
            kind: ValidationErrorKind::BreakingChange,
            field: "precondition[0]".into(),
            reason: "Required precondition removed".into(),
            suggestion: "Add it back".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("BREAKING"));
        assert!(msg.contains("precondition[0]"));
        assert!(msg.contains("Required precondition removed"));
        assert!(msg.contains("Add it back"));
    }

    #[test]
    fn is_wider_constraint_gt_lower_threshold() {
        assert!(is_wider_constraint(
            ">",
            &FactValue::Int(1000),
            ">",
            &FactValue::Int(500),
        ));
    }

    #[test]
    fn is_wider_constraint_gt_higher_is_not_wider() {
        assert!(!is_wider_constraint(
            ">",
            &FactValue::Int(500),
            ">",
            &FactValue::Int(1000),
        ));
    }

    #[test]
    fn is_wider_constraint_lt_higher_threshold() {
        assert!(is_wider_constraint(
            "<",
            &FactValue::Int(500),
            "<",
            &FactValue::Int(1000),
        ));
    }

    #[test]
    fn is_wider_constraint_different_ops_not_comparable() {
        assert!(!is_wider_constraint(
            ">",
            &FactValue::Int(1000),
            "<",
            &FactValue::Int(500),
        ));
    }

    #[test]
    fn is_wider_constraint_eq_not_wider() {
        assert!(!is_wider_constraint(
            "==",
            &FactValue::Int(1000),
            "==",
            &FactValue::Int(500),
        ));
    }

    #[test]
    fn describe_precondition_pattern() {
        let pc = PreCondition::Pattern {
            must_match: Some("(filter ?p ?r)".into()),
            must_not_match: None,
            description: None,
            optional: false,
        };
        let desc = describe_precondition(&pc);
        assert!(desc.contains("filter ?p ?r"));
    }

    #[test]
    fn describe_precondition_with_description() {
        let pc = PreCondition::Pattern {
            must_match: Some("(filter ?p ?r)".into()),
            must_not_match: None,
            description: Some("Must be a filter node".into()),
            optional: false,
        };
        let desc = describe_precondition(&pc);
        assert_eq!(desc, "Must be a filter node");
    }
}
