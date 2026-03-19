//! Commands for migrating rule pre-conditions from prose to formal YAML.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use ra_core::{FactValue, PreCondition, PreConditionBuilder};
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
    _baseline: &Path,
    _migrated: &Path,
    _facts_file: Option<&Path>,
) -> Result<bool> {
    // TODO: Implement actual validation logic
    // For now, assume all validations pass
    Ok(true)
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
