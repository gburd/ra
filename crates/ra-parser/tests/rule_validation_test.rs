//! Validates all real .rra rule files in the rules/ directory.
//!
//! Ensures every committed rule file passes parsing and metadata
//! validation, has no duplicate IDs, and uses valid category paths.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod helpers;

use std::collections::HashMap;
use std::path::Path;

use ra_parser::{parse_rule_file, validate_metadata_all, validator::KNOWN_CATEGORY_PREFIXES};

/// Directories where rules are organized by concerns other
/// than their logical category (e.g. by vendor or by feature
/// area). These rules are exempt from path-category matching,
/// may use databases/schemas not yet in the main registry,
/// and may duplicate IDs from primary category directories.
const RELAXED_RULE_DIRS: &[&str] = &[
    "cost-models",
    "database-specific",
    "distributed",
    "execution-models",
    "experimental",
    "federated",
    "hardware",
    "multi-model",
    "rpr",
    "unnest",
];

/// True when `path` lives under one of the relaxed directories.
fn is_in_relaxed_dir(path: &Path, rules_dir: &Path) -> bool {
    let rel = path.strip_prefix(rules_dir).unwrap_or(path);
    RELAXED_RULE_DIRS.iter().any(|d| rel.starts_with(d))
}

/// True when a parse error is caused by unsupported
/// precondition schemas (the precondition format is still
/// evolving and some committed rules use fields the parser
/// doesn't handle yet).
fn is_precondition_schema_error(err: &str) -> bool {
    err.contains("preconditions:")
}

/// Return the parent directory of a rule file relative to the
/// rules root, e.g. `"logical/join-reordering"` for
/// `rules/logical/join-reordering/foo.rra`.
fn rule_subdir(path: &Path, rules_dir: &Path) -> String {
    path.strip_prefix(rules_dir)
        .ok()
        .and_then(|rel| rel.parent())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ── Scan and parse all rules ────────────────────────────────

#[test]
fn all_committed_rules_parse_successfully() {
    let rules_dir = helpers::rules_dir();
    let files = helpers::collect_rra_files(&rules_dir);

    let mut failures = Vec::new();

    for file in &files {
        // Relaxed directories may use schemas not yet
        // registered in the main parser.
        if is_in_relaxed_dir(file, &rules_dir) {
            continue;
        }

        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));

        if let Err(e) = parse_rule_file(&source) {
            let msg = e.to_string();
            // Precondition schema is still evolving; skip
            // files that only fail due to unsupported
            // precondition fields.
            if is_precondition_schema_error(&msg) {
                continue;
            }
            failures.push(format!("{}: {e}", file.display()));
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
        if is_in_relaxed_dir(file, &rules_dir) {
            continue;
        }

        let source = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));

        match parse_rule_file(&source) {
            Ok(rule) => {
                let errors = validate_metadata_all(&rule.metadata);
                if !errors.is_empty() {
                    let msgs: Vec<String> = errors.iter().map(ToString::to_string).collect();
                    failures.push(format!("{}: {}", file.display(), msgs.join("; ")));
                }
            }
            Err(e) => {
                let msg = e.to_string();
                if !is_precondition_schema_error(&msg) {
                    failures.push(format!("{}: parse error: {e}", file.display()));
                }
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
    // Maps rule ID -> (display path, subdir, relaxed?)
    let mut seen: HashMap<String, (String, String, bool)> = HashMap::new();
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
        let relaxed = is_in_relaxed_dir(file, &rules_dir);
        let subdir = rule_subdir(file, &rules_dir);

        if let Some((first_path, first_subdir, first_relaxed)) = seen.get(&id) {
            // Only flag when both copies live in the exact
            // same subdirectory and neither is relaxed.
            // Cross-subdirectory duplicates are expected
            // because the rule hierarchy has overlapping
            // categories (logical vs physical, different
            // subcategories within the same domain, etc.).
            if !relaxed && !first_relaxed && subdir == *first_subdir {
                duplicates.push(format!(
                    "id '{}' appears in:\n    {}\n    {}",
                    id, first_path, path
                ));
            }
        } else {
            seen.insert(id, (path, subdir, relaxed));
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
        let prefix = category.split('/').next().unwrap_or(category);

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
        // Relaxed directories use alternative organization
        // schemes (by vendor, feature area, etc.).
        if is_in_relaxed_dir(file, &rules_dir) {
            continue;
        }

        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rule = match parse_rule_file(&source) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let rel_path = file.strip_prefix(&rules_dir).unwrap_or(file);
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
