//! Robustness/coverage harness over every `.rra` rule's `## Test Cases`.
//!
//! Each rule file documents example SQL in its `## Test Cases` section. Those
//! examples are illustrative (they reference tables that don't exist and use
//! features Ra may not support), so they cannot be a *differential* correctness
//! harness — there are no expected outputs or schemas. What they *can* enforce
//! is the prime invariant in miniature: **Ra must never panic** while parsing
//! or optimizing real SQL. This harness:
//!
//! - extracts every SQL statement from every rule's `## Test Cases`,
//! - runs `sql_to_relexpr` + `Optimizer::optimize` on each under
//!   `catch_unwind`,
//! - **asserts zero panics** (a panic in the parser/optimizer is a bug),
//! - and reports parse/optimize coverage so regressions in coverage are
//!   visible.
//!
//! A parse *error* (unsupported feature) is fine and counted, not failed.
#![expect(
    clippy::expect_used,
    clippy::print_stdout,
    reason = "test harness: expect for setup, println for the coverage report"
)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use ra_engine::Optimizer;
use ra_parser::rule_file_parser::parse_rule_file;
use ra_parser::sql_to_relexpr;

fn rules_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../rules")
        .canonicalize()
        .expect("rules/ dir should exist relative to the crate")
}

fn collect_rra(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rra(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rra") {
            out.push(path);
        }
    }
}

/// Split a `## Test Cases` SQL block into individual candidate statements.
/// Drops full-line comments and blank lines; splits on `;`.
fn statements(block: &str) -> Vec<String> {
    // Strip whole-line `--` comments first so they don't swallow following
    // statements when we split on ';'.
    let cleaned: String = block
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    cleaned
        .split(';')
        .map(str::trim)
        .filter(|s| {
            let upper = s.to_ascii_uppercase();
            !s.is_empty()
                && ["SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "VALUES", "MERGE", "TABLE"]
                    .iter()
                    .any(|kw| upper.starts_with(kw))
        })
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn rra_test_cases_never_panic_and_report_coverage() {
    let mut files = Vec::new();
    collect_rra(&rules_dir(), &mut files);
    assert!(!files.is_empty(), "expected to find .rra rule files");

    let optimizer = Optimizer::new();
    let (mut total, mut parsed, mut optimized, mut panicked) = (0u32, 0u32, 0u32, 0u32);
    let mut panic_examples: Vec<String> = Vec::new();

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let Ok(rule) = parse_rule_file(&source) else {
            continue;
        };
        for block in &rule.test_cases {
            for sql in statements(block) {
                total += 1;
                let result = catch_unwind(AssertUnwindSafe(|| {
                    let expr = sql_to_relexpr(&sql).ok()?;
                    Some(optimizer.optimize(&expr).is_ok())
                }));
                match result {
                    Ok(Some(opt_ok)) => {
                        parsed += 1;
                        if opt_ok {
                            optimized += 1;
                        }
                    }
                    Ok(None) => { /* parse error — fine, unsupported feature */ }
                    Err(_) => {
                        panicked += 1;
                        if panic_examples.len() < 10 {
                            panic_examples.push(format!(
                                "{}: {}",
                                file.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                                sql.lines().next().unwrap_or("").trim()
                            ));
                        }
                    }
                }
            }
        }
    }

    println!(
        "[rra-testcases] files={} statements={} parsed={} ({:.1}%) optimized={} panicked={}",
        files.len(),
        total,
        parsed,
        100.0 * f64::from(parsed) / f64::from(total.max(1)),
        optimized,
        panicked,
    );

    assert_eq!(
        panicked, 0,
        "Ra panicked on {panicked} documented .rra test-case statement(s); \
         the parser/optimizer must never panic. Examples:\n  {}",
        panic_examples.join("\n  ")
    );
}
