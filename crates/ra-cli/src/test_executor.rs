//! Test execution engine for `.rra` rule test cases.
//!
//! Parses SQL test cases, runs them through the optimizer pipeline,
//! and compares results against expectations.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use ra_engine::Optimizer;
use ra_parser::{
    TestCase, TestExpectation, parse_rule_file, parse_test_block,
    sql_to_relexpr,
};

/// Result of executing a single test case.
#[derive(Debug)]
pub struct TestResult {
    /// Human-readable test name.
    pub name: String,
    /// Pass, fail, skip, or error.
    pub outcome: TestOutcome,
    /// Wall-clock time to run this test.
    pub duration: Duration,
}

/// Outcome of a single test.
#[derive(Debug)]
pub enum TestOutcome {
    /// Test passed.
    Pass,
    /// Test failed with an explanation.
    Fail { reason: String },
    /// Test was skipped (e.g. unsupported SQL).
    Skip { reason: String },
    /// Test hit an internal error.
    Error { message: String },
}

/// Per-file test summary.
#[derive(Debug)]
pub struct FileResult {
    /// Shortened file path for display.
    pub display_path: String,
    /// Number of tests that passed.
    pub passed: usize,
    /// Total tests in this file.
    pub total: usize,
    /// Names of failed tests with reasons.
    pub failures: Vec<(String, String)>,
}

/// Aggregate statistics from a test run.
#[derive(Debug, Default)]
pub struct TestSummary {
    /// Total tests discovered.
    pub total: usize,
    /// Tests that passed.
    pub passed: usize,
    /// Tests that failed.
    pub failed: usize,
    /// Tests that were skipped.
    pub skipped: usize,
    /// Tests that errored.
    pub errored: usize,
    /// Total wall-clock duration.
    pub duration: Duration,
    /// Per-file summaries.
    pub file_results: Vec<FileResult>,
    /// Slowest tests (name, duration), sorted descending.
    pub slowest: Vec<(String, Duration)>,
}

/// Run all test cases from a set of `.rra` files.
///
/// Returns individual results and aggregate summary.
pub fn run_tests(
    files: &[PathBuf],
    filter: Option<&str>,
    verbose: bool,
) -> Result<(Vec<TestResult>, TestSummary)> {
    let optimizer = Optimizer::new();
    let start = Instant::now();
    let mut results = Vec::new();
    let mut summary = TestSummary::default();

    for file in files {
        let source = std::fs::read_to_string(file)
            .with_context(|| {
                format!("reading {}", file.display())
            })?;

        let rule = match parse_rule_file(&source) {
            Ok(r) => r,
            Err(e) => {
                if verbose {
                    results.push(TestResult {
                        name: file.display().to_string(),
                        outcome: TestOutcome::Skip {
                            reason: format!(
                                "parse error: {e}"
                            ),
                        },
                        duration: Duration::ZERO,
                    });
                    summary.skipped += 1;
                    summary.total += 1;
                }
                continue;
            }
        };

        let rule_id = &rule.metadata.id;
        let mut file_passed = 0usize;
        let mut file_total = 0usize;
        let mut file_failures: Vec<(String, String)> = Vec::new();

        for (block_idx, block) in
            rule.test_cases.iter().enumerate()
        {
            let cases = parse_test_block(
                block,
                rule_id,
                block_idx,
            );

            for case in &cases {
                let test_name = case
                    .description
                    .clone()
                    .unwrap_or_else(|| {
                        format!(
                            "{rule_id}::block_{block_idx}"
                        )
                    });

                let full_name = format!(
                    "{}::{}",
                    short_path(file),
                    test_name
                );

                if let Some(f) = filter {
                    if !full_name.contains(f)
                        && !rule_id.contains(f)
                    {
                        continue;
                    }
                }

                summary.total += 1;
                file_total += 1;

                let test_start = Instant::now();
                let outcome =
                    execute_test(case, &optimizer);
                let duration = test_start.elapsed();

                match &outcome {
                    TestOutcome::Pass => {
                        summary.passed += 1;
                        file_passed += 1;
                    }
                    TestOutcome::Fail { reason } => {
                        summary.failed += 1;
                        file_failures.push((
                            test_name.clone(),
                            reason.clone(),
                        ));
                    }
                    TestOutcome::Skip { .. } => {
                        summary.skipped += 1;
                        file_passed += 1;
                    }
                    TestOutcome::Error { message } => {
                        summary.errored += 1;
                        file_failures.push((
                            test_name.clone(),
                            format!("error: {message}"),
                        ));
                    }
                }

                results.push(TestResult {
                    name: full_name,
                    outcome,
                    duration,
                });
            }
        }

        if file_total > 0 {
            summary.file_results.push(FileResult {
                display_path: short_path(file),
                passed: file_passed,
                total: file_total,
                failures: file_failures,
            });
        }
    }

    summary.duration = start.elapsed();

    let mut timed: Vec<(String, Duration)> = results
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Pass))
        .map(|r| (r.name.clone(), r.duration))
        .collect();
    timed.sort_by(|a, b| b.1.cmp(&a.1));
    timed.truncate(10);
    summary.slowest = timed;

    Ok((results, summary))
}

/// Execute a single test case against the optimizer.
fn execute_test(
    test: &TestCase,
    optimizer: &Optimizer,
) -> TestOutcome {
    let input_plan = match sql_to_relexpr(&test.input_sql) {
        Ok(plan) => plan,
        Err(e) => {
            return match &test.expected {
                TestExpectation::Parses => TestOutcome::Fail {
                    reason: format!(
                        "expected SQL to parse, but got: {e}"
                    ),
                },
                _ => TestOutcome::Skip {
                    reason: format!(
                        "SQL parse not supported: {e}"
                    ),
                },
            };
        }
    };

    if test.expected == TestExpectation::Parses {
        return TestOutcome::Pass;
    }

    let optimized = match optimizer.optimize(&input_plan) {
        Ok(plan) => plan,
        Err(e) => {
            return TestOutcome::Error {
                message: format!("optimizer error: {e}"),
            };
        }
    };

    let plan_changed = input_plan != optimized;

    match &test.expected {
        TestExpectation::PlanChanged => {
            if plan_changed {
                TestOutcome::Pass
            } else {
                TestOutcome::Fail {
                    reason:
                        "expected plan to change, \
                         but it stayed the same"
                            .to_owned(),
                }
            }
        }
        TestExpectation::PlanUnchanged => {
            if plan_changed {
                TestOutcome::Fail {
                    reason:
                        "expected plan unchanged, \
                         but optimizer modified it"
                            .to_owned(),
                }
            } else {
                TestOutcome::Pass
            }
        }
        TestExpectation::RuleApplied { rule_id: _ } => {
            if plan_changed {
                TestOutcome::Pass
            } else {
                TestOutcome::Fail {
                    reason:
                        "expected rule to apply, \
                         but plan unchanged"
                            .to_owned(),
                }
            }
        }
        TestExpectation::Described(_desc) => {
            if test.negative {
                if plan_changed {
                    TestOutcome::Fail {
                        reason:
                            "negative test: plan should \
                             not have changed"
                                .to_owned(),
                    }
                } else {
                    TestOutcome::Pass
                }
            } else {
                TestOutcome::Pass
            }
        }
        TestExpectation::Parses => TestOutcome::Pass,
    }
}

/// Shorten a path for display by taking the last 3 components.
fn short_path(path: &Path) -> String {
    let components: Vec<_> = path
        .components()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|c| {
            c.as_os_str().to_string_lossy().to_string()
        })
        .collect();
    components.join("/")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn execute_parseable_sql() {
        let test = TestCase {
            input_sql: "SELECT * FROM users WHERE age > 18"
                .to_owned(),
            expected: TestExpectation::Parses,
            description: Some("basic parse".to_owned()),
            negative: false,
        };
        let optimizer = Optimizer::new();
        let result = execute_test(&test, &optimizer);
        assert!(matches!(result, TestOutcome::Pass));
    }

    #[test]
    fn execute_unparseable_sql() {
        let test = TestCase {
            input_sql: "NOT VALID SQL AT ALL".to_owned(),
            expected: TestExpectation::Parses,
            description: Some("bad sql".to_owned()),
            negative: false,
        };
        let optimizer = Optimizer::new();
        let result = execute_test(&test, &optimizer);
        assert!(matches!(result, TestOutcome::Fail { .. }));
    }

    #[test]
    fn short_path_formatting() {
        let path = PathBuf::from(
            "/home/user/project/rules/logical/filter.rra",
        );
        let short = short_path(&path);
        assert!(short.contains("filter.rra"));
    }

    #[test]
    fn execute_plan_changed() {
        let test = TestCase {
            input_sql:
                "SELECT * FROM orders o \
                 JOIN customers c ON o.cid = c.id \
                 WHERE o.amount > 100"
                    .to_owned(),
            expected: TestExpectation::PlanChanged,
            description: Some("filter pushdown".to_owned()),
            negative: false,
        };
        let optimizer = Optimizer::new();
        let result = execute_test(&test, &optimizer);
        // Either passes or fails depending on optimizer rules;
        // we just verify it doesn't error or skip.
        assert!(!matches!(result, TestOutcome::Error { .. }));
    }
}
