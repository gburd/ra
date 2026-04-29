//! End-to-end tests for the .rra parser against fixture files.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod helpers;

use ra_parser::{parse_metadata, parse_rule_file, validate_metadata_all, ParseError};

// ── Valid fixtures ──────────────────────────────────────────

#[test]
fn parse_valid_simple_rule_fixture() {
    let source = helpers::load_fixture("valid-simple-rule.rra");
    let rule = parse_rule_file(&source).expect("valid-simple-rule.rra should parse");

    assert_eq!(rule.metadata.id, "filter-pushdown-basic");
    assert_eq!(rule.metadata.name, "Basic Filter Pushdown");
    assert_eq!(rule.metadata.category, "logical/predicate-pushdown");
    assert!(
        !rule.description.is_empty(),
        "description should not be empty"
    );
}

#[test]
fn parse_valid_complex_rule_fixture() {
    let source = helpers::load_fixture("valid-complex-rule.rra");
    let rule = parse_rule_file(&source).expect("valid-complex-rule.rra should parse");

    assert_eq!(rule.metadata.id, "join-commutativity");
    assert_eq!(rule.metadata.name, "Join Commutativity");
    assert_eq!(rule.metadata.category, "logical/join-reordering");
    assert_eq!(rule.metadata.version, "1.0.0");

    assert!(
        !rule.metadata.databases.is_empty(),
        "databases should be present"
    );
    assert!(
        !rule.metadata.authors.is_empty(),
        "authors should be present"
    );
    assert!(!rule.metadata.tags.is_empty(), "tags should be present");
    assert!(
        rule.metadata.standard.is_some(),
        "standard should be present"
    );

    assert!(
        !rule.description.is_empty(),
        "description should be present"
    );
    assert!(
        rule.algebra_notation.is_some(),
        "algebra notation should be present"
    );
    assert!(
        rule.implementation.is_some(),
        "implementation should be present"
    );
    assert!(!rule.test_cases.is_empty(), "test cases should be present");

    let errors = validate_metadata_all(&rule.metadata);
    assert!(
        errors.is_empty(),
        "valid-complex-rule should have no validation errors, \
         got: {errors:?}"
    );
}

// ── Invalid fixtures ────────────────────────────────────────

#[test]
fn parse_invalid_missing_fields_fixture() {
    let source = helpers::load_fixture("invalid-missing-fields.rra");
    let result = parse_rule_file(&source);
    assert!(
        result.is_err(),
        "invalid-missing-fields.rra should fail to parse"
    );
}

#[test]
fn parse_invalid_bad_yaml_fixture() {
    let source = helpers::load_fixture("invalid-bad-yaml.rra");
    let err = parse_rule_file(&source).expect_err("invalid-bad-yaml.rra should fail");
    assert!(
        matches!(err, ParseError::InvalidYaml { .. }),
        "expected InvalidYaml, got: {err}"
    );
}

#[test]
fn parse_invalid_bad_category_fixture() {
    let source = helpers::load_fixture("invalid-bad-category.rra");
    let result = parse_rule_file(&source);
    assert!(
        result.is_err(),
        "invalid-bad-category.rra should fail validation"
    );

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("nonexistent") || err_msg.contains("category"),
        "error should mention the bad category, got: {err_msg}"
    );
}

// ── Error message quality ───────────────────────────────────

#[test]
fn missing_frontmatter_error_is_descriptive() {
    let err = parse_rule_file("# No frontmatter here").expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("frontmatter"),
        "error should mention frontmatter, got: {msg}"
    );
}

#[test]
fn invalid_yaml_error_includes_line_info() {
    let bad = "---\n: [invalid yaml {{{\n---\n# Body\n";
    let err = parse_rule_file(bad).expect_err("should fail");
    match err {
        ParseError::InvalidYaml { line, .. } => {
            assert!(line > 0, "line number should be positive, got: {line}");
        }
        other => {
            panic!("expected InvalidYaml, got: {other}");
        }
    }
}

#[test]
fn validation_error_names_the_field() {
    let src = "---\nid: \"\"\nname: X\n\
               category: logical/predicate-pushdown\n---\n";
    let err = parse_rule_file(src).expect_err("should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("id"),
        "error should name the failing field, got: {msg}"
    );
}

// ── Round-trip (parse metadata -> verify fields) ─────────────

#[test]
fn metadata_round_trip_for_all_valid_fixtures() {
    for name in &["valid-simple-rule.rra", "valid-complex-rule.rra"] {
        let source = helpers::load_fixture(name);

        let full =
            parse_rule_file(&source).unwrap_or_else(|e| panic!("{name} full parse failed: {e}"));

        let meta_only =
            parse_metadata(&source).unwrap_or_else(|e| panic!("{name} metadata parse failed: {e}"));

        assert_eq!(
            full.metadata, meta_only,
            "{name}: full parse and metadata-only parse \
             should yield identical metadata"
        );
    }
}
