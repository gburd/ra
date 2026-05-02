//! Main parser for `.rra` literate rule files.
//!
//! Parses markdown files with YAML frontmatter into structured
//! [`RuleFile`] values containing metadata, prose, and code blocks.

use pulldown_cmark::{Event, Options, Parser as MdParser, Tag};
use ra_core::PreCondition;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::extractor::{extract_code_blocks, CodeBlock};
use crate::validator::{self, ValidationError};

// ── Error types ──────────────────────────────────────────────

/// Errors that can occur while parsing an `.rra` file.
#[derive(Debug, Error)]
pub enum ParseError {
    /// The file contains no YAML frontmatter delimiters (`---`).
    #[error("missing YAML frontmatter")]
    MissingFrontmatter,

    /// YAML frontmatter could not be deserialized.
    #[error("invalid frontmatter YAML at line {line}: {source}")]
    InvalidYaml {
        /// Approximate line in the source file.
        line: usize,
        /// Underlying YAML error.
        source: serde_yml::Error,
    },

    /// Frontmatter values failed validation.
    #[error("validation failed: {0}")]
    Validation(#[from] ValidationError),

    /// Generic I/O or conversion failure.
    #[error("{0}")]
    Other(String),
}

// ── Public types ─────────────────────────────────────────────

/// A fully parsed `.rra` rule file.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleFile {
    /// YAML frontmatter metadata.
    pub metadata: RuleMetadata,
    /// Human-readable description extracted from the Description
    /// section.
    pub description: String,
    /// Formal relational algebra notation.
    pub algebra_notation: Option<String>,
    /// Rust implementation code.
    pub implementation: Option<String>,
    /// SQL or other test-case code blocks.
    pub test_cases: Vec<String>,
    /// Reference strings (URLs, citations).
    pub references: Vec<String>,
}

/// Metadata extracted from YAML frontmatter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleMetadata {
    /// Unique rule identifier (e.g. `filter-through-join`).
    pub id: String,
    /// Human-friendly name.
    pub name: String,
    /// Category path (e.g. `logical/predicate-pushdown`).
    pub category: String,
    /// Databases this rule applies to.
    #[serde(default)]
    pub databases: Vec<String>,
    /// SQL standard reference (e.g. `sql:1992`).
    #[serde(default)]
    pub standard: Option<String>,
    /// Execution models this rule supports.
    #[serde(default)]
    pub execution_models: Vec<String>,
    /// Semantic version of the rule.
    #[serde(default = "default_version")]
    pub version: String,
    /// Authors of the rule.
    #[serde(default)]
    pub authors: Vec<String>,
    /// Free-form tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Formal pre-conditions (optional, for rule filtering).
    #[serde(default)]
    pub preconditions: Vec<PreCondition>,
}

fn default_version() -> String {
    "0.1.0".to_owned()
}

// ── Section tracker ──────────────────────────────────────────

/// Which markdown `## Section` we are currently inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Description,
    Algebra,
    Implementation,
    TestCases,
    References,
    Other,
}

impl Section {
    fn from_heading(text: &str) -> Self {
        let lower = text.trim().to_lowercase();
        if lower.contains("description") {
            Self::Description
        } else if lower.contains("relational algebra") || lower.contains("algebra") {
            Self::Algebra
        } else if lower.contains("implementation") {
            Self::Implementation
        } else if lower.contains("test") {
            Self::TestCases
        } else if lower.contains("reference") {
            Self::References
        } else {
            Self::Other
        }
    }
}

// ── Frontmatter splitting ────────────────────────────────────

/// Split `---`-delimited YAML frontmatter from the remaining
/// markdown body.  Returns `(yaml, body, yaml_end_line)`.
fn split_frontmatter(source: &str) -> Result<(&str, &str, usize), ParseError> {
    let trimmed = source.trim_start();
    if !trimmed.starts_with("---") {
        return Err(ParseError::MissingFrontmatter);
    }
    let after_open = trimmed.find('\n').map_or(trimmed.len(), |i| i + 1);
    let rest = &trimmed[after_open..];
    let close = rest.find("\n---").ok_or(ParseError::MissingFrontmatter)?;
    let yaml = &rest[..close];
    // +2: one for the opening `---` line, one for the closing
    let yaml_end_line = yaml.lines().count() + 2;
    let body_start = close + 4; // skip `\n---`
    let body = rest
        .get(body_start..)
        .map_or("", |s| s.strip_prefix('\n').unwrap_or(s));
    Ok((yaml, body, yaml_end_line))
}

// ── Parse markdown body ──────────────────────────────────────

/// Walk the pulldown-cmark event stream and collect prose and
/// code blocks into the appropriate sections.
fn parse_body(body: &str) -> BodyParts {
    let opts =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_SMART_PUNCTUATION;
    let parser = MdParser::new_ext(body, opts);

    let mut parts = BodyParts::default();
    let mut current_section = Section::Description;
    let mut in_heading = false;
    let mut heading_buf = String::new();
    let mut text_buf = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading(pulldown_cmark::HeadingLevel::H2, ..)) => {
                flush_text(&mut parts, current_section, &text_buf);
                text_buf.clear();
                in_heading = true;
                heading_buf.clear();
            }
            Event::End(Tag::Heading(pulldown_cmark::HeadingLevel::H2, ..)) => {
                current_section = Section::from_heading(&heading_buf);
                in_heading = false;
            }
            Event::Text(ref t) | Event::Code(ref t) => {
                if in_heading {
                    heading_buf.push_str(t);
                } else {
                    text_buf.push_str(t);
                }
            }
            Event::SoftBreak | Event::HardBreak if !in_heading => {
                text_buf.push('\n');
            }
            _ => {}
        }
    }
    flush_text(&mut parts, current_section, &text_buf);

    let code_blocks = extract_code_blocks(body);
    assign_code_blocks(&mut parts, &code_blocks);

    parts
}

fn flush_text(parts: &mut BodyParts, section: Section, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    match section {
        Section::Description => {
            if !parts.description.is_empty() {
                parts.description.push('\n');
            }
            parts.description.push_str(trimmed);
        }
        Section::References => {
            for line in trimmed.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    parts.references.push(line.to_owned());
                }
            }
        }
        Section::Algebra | Section::Implementation | Section::TestCases | Section::Other => {}
    }
}

fn assign_code_blocks(parts: &mut BodyParts, blocks: &[CodeBlock]) {
    for block in blocks {
        match block.language.as_str() {
            "algebra" | "ra" => {
                parts.algebra_notation = Some(block.content.clone());
            }
            "rust" => {
                parts.implementation = Some(block.content.clone());
            }
            "sql" | "test" | "tla" => {
                parts.test_cases.push(block.content.clone());
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct BodyParts {
    description: String,
    algebra_notation: Option<String>,
    implementation: Option<String>,
    test_cases: Vec<String>,
    references: Vec<String>,
}

// ── Public API ───────────────────────────────────────────────

/// Parse the full text of an `.rra` file into a [`RuleFile`].
///
/// Performs frontmatter deserialization, markdown extraction,
/// and metadata validation in one step.
///
/// # Errors
///
/// Returns [`ParseError`] when:
/// - YAML frontmatter is missing or malformed
/// - Metadata fields fail validation
pub fn parse_rule_file(source: &str) -> Result<RuleFile, ParseError> {
    let (yaml, body, yaml_end_line) = split_frontmatter(source)?;

    let metadata: RuleMetadata =
        serde_yml::from_str(yaml).map_err(|e| ParseError::InvalidYaml {
            line: yaml_end_line,
            source: e,
        })?;

    validator::validate_metadata(&metadata)?;

    let parts = parse_body(body);

    Ok(RuleFile {
        metadata,
        description: parts.description,
        algebra_notation: parts.algebra_notation,
        implementation: parts.implementation,
        test_cases: parts.test_cases,
        references: parts.references,
    })
}

/// Parse only the YAML frontmatter, skipping body extraction.
///
/// Useful for quick metadata queries without full parsing.
///
/// # Errors
///
/// Returns [`ParseError`] when frontmatter is missing or
/// contains invalid YAML.
pub fn parse_metadata(source: &str) -> Result<RuleMetadata, ParseError> {
    let (yaml, _, yaml_end_line) = split_frontmatter(source)?;
    serde_yml::from_str(yaml).map_err(|e| ParseError::InvalidYaml {
        line: yaml_end_line,
        source: e,
    })
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    const SAMPLE_RRA: &str = "\
---
id: filter-through-join
name: Filter Pushdown Through Join
category: logical/predicate-pushdown
databases: [postgresql, mysql, duckdb, sqlite]
standard: \"sql:1992\"
version: \"1.0.0\"
authors: [\"Alice\", \"Bob\"]
tags: [filter, join, pushdown]
---

# Filter Pushdown Through Join

## Description

Pushes selection predicates through join operators when the
predicate only references columns from one side of the join.

## Relational Algebra

```algebra
sigma[p](R join[c] S) -> (sigma[p](R)) join[c] S
  where attrs(p) subset attrs(R)
```

## Implementation

```rust
fn rewrite(plan: &Plan) -> Plan {
    // push filter below join
    Plan::Join {
        left: Box::new(Plan::Filter {
            predicate: p.clone(),
            input: Box::new(left.clone()),
        }),
        right: right.clone(),
        condition: c.clone(),
    }
}
```

## Test Cases

```sql
-- Before
SELECT * FROM orders o JOIN items i ON o.id = i.order_id
WHERE o.status = 'shipped';

-- After
SELECT * FROM (SELECT * FROM orders WHERE status = 'shipped') o
JOIN items i ON o.id = i.order_id;
```

## References

Selinger et al. \"Access Path Selection in a Relational Database Management System\" (1979)
Graefe, \"The Cascades Framework for Query Optimization\" (1995)
";

    #[test]
    fn parse_full_rule_file() {
        let rule = parse_rule_file(SAMPLE_RRA).expect("should parse sample");
        assert_eq!(rule.metadata.id, "filter-through-join");
        assert_eq!(rule.metadata.name, "Filter Pushdown Through Join");
        assert_eq!(rule.metadata.category, "logical/predicate-pushdown");
        assert_eq!(
            rule.metadata.databases,
            vec!["postgresql", "mysql", "duckdb", "sqlite"]
        );
        assert_eq!(rule.metadata.standard.as_deref(), Some("sql:1992"));
        assert_eq!(rule.metadata.version, "1.0.0");
        assert_eq!(rule.metadata.authors.len(), 2);
        assert!(rule.description.contains("selection predicates"));
        assert!(rule.algebra_notation.is_some());
        assert!(rule.implementation.is_some());
        assert!(!rule.test_cases.is_empty());
        assert_eq!(rule.references.len(), 2);
    }

    #[test]
    fn parse_metadata_only() {
        let meta = parse_metadata(SAMPLE_RRA).expect("should parse metadata");
        assert_eq!(meta.id, "filter-through-join");
        assert_eq!(meta.tags, vec!["filter", "join", "pushdown"]);
    }

    #[test]
    fn missing_frontmatter() {
        let bad = "# No frontmatter here\nJust markdown.";
        let err = parse_rule_file(bad).unwrap_err();
        assert!(
            matches!(err, ParseError::MissingFrontmatter),
            "expected MissingFrontmatter, got: {err}"
        );
    }

    #[test]
    fn unclosed_frontmatter() {
        let bad = "---\nid: test\nname: Test\n";
        let err = parse_rule_file(bad).unwrap_err();
        assert!(matches!(err, ParseError::MissingFrontmatter));
    }

    #[test]
    fn invalid_yaml_in_frontmatter() {
        let bad = "---\n: [invalid yaml\n---\n# Body";
        let err = parse_rule_file(bad).unwrap_err();
        assert!(matches!(err, ParseError::InvalidYaml { .. }));
    }

    #[test]
    fn minimal_valid_rule() {
        let src = "\
---
id: minimal
name: Minimal Rule
category: logical/expression-simplification
---

## Description

A minimal rule for testing.
";
        let rule = parse_rule_file(src).expect("should parse minimal rule");
        assert_eq!(rule.metadata.id, "minimal");
        assert_eq!(rule.metadata.version, "0.1.0");
        assert!(rule.metadata.databases.is_empty());
        assert!(rule.algebra_notation.is_none());
        assert!(rule.implementation.is_none());
        assert!(rule.test_cases.is_empty());
    }

    #[test]
    fn frontmatter_defaults() {
        let src = "---\nid: x\nname: X\n\
                   category: logical/set-operations\n---\n";
        let meta = parse_metadata(src).expect("should parse");
        assert_eq!(meta.version, "0.1.0");
        assert!(meta.databases.is_empty());
        assert!(meta.execution_models.is_empty());
        assert!(meta.authors.is_empty());
        assert!(meta.tags.is_empty());
        assert!(meta.standard.is_none());
    }

    #[test]
    fn validation_rejects_empty_id() {
        let src = "---\nid: \"\"\nname: X\n\
                   category: logical/set-operations\n---\n";
        let err = parse_rule_file(src).unwrap_err();
        assert!(
            matches!(err, ParseError::Validation(_)),
            "expected validation error, got: {err}"
        );
    }

    #[test]
    fn multiple_test_blocks() {
        let src = "\
---
id: multi-test
name: Multi Test
category: logical/join-reordering
---

## Test Cases

```sql
SELECT 1;
```

```sql
SELECT 2;
```
";
        let rule = parse_rule_file(src).expect("should parse");
        assert_eq!(rule.test_cases.len(), 2);
    }
}
