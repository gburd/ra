//! Structured test case types parsed from `.rra` rule files.
//!
//! Each `.rra` file may contain SQL code blocks under `## Test Cases`.
//! This module parses those blocks into structured [`TestCase`] values
//! with expectations extracted from inline comments.

use std::fmt;

/// A single test case extracted from an `.rra` file.
#[derive(Debug, Clone, PartialEq)]
pub struct TestCase {
    /// The input SQL query to parse and optimize.
    pub input_sql: String,
    /// What outcome the test expects.
    pub expected: TestExpectation,
    /// Human-readable description from comments.
    pub description: Option<String>,
    /// Whether this is a negative test (optimization should NOT
    /// apply).
    pub negative: bool,
}

/// What a test case expects from the optimizer.
#[derive(Debug, Clone, PartialEq)]
pub enum TestExpectation {
    /// The plan should differ from the input (optimization applied).
    PlanChanged,
    /// The plan should remain unchanged (optimization does not
    /// apply).
    PlanUnchanged,
    /// The SQL should parse successfully (parse-only test).
    Parses,
    /// A specific rule should be verified as applied.
    RuleApplied {
        /// The rule ID that should have been applied.
        rule_id: String,
    },
    /// A specific comment describes the expected outcome (freeform).
    Described(String),
}

impl fmt::Display for TestExpectation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanChanged => write!(f, "plan changed"),
            Self::PlanUnchanged => write!(f, "plan unchanged"),
            Self::Parses => write!(f, "parses successfully"),
            Self::RuleApplied { rule_id } => {
                write!(f, "rule '{rule_id}' applied")
            }
            Self::Described(desc) => write!(f, "{desc}"),
        }
    }
}

/// Parse a single SQL code block into zero or more [`TestCase`]
/// values.
///
/// Extracts structured expectations from inline SQL comments:
/// - `-- Expected: ...` sets a freeform expectation
/// - `-- Expected-Rule: <id>` expects a specific rule applied
/// - `-- Positive: ...` marks a positive test case
/// - `-- Negative: ...` marks a negative test case
/// - `-- Before` / `-- After` separates input from expected output
///
/// When a block contains both `-- Before` and `-- After` sections,
/// only the `-- Before` SQL is used as input and the expectation
/// is `PlanChanged`.
///
/// When a block has no structured markers, each standalone SQL
/// statement becomes a `Parses` test.
#[must_use]
pub fn parse_test_block(block: &str, rule_id: &str, block_index: usize) -> Vec<TestCase> {
    let lines: Vec<&str> = block.lines().collect();
    if lines.is_empty() {
        return vec![];
    }

    // Check for Before/After pattern
    if has_before_after(&lines) {
        return parse_before_after(&lines, rule_id, block_index);
    }

    // Check for Input/Output pattern
    if has_input_output(&lines) {
        return parse_input_output(&lines, rule_id, block_index);
    }

    // Check for Positive/Negative markers
    if has_case_markers(&lines) {
        return parse_marked_cases(&lines, rule_id, block_index);
    }

    // Fall back: treat as single test case
    parse_single_block(&lines, rule_id, block_index)
}

fn has_before_after(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        let t = l.trim();
        t.starts_with("-- Before") || t.starts_with("-- Input")
    }) && lines.iter().any(|l| {
        let t = l.trim();
        t.starts_with("-- After") || t.starts_with("-- Output")
    })
}

fn has_input_output(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        let t = l.trim();
        t.starts_with("-- Input")
    }) && lines.iter().any(|l| {
        let t = l.trim();
        t.starts_with("-- Expected")
    })
}

fn has_case_markers(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        let t = l.trim();
        t.starts_with("-- Positive:") || t.starts_with("-- Negative:")
    })
}

fn parse_before_after(lines: &[&str], rule_id: &str, block_index: usize) -> Vec<TestCase> {
    let mut before_sql = String::new();
    let mut description = extract_description(lines);
    let negative = is_negative(lines);
    let mut in_before = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("-- Before") || trimmed.starts_with("-- Input") {
            in_before = true;
            if description.is_none() {
                let after_marker = strip_comment_prefix(trimmed, "-- Before")
                    .or_else(|| strip_comment_prefix(trimmed, "-- Input"));
                if let Some(desc) = after_marker {
                    if !desc.is_empty() {
                        description = Some(desc.to_owned());
                    }
                }
            }
            continue;
        }
        if trimmed.starts_with("-- After")
            || trimmed.starts_with("-- Output")
            || trimmed.starts_with("-- Expected")
        {
            break;
        }
        if in_before && !trimmed.starts_with("--") {
            if !before_sql.is_empty() {
                before_sql.push('\n');
            }
            before_sql.push_str(line);
        }
    }

    let before_sql = before_sql.trim().to_owned();
    if before_sql.is_empty() {
        return vec![];
    }

    let expected = if negative {
        TestExpectation::PlanUnchanged
    } else {
        TestExpectation::PlanChanged
    };

    vec![TestCase {
        input_sql: before_sql,
        expected,
        description: description.or_else(|| Some(format!("{rule_id}::block_{block_index}"))),
        negative,
    }]
}

fn parse_input_output(lines: &[&str], rule_id: &str, block_index: usize) -> Vec<TestCase> {
    let mut input_sql = String::new();
    let mut in_input = false;
    let negative = is_negative(lines);
    let description = extract_description(lines);
    let expectation = extract_expectation(lines);

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("-- Input") {
            in_input = true;
            continue;
        }
        if trimmed.starts_with("-- Expected") || trimmed.starts_with("-- Output") {
            break;
        }
        if in_input && !trimmed.starts_with("--") {
            if !input_sql.is_empty() {
                input_sql.push('\n');
            }
            input_sql.push_str(line);
        }
    }

    let input_sql = input_sql.trim().to_owned();
    if input_sql.is_empty() {
        return vec![];
    }

    let expected = expectation.unwrap_or(if negative {
        TestExpectation::PlanUnchanged
    } else {
        TestExpectation::PlanChanged
    });

    vec![TestCase {
        input_sql,
        expected,
        description: description.or_else(|| Some(format!("{rule_id}::block_{block_index}"))),
        negative,
    }]
}

fn parse_marked_cases(lines: &[&str], rule_id: &str, block_index: usize) -> Vec<TestCase> {
    let mut cases = Vec::new();
    let mut current_sql = String::new();
    let mut current_negative = false;
    let mut current_desc: Option<String> = None;
    let mut current_expectation: Option<TestExpectation> = None;
    let mut sub_index = 0u32;

    for line in lines {
        let trimmed = line.trim();

        if trimmed.starts_with("-- Positive:") {
            flush_case(
                &mut cases,
                &mut current_sql,
                current_negative,
                current_desc.take(),
                current_expectation.take(),
                rule_id,
                block_index,
                sub_index,
            );
            sub_index += 1;
            current_negative = false;
            current_desc = strip_comment_prefix(trimmed, "-- Positive:").map(str::to_owned);
            continue;
        }

        if trimmed.starts_with("-- Negative:") {
            flush_case(
                &mut cases,
                &mut current_sql,
                current_negative,
                current_desc.take(),
                current_expectation.take(),
                rule_id,
                block_index,
                sub_index,
            );
            sub_index += 1;
            current_negative = true;
            current_desc = strip_comment_prefix(trimmed, "-- Negative:").map(str::to_owned);
            continue;
        }

        if trimmed.starts_with("-- Expected-Rule:") {
            if let Some(id) = strip_comment_prefix(trimmed, "-- Expected-Rule:") {
                current_expectation = Some(TestExpectation::RuleApplied {
                    rule_id: id.to_owned(),
                });
            }
            continue;
        }

        if trimmed.starts_with("-- Expected:") {
            if let Some(desc) = strip_comment_prefix(trimmed, "-- Expected:") {
                current_expectation = Some(TestExpectation::Described(desc.to_owned()));
            }
            continue;
        }

        if !trimmed.starts_with("--") && !trimmed.is_empty() {
            if !current_sql.is_empty() {
                current_sql.push('\n');
            }
            current_sql.push_str(line);
        }
    }

    flush_case(
        &mut cases,
        &mut current_sql,
        current_negative,
        current_desc,
        current_expectation,
        rule_id,
        block_index,
        sub_index,
    );

    cases
}

#[allow(clippy::too_many_arguments)]
fn flush_case(
    cases: &mut Vec<TestCase>,
    sql: &mut String,
    negative: bool,
    description: Option<String>,
    expectation: Option<TestExpectation>,
    rule_id: &str,
    block_index: usize,
    sub_index: u32,
) {
    let trimmed = sql.trim().to_owned();
    if trimmed.is_empty() {
        return;
    }
    let expected = expectation.unwrap_or(if negative {
        TestExpectation::PlanUnchanged
    } else {
        TestExpectation::PlanChanged
    });
    cases.push(TestCase {
        input_sql: trimmed,
        expected,
        description: description
            .or_else(|| Some(format!("{rule_id}::block_{block_index}_{sub_index}"))),
        negative,
    });
    sql.clear();
}

fn parse_single_block(lines: &[&str], rule_id: &str, block_index: usize) -> Vec<TestCase> {
    let mut sql = String::new();
    let negative = is_negative(lines);
    let description = extract_description(lines);
    let expectation = extract_expectation(lines);

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("--") || trimmed.is_empty() {
            continue;
        }
        if !sql.is_empty() {
            sql.push('\n');
        }
        sql.push_str(line);
    }

    let sql = sql.trim().to_owned();
    if sql.is_empty() {
        return vec![];
    }

    let expected = expectation.unwrap_or(TestExpectation::Parses);

    vec![TestCase {
        input_sql: sql,
        expected,
        description: description.or_else(|| Some(format!("{rule_id}::block_{block_index}"))),
        negative,
    }]
}

fn is_negative(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        let t = l.trim().to_lowercase();
        t.starts_with("-- negative")
            || t.contains("should not apply")
            || t.contains("should not change")
            || t.contains("unchanged")
            || t.contains("cannot push")
    })
}

fn extract_description(lines: &[&str]) -> Option<String> {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("-- Positive:") {
            return strip_comment_prefix(trimmed, "-- Positive:").map(str::to_owned);
        }
        if trimmed.starts_with("-- Negative:") {
            return strip_comment_prefix(trimmed, "-- Negative:").map(str::to_owned);
        }
    }
    None
}

fn extract_expectation(lines: &[&str]) -> Option<TestExpectation> {
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("-- Expected-Rule:") {
            if let Some(id) = strip_comment_prefix(trimmed, "-- Expected-Rule:") {
                return Some(TestExpectation::RuleApplied {
                    rule_id: id.to_owned(),
                });
            }
        }
        if trimmed.starts_with("-- Expected:") {
            if let Some(desc) = strip_comment_prefix(trimmed, "-- Expected:") {
                let lower = desc.to_lowercase();
                if lower.contains("unchanged") || lower.contains("not apply") {
                    return Some(TestExpectation::PlanUnchanged);
                }
                if lower.contains("plan optimized") || lower.contains("plan changed") {
                    return Some(TestExpectation::PlanChanged);
                }
                return Some(TestExpectation::Described(desc.to_owned()));
            }
        }
    }
    None
}

fn strip_comment_prefix<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix)?;
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_before_after_block() {
        let block = "\
-- Before
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000;

-- After
SELECT * FROM (
    SELECT * FROM orders WHERE amount > 1000
) o
JOIN customers c ON o.customer_id = c.id;";

        let cases = parse_test_block(block, "filter-pushdown", 0);
        assert_eq!(cases.len(), 1);
        assert!(!cases[0].negative);
        assert_eq!(cases[0].expected, TestExpectation::PlanChanged);
        assert!(cases[0].input_sql.contains("orders"));
    }

    #[test]
    fn parse_negative_block() {
        let block = "\
-- Negative: predicate references both sides, cannot push
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > c.credit_limit;";

        let cases = parse_test_block(block, "filter-pushdown", 2);
        assert_eq!(cases.len(), 1);
        assert!(cases[0].negative);
    }

    #[test]
    fn parse_expected_comment() {
        let block = "\
-- Input
SELECT name FROM employees WHERE age > 30;

-- Expected: filter pushed below projection
-- Plan: Project[name](Filter[age > 30](Scan(employees)))";

        let cases = parse_test_block(block, "filter-pushdown-basic", 0);
        assert_eq!(cases.len(), 1);
        assert_eq!(
            cases[0].expected,
            TestExpectation::Described("filter pushed below projection".to_owned())
        );
    }

    #[test]
    fn parse_plain_sql() {
        let block = "SELECT * FROM users WHERE age > 18;";
        let cases = parse_test_block(block, "some-rule", 0);
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].expected, TestExpectation::Parses);
    }

    #[test]
    fn empty_block() {
        let cases = parse_test_block("", "some-rule", 0);
        assert!(cases.is_empty());
    }
}
