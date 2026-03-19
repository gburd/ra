//! End-to-end tests for the `ra-cli` binary.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::NamedTempFile;

// ── Helpers ─────────────────────────────────────────────────

fn ra_cli() -> Command {
    Command::cargo_bin("ra-cli")
        .expect("ra-cli binary should be built")
}

fn workspace_root() -> std::path::PathBuf {
    let crate_dir = env!("CARGO_MANIFEST_DIR");
    std::path::Path::new(crate_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn fixtures_dir() -> String {
    workspace_root()
        .join("tests")
        .join("fixtures")
        .to_string_lossy()
        .into_owned()
}

fn rules_dir() -> String {
    workspace_root()
        .join("rules")
        .to_string_lossy()
        .into_owned()
}

fn write_temp_rra(content: &str) -> NamedTempFile {
    let mut f = tempfile::Builder::new()
        .suffix(".rra")
        .tempfile()
        .expect("create temp file");
    f.write_all(content.as_bytes())
        .expect("write temp file");
    f.flush().expect("flush temp file");
    f
}

const VALID_MINIMAL: &str = "\
---
id: minimal-test
name: Minimal Test Rule
category: logical/expression-simplification
version: \"1.0.0\"
---

## Description

A minimal rule for testing.
";

// ── validate: success cases ─────────────────────────────────

#[test]
fn validate_valid_simple_rule_reports_pass() {
    let fixture = format!(
        "{}/valid-simple-rule.rra",
        fixtures_dir()
    );

    ra_cli()
        .args(["validate", &fixture])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "1 file(s) passed validation",
        ));
}

#[test]
fn validate_valid_complex_rule_reports_pass() {
    let fixture = format!(
        "{}/valid-complex-rule.rra",
        fixtures_dir()
    );

    ra_cli()
        .args(["validate", &fixture])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "1 file(s) passed validation",
        ));
}

#[test]
fn validate_temp_rule_with_good_content_passes() {
    let tmp = write_temp_rra(VALID_MINIMAL);

    ra_cli()
        .args(["validate", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("passed validation"));
}

// ── validate: failure cases ─────────────────────────────────

#[test]
fn validate_invalid_bad_yaml_fails_with_detail() {
    let fixture = format!(
        "{}/invalid-bad-yaml.rra",
        fixtures_dir()
    );

    ra_cli()
        .args(["validate", &fixture])
        .assert()
        .failure()
        .stderr(predicate::str::contains("[FAIL]"));
}

#[test]
fn validate_invalid_missing_fields_fails() {
    let fixture = format!(
        "{}/invalid-missing-fields.rra",
        fixtures_dir()
    );

    ra_cli()
        .args(["validate", &fixture])
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed validation"));
}

#[test]
fn validate_invalid_bad_category_shows_category_error() {
    let fixture = format!(
        "{}/invalid-bad-category.rra",
        fixtures_dir()
    );

    ra_cli()
        .args(["validate", &fixture])
        .assert()
        .failure()
        .stderr(predicate::str::contains("category"));
}

#[test]
fn validate_nonexistent_path_reports_not_found() {
    ra_cli()
        .args(["validate", "/nonexistent/path.rra"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn validate_bad_frontmatter_content() {
    let content = "---\n: [broken yaml {{{\n---\n# Body\n";
    let tmp = write_temp_rra(content);

    ra_cli()
        .args(["validate", tmp.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("[FAIL]"));
}

#[test]
fn validate_no_frontmatter() {
    let content = "# Just markdown, no frontmatter\n";
    let tmp = write_temp_rra(content);

    ra_cli()
        .args(["validate", tmp.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("frontmatter"));
}

// ── validate: directory scanning ────────────────────────────

#[test]
fn validate_fixtures_directory_reports_multiple_files() {
    ra_cli()
        .args(["validate", &fixtures_dir()])
        .assert()
        .failure() // some fixtures are intentionally invalid
        .stderr(
            predicate::str::contains("Validating")
                .and(predicate::str::contains("file(s)")),
        );
}

#[test]
fn validate_directory_counts_passes_and_failures() {
    ra_cli()
        .args(["validate", &fixtures_dir()])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("passed")
                .and(predicate::str::contains("failed")),
        );
}

// ── validate: verbose flag ──────────────────────────────────

#[test]
fn validate_verbose_shows_pass_for_each_file() {
    let tmp = write_temp_rra(VALID_MINIMAL);

    ra_cli()
        .args([
            "--verbose",
            "validate",
            tmp.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("[PASS]"));
}

#[test]
fn validate_without_verbose_hides_pass_status() {
    let tmp = write_temp_rra(VALID_MINIMAL);

    ra_cli()
        .args(["validate", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("[PASS]").not());
}

// ── validate: quiet flag ────────────────────────────────────

#[test]
fn validate_quiet_suppresses_all_output_on_success() {
    let tmp = write_temp_rra(VALID_MINIMAL);

    let output = ra_cli()
        .args([
            "--quiet",
            "validate",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("run ra-cli");

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "quiet mode should produce no stderr, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn validate_quiet_still_fails_on_invalid_input() {
    let fixture = format!(
        "{}/invalid-bad-yaml.rra",
        fixtures_dir()
    );

    ra_cli()
        .args(["--quiet", "validate", &fixture])
        .assert()
        .failure();
}

// ── list command ────────────────────────────────────────────

#[test]
fn list_fixtures_dir_shows_rule_count() {
    ra_cli()
        .args(["list", "--dir", &fixtures_dir()])
        .assert()
        .success()
        .stderr(predicate::str::contains("rule(s) found"));
}

#[test]
fn list_fixtures_shows_table_headers() {
    ra_cli()
        .args(["list", "--dir", &fixtures_dir()])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("ID")
                .and(predicate::str::contains("NAME"))
                .and(predicate::str::contains("CATEGORY")),
        );
}

#[test]
fn list_fixtures_includes_known_rule_id() {
    ra_cli()
        .args(["list", "--dir", &fixtures_dir()])
        .assert()
        .success()
        .stderr(predicate::str::contains("filter-pushdown-basic"));
}

#[test]
fn list_rules_dir_shows_committed_rules() {
    ra_cli()
        .args(["list", "--dir", &rules_dir()])
        .assert()
        .success()
        .stderr(predicate::str::contains("rule(s) found"));
}

#[test]
fn list_nonexistent_dir_fails() {
    ra_cli()
        .args(["list", "--dir", "/nonexistent/dir"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

// ── show command ────────────────────────────────────────────

#[test]
fn show_existing_rule_displays_metadata() {
    ra_cli()
        .args([
            "show",
            "filter-pushdown-basic",
            "--dir",
            &fixtures_dir(),
        ])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("filter-pushdown-basic")
                .and(predicate::str::contains("Name"))
                .and(predicate::str::contains(
                    "Basic Filter Pushdown",
                ))
                .and(predicate::str::contains("Category"))
                .and(predicate::str::contains(
                    "logical/predicate-pushdown",
                )),
        );
}

#[test]
fn show_complex_rule_displays_all_sections() {
    ra_cli()
        .args([
            "show",
            "join-commutativity",
            "--dir",
            &fixtures_dir(),
        ])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("Join Commutativity")
                .and(predicate::str::contains("Description"))
                .and(predicate::str::contains(
                    "Relational Algebra",
                ))
                .and(predicate::str::contains("Implementation"))
                .and(predicate::str::contains("Test Cases")),
        );
}

#[test]
fn show_nonexistent_rule_fails_with_hint() {
    ra_cli()
        .args([
            "show",
            "no-such-rule",
            "--dir",
            &fixtures_dir(),
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("not found").and(
                predicate::str::contains("ra-cli list"),
            ),
        );
}

#[test]
fn show_rule_from_real_rules_dir() {
    ra_cli()
        .args([
            "show",
            "filter-through-join",
            "--dir",
            &rules_dir(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Filter Pushdown Through Join",
        ));
}

// ── test command ────────────────────────────────────────────

#[test]
fn test_command_shows_summary() {
    // Fixtures include tests that may fail, so don't check exit code.
    ra_cli()
        .args(["test", &fixtures_dir()])
        .assert()
        .stderr(predicate::str::contains("Summary:"));
}

#[test]
fn test_command_reports_pass_rate() {
    ra_cli()
        .args(["test", &fixtures_dir()])
        .assert()
        .stderr(predicate::str::contains("passed"));
}

#[test]
fn test_command_shows_per_file_results() {
    ra_cli()
        .args(["test", &fixtures_dir(), "--verbose"])
        .assert()
        .stderr(predicate::str::contains("passed)"));
}

#[test]
fn test_command_shows_duration() {
    ra_cli()
        .args(["test", &fixtures_dir()])
        .assert()
        .stderr(predicate::str::contains("Duration"));
}

#[test]
fn test_single_valid_file() {
    let fixture = format!(
        "{}/valid-simple-rule.rra",
        fixtures_dir()
    );

    ra_cli()
        .args(["test", &fixture])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("Summary:")
                .and(predicate::str::contains("passed")),
        );
}

#[test]
fn test_nonexistent_path_fails() {
    ra_cli()
        .args(["test", "/nonexistent/path.rra"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

// ── optimize command (stub) ─────────────────────────────────

#[test]
fn optimize_stub_succeeds_and_shows_input() {
    ra_cli()
        .args(["optimize", "SELECT * FROM users"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("Query Optimization")
                .and(predicate::str::contains("SELECT * FROM users")),
        );
}

#[test]
fn optimize_quiet_produces_no_output() {
    let output = ra_cli()
        .args([
            "--quiet",
            "optimize",
            "SELECT * FROM users",
        ])
        .output()
        .expect("run ra-cli");

    assert!(output.status.success());
    // With actual optimize implementation, --quiet still suppresses verbose output
    // but may produce minimal result output
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr_str.is_empty() || !stderr_str.contains("Query Optimization"),
        "quiet optimize should suppress verbose output, got: {}", stderr_str
    );
}

// ── general CLI behavior ────────────────────────────────────

#[test]
fn no_subcommand_shows_usage() {
    ra_cli()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn help_flag_shows_all_commands() {
    ra_cli()
        .args(["--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("validate")
                .and(predicate::str::contains("test"))
                .and(predicate::str::contains("list"))
                .and(predicate::str::contains("show"))
                .and(predicate::str::contains("optimize")),
        );
}

#[test]
fn validate_subcommand_help() {
    ra_cli()
        .args(["validate", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Validate"));
}

#[test]
fn unknown_subcommand_fails() {
    ra_cli()
        .args(["nonexistent-command"])
        .assert()
        .failure();
}
