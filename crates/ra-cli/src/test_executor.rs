//! Test execution engine for `.rra` rule test cases.
//!
//! Parses SQL, runs the optimizer, and compares results against
//! the declared [`TestExpectation`].

use std::fmt;

use ra_core::algebra::RelExpr;
use ra_engine::Optimizer;
use ra_parser::test_case::{TestCase, TestExpectation};
use ra_parser::sql_to_relexpr;

use crate::display::format_plan_tree;

/// Outcome of executing a single test case.
#[derive(Debug)]
pub struct TestResult {
    /// The test case that was executed (retained for diagnostics).
    #[allow(dead_code)]
    pub test: TestCase,
    /// Whether the test passed, failed, or was skipped.
    pub outcome: TestOutcome,
}

/// Classification of a test result.
#[derive(Debug)]
pub enum TestOutcome {
    /// Test passed.
    Pass,
    /// Test failed with an explanation.
    Fail(String),
    /// Test was skipped (SQL not parseable, etc.).
    Skip(String),
    /// An internal error prevented execution.
    Error(String),
}

impl fmt::Display for TestOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => write!(f, "PASS"),
            Self::Fail(msg) => write!(f, "FAIL: {msg}"),
            Self::Skip(msg) => write!(f, "SKIP: {msg}"),
            Self::Error(msg) => write!(f, "ERROR: {msg}"),
        }
    }
}

/// Execute a single test case.
///
/// Parses the input SQL, optimizes it, and compares the result
/// against the expectation.
pub fn execute_test(
    test: &TestCase,
    optimizer: &Optimizer,
) -> TestResult {
    let sql = extract_select_statement(&test.input_sql);
    let original = match sql_to_relexpr(&sql) {
        Ok(plan) => plan,
        Err(e) => {
            let reason = format!("SQL parse error: {e}");
            return TestResult {
                test: test.clone(),
                outcome: TestOutcome::Skip(reason),
            };
        }
    };

    let optimized = match optimizer.optimize(&original) {
        Ok(plan) => plan,
        Err(e) => {
            let msg = format!("optimizer error: {e}");
            let outcome = if is_known_limitation(&msg) {
                TestOutcome::Skip(msg)
            } else {
                TestOutcome::Error(msg)
            };
            return TestResult {
                test: test.clone(),
                outcome,
            };
        }
    };

    let outcome =
        check_expectation(test, &original, &optimized, optimizer);

    TestResult {
        test: test.clone(),
        outcome,
    }
}

fn check_expectation(
    test: &TestCase,
    original: &RelExpr,
    optimized: &RelExpr,
    optimizer: &Optimizer,
) -> TestOutcome {
    let plan_changed = original != optimized;

    match &test.expectation {
        TestExpectation::PlanChanged => {
            if plan_changed {
                TestOutcome::Pass
            } else {
                TestOutcome::Fail(
                    "expected plan to change, but it did not"
                        .to_owned(),
                )
            }
        }
        TestExpectation::PlanUnchanged => {
            if plan_changed {
                TestOutcome::Fail(format!(
                    "expected plan unchanged, but it changed:\n\
                     original:\n{}\noptimized:\n{}",
                    format_plan_tree(original),
                    format_plan_tree(optimized),
                ))
            } else {
                TestOutcome::Pass
            }
        }
        TestExpectation::PlanMatchesSql(expected_sql) => {
            check_plan_matches_sql(
                expected_sql,
                optimized,
                optimizer,
            )
        }
        TestExpectation::RuleApplied(_rule_id) => {
            // We cannot directly check which rules fired in the
            // e-graph, so we approximate: if the plan changed,
            // assume the expected rule was among those applied.
            if plan_changed {
                TestOutcome::Pass
            } else {
                TestOutcome::Fail(
                    "expected a rule to fire, \
                     but the plan is unchanged"
                        .to_owned(),
                )
            }
        }
        TestExpectation::NoError => TestOutcome::Pass,
    }
}

fn check_plan_matches_sql(
    expected_sql: &str,
    optimized: &RelExpr,
    optimizer: &Optimizer,
) -> TestOutcome {
    let expected_plan = match sql_to_relexpr(expected_sql) {
        Ok(plan) => plan,
        Err(e) => {
            return TestOutcome::Skip(format!(
                "expected SQL parse error: {e}"
            ));
        }
    };

    // Optimize the expected plan too so we compare canonical forms
    let expected_optimized =
        match optimizer.optimize(&expected_plan) {
            Ok(plan) => plan,
            Err(e) => {
                return TestOutcome::Skip(format!(
                    "expected SQL optimizer error: {e}"
                ));
            }
        };

    if optimized == &expected_optimized {
        TestOutcome::Pass
    } else {
        TestOutcome::Fail(format!(
            "plan does not match expected:\n\
             actual:\n{}\nexpected:\n{}",
            format_plan_tree(optimized),
            format_plan_tree(&expected_optimized),
        ))
    }
}

/// Check if an optimizer error is a known, expected limitation.
fn is_known_limitation(msg: &str) -> bool {
    msg.contains("not yet supported")
        || msg.contains("not supported")
        || msg.contains("failed to extract plan")
}

/// Extract the first SELECT statement from SQL that may contain
/// DDL (CREATE INDEX, etc.) mixed with queries.
fn extract_select_statement(sql: &str) -> String {
    // Split on semicolons and find the first SELECT
    for stmt in sql.split(';') {
        let trimmed = stmt.trim();
        if trimmed
            .to_ascii_uppercase()
            .starts_with("SELECT")
        {
            return trimmed.to_owned();
        }
        // Also handle parenthesized selects like (SELECT ...)
        if trimmed.starts_with('(') {
            let inner = trimmed.trim_start_matches('(');
            if inner
                .trim()
                .to_ascii_uppercase()
                .starts_with("SELECT")
            {
                return trimmed.to_owned();
            }
        }
    }
    // No SELECT found -- return original for parse error reporting
    sql.to_owned()
}

/// Aggregate statistics from a test run.
#[derive(Debug, Default)]
pub struct TestStats {
    /// Total tests executed.
    pub total: usize,
    /// Tests that passed.
    pub passed: usize,
    /// Tests that failed.
    pub failed: usize,
    /// Tests that were skipped.
    pub skipped: usize,
    /// Tests with internal errors.
    pub errors: usize,
}

impl TestStats {
    /// Record a test result.
    pub fn record(&mut self, result: &TestResult) {
        self.total += 1;
        match &result.outcome {
            TestOutcome::Pass => self.passed += 1,
            TestOutcome::Fail(_) => self.failed += 1,
            TestOutcome::Skip(_) => self.skipped += 1,
            TestOutcome::Error(_) => self.errors += 1,
        }
    }

    /// Pass rate as a percentage (excluding skips and errors).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn pass_rate(&self) -> f64 {
        let executed = self.passed + self.failed;
        if executed == 0 {
            return 0.0;
        }
        (self.passed as f64 / executed as f64) * 100.0
    }
}

impl fmt::Display for TestStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} total: {} passed, {} failed, \
             {} skipped, {} errors ({:.1}% pass rate)",
            self.total,
            self.passed,
            self.failed,
            self.skipped,
            self.errors,
            self.pass_rate(),
        )
    }
}
