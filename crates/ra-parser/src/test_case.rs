//! Structured test case extraction from `.rra` code blocks.
//!
//! Parses SQL test blocks with inline comment annotations into
//! structured [`TestCase`] values for automated execution.

/// A single test case extracted from an `.rra` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCase {
    /// SQL input to parse and optimize.
    pub input_sql: String,
    /// What outcome is expected after optimization.
    pub expectation: TestExpectation,
    /// Human-readable label (from comment annotations).
    pub label: String,
    /// The rule ID this test belongs to.
    pub rule_id: String,
}

/// What a test expects the optimizer to produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestExpectation {
    /// The optimizer should change the plan (positive test).
    PlanChanged,
    /// The optimizer should not change the plan (negative test).
    PlanUnchanged,
    /// The optimizer should produce a plan matching the given SQL.
    PlanMatchesSql(String),
    /// A specific rule should be applied (by rule ID).
    RuleApplied(String),
    /// No specific expectation; just verify the SQL parses and
    /// optimizes without error.
    NoError,
}

/// Parse a raw test-case code block into structured [`TestCase`]
/// values.
///
/// A single code block may contain multiple test cases separated
/// by annotations like `-- Positive:`, `-- Negative:`, or
/// `-- Before`/`-- After` pairs.
#[must_use]
pub fn parse_test_block(
    block: &str,
    rule_id: &str,
) -> Vec<TestCase> {
    let lines: Vec<&str> = block.lines().collect();
    let mut cases = Vec::new();
    let mut idx = 0;

    while idx < lines.len() {
        let line = lines[idx].trim();
        idx += 1;

        if let Some(parsed) =
            try_parse_annotation(line, &lines, &mut idx, rule_id)
        {
            cases.push(parsed);
        } else if is_before_marker(line) {
            if let Some(parsed) =
                parse_before_after(&lines, &mut idx, rule_id)
            {
                cases.push(parsed);
            }
        }
    }

    if cases.is_empty() {
        let sql = extract_bare_sql(block);
        if !sql.is_empty() {
            cases.push(TestCase {
                input_sql: sql,
                expectation: TestExpectation::NoError,
                label: String::new(),
                rule_id: rule_id.to_owned(),
            });
        }
    }

    cases
}

fn try_parse_annotation(
    line: &str,
    lines: &[&str],
    idx: &mut usize,
    rule_id: &str,
) -> Option<TestCase> {
    if let Some(label) = strip_annotation(line, "Positive:") {
        let (sql, after_sql, next) =
            collect_sql_section(lines, *idx);
        *idx = next;
        if sql.is_empty() {
            return None;
        }
        let expectation = after_sql.map_or(
            TestExpectation::PlanChanged,
            TestExpectation::PlanMatchesSql,
        );
        return Some(TestCase {
            input_sql: sql,
            expectation,
            label: label.to_owned(),
            rule_id: rule_id.to_owned(),
        });
    }

    if let Some(label) = strip_annotation(line, "Negative:") {
        let (sql, _, next) = collect_sql_section(lines, *idx);
        *idx = next;
        if sql.is_empty() {
            return None;
        }
        return Some(TestCase {
            input_sql: sql,
            expectation: TestExpectation::PlanUnchanged,
            label: label.to_owned(),
            rule_id: rule_id.to_owned(),
        });
    }

    if let Some(label) = strip_annotation(line, "Expected:") {
        let (sql, _, next) = collect_sql_section(lines, *idx);
        *idx = next;
        if sql.is_empty() {
            return None;
        }
        return Some(TestCase {
            input_sql: sql,
            expectation: TestExpectation::PlanChanged,
            label: label.to_owned(),
            rule_id: rule_id.to_owned(),
        });
    }

    if let Some(rule) = strip_annotation(line, "Expected-Rule:") {
        let (sql, _, next) = collect_sql_section(lines, *idx);
        *idx = next;
        if sql.is_empty() {
            return None;
        }
        return Some(TestCase {
            input_sql: sql,
            expectation: TestExpectation::RuleApplied(
                rule.to_owned(),
            ),
            label: format!("rule {rule} applied"),
            rule_id: rule_id.to_owned(),
        });
    }

    None
}

fn parse_before_after(
    lines: &[&str],
    idx: &mut usize,
    rule_id: &str,
) -> Option<TestCase> {
    let (before_sql, after_sql, next) =
        collect_sql_section(lines, *idx);
    *idx = next;
    // If collect_sql_section didn't find an after block inline,
    // try scanning forward for a standalone `-- After` marker.
    let after_sql =
        after_sql.or_else(|| collect_after_sql(lines, idx));
    if before_sql.is_empty() {
        return None;
    }
    let expectation = after_sql.map_or(
        TestExpectation::PlanChanged,
        TestExpectation::PlanMatchesSql,
    );
    Some(TestCase {
        input_sql: before_sql,
        expectation,
        label: "before/after".to_owned(),
        rule_id: rule_id.to_owned(),
    })
}

/// Strip a `-- <prefix>` annotation and return the remainder.
fn strip_annotation<'a>(
    line: &'a str,
    prefix: &str,
) -> Option<&'a str> {
    let stripped = line.strip_prefix("--")?.trim_start();
    let rest = stripped.strip_prefix(prefix)?;
    Some(rest.trim())
}

fn is_before_marker(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix("--") else {
        return false;
    };
    let rest = rest.trim();
    rest.eq_ignore_ascii_case("before")
        || rest.starts_with("Before:")
        || rest.starts_with("Before ")
}

fn is_after_marker(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix("--") else {
        return false;
    };
    let rest = rest.trim();
    rest.eq_ignore_ascii_case("after")
        || rest.starts_with("After:")
        || rest.starts_with("After ")
}

/// Collect SQL lines until we hit another annotation or end.
///
/// Returns `(sql, after_sql, next_line_index)`.
fn collect_sql_section(
    lines: &[&str],
    start: usize,
) -> (String, Option<String>, usize) {
    let mut sql_lines = Vec::new();
    let mut idx = start;

    while idx < lines.len() {
        let line = lines[idx].trim();
        if is_annotation_line(line) || is_after_marker(line) {
            break;
        }
        sql_lines.push(lines[idx]);
        idx += 1;
    }

    let sql = join_sql_lines(&sql_lines);

    let after = if idx < lines.len()
        && is_after_marker(lines[idx].trim())
    {
        idx += 1;
        let mut after_lines = Vec::new();
        while idx < lines.len() {
            let line = lines[idx].trim();
            if is_annotation_line(line) {
                break;
            }
            after_lines.push(lines[idx]);
            idx += 1;
        }
        let after_sql = join_sql_lines(&after_lines);
        if after_sql.is_empty() {
            None
        } else {
            Some(after_sql)
        }
    } else {
        None
    };

    (sql, after, idx)
}

/// Collect the "After" SQL when processing a Before/After pair.
fn collect_after_sql(
    lines: &[&str],
    idx: &mut usize,
) -> Option<String> {
    while *idx < lines.len() {
        let line = lines[*idx].trim();
        if is_after_marker(line) {
            *idx += 1;
            let mut after_lines = Vec::new();
            while *idx < lines.len() {
                let line = lines[*idx].trim();
                if is_annotation_line(line) {
                    break;
                }
                after_lines.push(lines[*idx]);
                *idx += 1;
            }
            let sql = join_sql_lines(&after_lines);
            if sql.is_empty() {
                return None;
            }
            return Some(sql);
        }
        if !line.is_empty() && !line.starts_with("--") {
            break;
        }
        *idx += 1;
    }
    None
}

fn is_annotation_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix("--") else {
        return false;
    };
    let rest = rest.trim();
    rest.starts_with("Positive:")
        || rest.starts_with("Negative:")
        || rest.starts_with("Expected:")
        || rest.starts_with("Expected-Rule:")
        || rest.starts_with("Expected-Plan:")
}

fn join_sql_lines(lines: &[&str]) -> String {
    let joined: String = lines
        .iter()
        .copied()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("--")
        })
        .collect::<Vec<_>>()
        .join("\n");
    joined.trim().to_owned()
}

/// Extract SQL from a block with no structured annotations.
fn extract_bare_sql(block: &str) -> String {
    let lines: Vec<&str> = block
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("--")
        })
        .collect();
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_positive_negative() {
        let block = "\
-- Positive: basic filter pushdown
SELECT * FROM orders WHERE amount > 100;

-- Negative: cannot push through outer join
SELECT * FROM a LEFT JOIN b ON a.id = b.id WHERE b.x = 1;";

        let cases = parse_test_block(block, "test-rule");
        assert_eq!(cases.len(), 2);
        assert_eq!(
            cases[0].expectation,
            TestExpectation::PlanChanged
        );
        assert_eq!(cases[0].label, "basic filter pushdown");
        assert_eq!(
            cases[1].expectation,
            TestExpectation::PlanUnchanged
        );
    }

    #[test]
    fn parse_before_after_pair() {
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

        let cases = parse_test_block(block, "filter-pushdown");
        assert_eq!(cases.len(), 1);
        assert!(matches!(
            cases[0].expectation,
            TestExpectation::PlanMatchesSql(_)
        ));
    }

    #[test]
    fn parse_bare_sql() {
        let block = "SELECT * FROM users WHERE age > 18;";
        let cases = parse_test_block(block, "rule-x");
        assert_eq!(cases.len(), 1);
        assert_eq!(
            cases[0].expectation,
            TestExpectation::NoError
        );
    }

    #[test]
    fn parse_expected_rule() {
        let block = "\
-- Expected-Rule: filter-through-join
SELECT * FROM orders o JOIN items i ON o.id = i.oid
WHERE o.status = 'shipped';";

        let cases =
            parse_test_block(block, "filter-through-join");
        assert_eq!(cases.len(), 1);
        assert_eq!(
            cases[0].expectation,
            TestExpectation::RuleApplied(
                "filter-through-join".to_owned()
            )
        );
    }

    #[test]
    fn empty_block_yields_no_cases() {
        let cases = parse_test_block("", "r");
        assert!(cases.is_empty());
    }

    #[test]
    fn comments_only_yields_no_cases() {
        let block = "-- just a comment\n-- another comment";
        let cases = parse_test_block(block, "r");
        assert!(cases.is_empty());
    }

    #[test]
    fn parse_expected_annotation() {
        let block = "\
-- Expected: uses hardware-accelerated operator
SELECT * FROM sensors WHERE reading > 42.0;";

        let cases = parse_test_block(block, "hw-rule");
        assert_eq!(cases.len(), 1);
        assert_eq!(
            cases[0].expectation,
            TestExpectation::PlanChanged
        );
        assert_eq!(
            cases[0].label,
            "uses hardware-accelerated operator"
        );
    }
}
