//! `ra verify` — run the query corpus and report structural pass rates.
//!
//! RA-STEERING §5.3 / §7.2. This checks that Ra *parses and optimizes* every
//! query in a tier without error. It intentionally does NOT check
//! answer-correctness against PostgreSQL — that is the PG-oracle work (§5.2)
//! and requires a live database (`--db`, not yet wired). The distinction is
//! stated in the output so the number is not mistaken for a correctness score.
#![expect(clippy::print_stdout, reason = "CLI output")]

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use serde::Serialize;

/// Built-in Tier-0 corpus, relative to the repo root.
const TIER0_CORPUS: &str = "benchmarks/planner_comparison/queries";

#[derive(Default, Serialize)]
struct CategoryResult {
    category: String,
    total: usize,
    parsed: usize,
    optimized: usize,
    failures: Vec<QueryFailure>,
}

#[derive(Serialize)]
struct QueryFailure {
    file: String,
    stage: &'static str,
    error: String,
}

#[derive(Serialize)]
struct VerifyReport {
    tier: u8,
    corpus: String,
    total: usize,
    parsed: usize,
    optimized: usize,
    /// What this report *does* measure — spelled out so it is not read as a
    /// correctness-vs-PostgreSQL number.
    checks: &'static str,
    categories: Vec<CategoryResult>,
}

/// Runs tier-0 verification. Returns `Ok(true)` when every query passed the
/// structural checks, `Ok(false)` when there were failures (the caller maps
/// that to a non-zero exit code).
pub fn cmd_verify(
    tier: u8,
    report: bool,
    corpus: Option<&Path>,
    format: &str,
    quiet: bool,
) -> Result<bool> {
    if tier != 0 {
        bail!(
            "tier {tier} is not wired yet. Only tier 0 (the 120-query \
             qualification corpus) runs today. Tiers 1-4 (PostgreSQL \
             src/test/regress, sqllogictest, workload results, differential \
             fuzzing) need the PG oracle and are tracked in the issue \
             tracker (RA-STEERING §5.3, §6 Gate 1)."
        );
    }

    let corpus_dir = corpus.map_or_else(|| PathBuf::from(TIER0_CORPUS), Path::to_path_buf);
    if !corpus_dir.is_dir() {
        bail!(
            "corpus directory not found: {}. Run from the repo root or pass \
             --corpus <dir>.",
            corpus_dir.display()
        );
    }

    let report_data = run_tier0(tier, &corpus_dir)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report_data)?);
        return Ok(!has_failures(&report_data));
    }

    print_text(&report_data, report, quiet);
    Ok(!has_failures(&report_data))
}

fn run_tier0(tier: u8, corpus_dir: &Path) -> Result<VerifyReport> {
    let mut categories: Vec<CategoryResult> = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(corpus_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::path);

    for dir in entries {
        let cat_name = dir.file_name().to_string_lossy().into_owned();
        let mut cat = CategoryResult {
            category: cat_name.clone(),
            ..Default::default()
        };

        let mut sql_files: Vec<PathBuf> = std::fs::read_dir(dir.path())?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sql"))
            .collect();
        sql_files.sort();

        for sql_path in sql_files {
            let file = sql_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let sql = std::fs::read_to_string(&sql_path)?;
            cat.total += 1;

            // The "unsupported" category is expected to fail to parse — it
            // documents SQL Ra does not yet accept. Count it separately by
            // not treating a parse failure there as a regression.
            let expect_unsupported = cat_name == "unsupported";

            match sql_to_relexpr(&sql) {
                Ok(expr) => {
                    cat.parsed += 1;
                    match Optimizer::new().optimize(&expr) {
                        Ok(_) => cat.optimized += 1,
                        Err(e) => cat.failures.push(QueryFailure {
                            file: file.clone(),
                            stage: "optimize",
                            error: e.to_string(),
                        }),
                    }
                }
                Err(e) => {
                    if !expect_unsupported {
                        cat.failures.push(QueryFailure {
                            file: file.clone(),
                            stage: "parse",
                            error: e.to_string(),
                        });
                    }
                }
            }
        }
        categories.push(cat);
    }

    let total: usize = categories.iter().map(|c| c.total).sum();
    let parsed: usize = categories.iter().map(|c| c.parsed).sum();
    let optimized: usize = categories.iter().map(|c| c.optimized).sum();

    Ok(VerifyReport {
        tier,
        corpus: corpus_dir.display().to_string(),
        total,
        parsed,
        optimized,
        checks: "structural (parse + optimize without error); \
                 NOT answer-correctness vs PostgreSQL — that needs the PG oracle",
        categories,
    })
}

fn print_text(r: &VerifyReport, report: bool, quiet: bool) {
    if !quiet {
        println!("Tier {} verification — corpus: {}", r.tier, r.corpus);
        println!("Checks: {}\n", r.checks);
        println!(
            "{:<18} {:>6} {:>8} {:>10}",
            "category", "total", "parsed", "optimized"
        );
        println!("{}", "-".repeat(46));
        for c in &r.categories {
            println!(
                "{:<18} {:>6} {:>8} {:>10}",
                c.category, c.total, c.parsed, c.optimized
            );
        }
        println!("{}", "-".repeat(46));
    }

    println!(
        "{:<18} {:>6} {:>8} {:>10}",
        "TOTAL", r.total, r.parsed, r.optimized
    );

    // Failures (always shown — a status report with no failures is not a
    // status report, RA-STEERING §10.7).
    let failures: Vec<_> = r
        .categories
        .iter()
        .flat_map(|c| c.failures.iter().map(move |f| (c.category.as_str(), f)))
        .collect();
    if failures.is_empty() {
        println!("\nNo structural failures.");
    } else {
        println!("\n{} structural failure(s):", failures.len());
        for (cat, f) in &failures {
            println!("  [{}] {} ({}): {}", cat, f.file, f.stage, f.error);
        }
    }

    if report {
        let pct = |n: usize| {
            if r.total == 0 {
                0.0
            } else {
                100.0 * n as f64 / r.total as f64
            }
        };
        println!(
            "\nREADME line (tier {}): parsed {}/{} ({:.1}%), optimized {}/{} ({:.1}%). \
             Harness: `ra verify --tier {} --report`.",
            r.tier,
            r.parsed,
            r.total,
            pct(r.parsed),
            r.optimized,
            r.total,
            pct(r.optimized),
            r.tier,
        );
        println!("Answer-correctness vs PostgreSQL is NOT measured here (PG oracle, §5.2).");
    }
}

fn has_failures(r: &VerifyReport) -> bool {
    r.categories.iter().any(|c| !c.failures.is_empty())
}
