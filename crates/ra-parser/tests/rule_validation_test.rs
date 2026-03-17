//! Validates all real .rra rule files in the rules/ directory.
//!
//! Ensures every committed rule file passes parsing and metadata
//! validation, has no duplicate IDs, and uses valid category paths.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod helpers;

use std::collections::HashMap;

use ra_parser::{
    parse_rule_file, validate_metadata_all,
    validator::KNOWN_CATEGORY_PREFIXES,
};

// ── Scan and parse all rules ────────────────────────────────

#[test]
fn all_committed_rules_parse_successfully() {
    let rules_dir = helpers::rules_dir();
    let files = helpers::collect_rra_files(&rules_dir);

    let mut failures = Vec::new();

    for file in &files {
        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| {
                panic!(
                    "cannot read {}: {e}",
                    file.display()
                )
            });

        if let Err(e) = parse_rule_file(&source) {
            failures.push(format!(
                "{}: {e}",
                file.display()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "The following rule files failed to parse:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn all_committed_rules_pass_full_validation() {
    let rules_dir = helpers::rules_dir();
    let files = helpers::collect_rra_files(&rules_dir);
    let mut failures = Vec::new();

    for file in &files {
        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| {
                panic!(
                    "cannot read {}: {e}",
                    file.display()
                )
            });

        match parse_rule_file(&source) {
            Ok(rule) => {
                let errors =
                    validate_metadata_all(&rule.metadata);
                if !errors.is_empty() {
                    let msgs: Vec<String> = errors
                        .iter()
                        .map(ToString::to_string)
                        .collect();
                    failures.push(format!(
                        "{}: {}",
                        file.display(),
                        msgs.join("; ")
                    ));
                }
            }
            Err(e) => {
                failures.push(format!(
                    "{}: parse error: {e}",
                    file.display()
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "The following rules failed validation:\n  {}",
        failures.join("\n  ")
    );
}

// ── Duplicate ID detection ──────────────────────────────────

#[test]
fn no_duplicate_rule_ids_across_rules_directory() {
    let rules_dir = helpers::rules_dir();
    let files = helpers::collect_rra_files(&rules_dir);
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut duplicates = Vec::new();

    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rule = match parse_rule_file(&source) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let id = rule.metadata.id.clone();
        let path = file.display().to_string();

        if let Some(first_path) = seen.get(&id) {
            duplicates.push(format!(
                "id '{}' appears in:\n    {}\n    {}",
                id, first_path, path
            ));
        } else {
            seen.insert(id, path);
        }
    }

    assert!(
        duplicates.is_empty(),
        "Duplicate rule IDs found:\n  {}",
        duplicates.join("\n  ")
    );
}

// ── Category path validation ────────────────────────────────

#[test]
fn all_rule_categories_use_known_prefixes() {
    let rules_dir = helpers::rules_dir();
    let files = helpers::collect_rra_files(&rules_dir);
    let mut invalid = Vec::new();

    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rule = match parse_rule_file(&source) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let category = &rule.metadata.category;
        let prefix = category
            .split('/')
            .next()
            .unwrap_or(category);

        if !KNOWN_CATEGORY_PREFIXES.contains(&prefix) {
            invalid.push(format!(
                "{}: unknown category prefix '{}' \
                 in '{}'",
                file.display(),
                prefix,
                category
            ));
        }
    }

    assert!(
        invalid.is_empty(),
        "Rules with invalid category prefixes:\n  {}",
        invalid.join("\n  ")
    );
}

// ── Rule file location matches category ─────────────────────

#[test]
fn rule_file_path_matches_category() {
    let rules_dir = helpers::rules_dir();
    let files = helpers::collect_rra_files(&rules_dir);
    let mut mismatches = Vec::new();

    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rule = match parse_rule_file(&source) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let rel_path = file
            .strip_prefix(&rules_dir)
            .unwrap_or(file);
        let rel_str = rel_path.to_string_lossy();
        let category = &rule.metadata.category;

        if !rel_str.starts_with(category) {
            mismatches.push(format!(
                "{}: category '{}' does not match \
                 file path",
                file.display(),
                category
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "Rules whose file path does not match category:\n  {}",
        mismatches.join("\n  ")
    );
}
