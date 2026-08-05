//! `ra verify` — run the query corpus and report pass rates.
//!
//! RA-STEERING §5.3 / §7.2. Two modes:
//!
//! - **Structural** (default): every query in a tier *parses and optimizes*
//!   without error. It does NOT check answer-correctness.
//! - **Differential result oracle** (`--db <libpq-url>`, requires the
//!   `pg-oracle` build feature, RA-STEERING §5 Gate 1): for each query, run
//!   the ORIGINAL SQL on PostgreSQL (baseline) and run Ra's OPTIMIZED plan —
//!   re-emitted to SQL — on the same PostgreSQL, then compare the row
//!   *multisets*. A divergence means Ra's optimizer changed the answer: a
//!   wrong-answer defect (RA-STEERING §2 result equivalence). This isolates
//!   optimizer correctness bugs.
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
/// checks, `Ok(false)` when there were failures / wrong-answer mismatches (the
/// caller maps that to a non-zero exit code).
pub fn cmd_verify(
    tier: u8,
    report: bool,
    corpus: Option<&Path>,
    format: &str,
    quiet: bool,
    db: Option<&str>,
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

    // Differential result oracle: only when --db is given.
    if let Some(url) = db {
        return run_oracle(&corpus_dir, url, format, quiet);
    }

    let report_data = run_tier0(tier, &corpus_dir)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&report_data)?);
        return Ok(!has_failures(&report_data));
    }

    print_text(&report_data, report, quiet);
    Ok(!has_failures(&report_data))
}

/// Dispatch the differential result oracle. Gated behind `pg-oracle`.
#[cfg(feature = "pg-oracle")]
fn run_oracle(corpus_dir: &Path, url: &str, format: &str, quiet: bool) -> Result<bool> {
    oracle::run(corpus_dir, url, format, quiet)
}

#[cfg(not(feature = "pg-oracle"))]
fn run_oracle(_corpus_dir: &Path, _url: &str, _format: &str, _quiet: bool) -> Result<bool> {
    bail!(
        "--db (the differential result oracle) requires the `pg-oracle` build \
         feature. Rebuild with `--features pg-oracle` (RA-STEERING §5, Gate 1)."
    );
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

/// Differential result oracle (RA-STEERING §5, Gate 1). All PostgreSQL
/// execution lives here, gated behind the `pg-oracle` feature.
#[cfg(feature = "pg-oracle")]
mod oracle {
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use postgres::{Client, NoTls};
    use ra_dialect::dialect::Dialect;
    use ra_dialect::emitter::emit_sql;
    use ra_engine::Optimizer;
    use ra_parser::sql_to_relexpr;
    use serde::Serialize;

    /// Per-query outcome of the differential check.
    #[derive(Serialize)]
    struct QueryOutcome {
        file: String,
        /// One of: matched, mismatch, parse-skip, emit-fail, setup-skip.
        status: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// Present only for a mismatch: the re-emitted optimized SQL and the
        /// two result sets, so the wrong-answer defect is reported verbatim.
        #[serde(skip_serializing_if = "Option::is_none")]
        optimized_sql: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        original_rows: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        optimized_rows: Option<Vec<String>>,
    }

    #[derive(Default, Serialize)]
    struct CatOracle {
        category: String,
        total: usize,
        checked: usize,
        matched: usize,
        mismatched: usize,
        emit_fail: usize,
        setup_skip: usize,
        parse_skip: usize,
        outcomes: Vec<QueryOutcome>,
    }

    #[derive(Serialize)]
    struct OracleReport {
        mode: &'static str,
        corpus: String,
        db: String,
        // Roll-ups.
        total: usize,
        checked: usize,
        matched: usize,
        mismatched: usize,
        emit_fail: usize,
        setup_skip: usize,
        parse_skip: usize,
        note: &'static str,
        followups: Vec<&'static str>,
        categories: Vec<CatOracle>,
    }

    /// Run the oracle over the corpus. Returns `Ok(false)` if any mismatch.
    pub fn run(corpus_dir: &Path, url: &str, format: &str, quiet: bool) -> Result<bool> {
        let mut client = Client::connect(url, NoTls)
            .with_context(|| format!("connecting to PostgreSQL at {url}"))?;

        let mut categories: Vec<CatOracle> = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(corpus_dir)?
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().is_dir())
            .collect();
        entries.sort_by_key(std::fs::DirEntry::path);

        for dir in entries {
            let cat_name = dir.file_name().to_string_lossy().into_owned();
            // The `unsupported` category is expected to fail to parse; skip it
            // entirely — it is not part of the answer-correctness surface.
            if cat_name == "unsupported" {
                continue;
            }
            let mut cat = CatOracle {
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
                let outcome = check_query(&mut client, &file, &sql);
                match outcome.status {
                    "matched" => {
                        cat.checked += 1;
                        cat.matched += 1;
                    }
                    "mismatch" => {
                        cat.checked += 1;
                        cat.mismatched += 1;
                    }
                    "emit-fail" => cat.emit_fail += 1,
                    "setup-skip" => cat.setup_skip += 1,
                    _ => cat.parse_skip += 1,
                }
                cat.outcomes.push(outcome);
            }
            categories.push(cat);
        }

        let report = OracleReport {
            mode: "differential-result-oracle",
            corpus: corpus_dir.display().to_string(),
            db: redact(url),
            total: categories.iter().map(|c| c.total).sum(),
            checked: categories.iter().map(|c| c.checked).sum(),
            matched: categories.iter().map(|c| c.matched).sum(),
            mismatched: categories.iter().map(|c| c.mismatched).sum(),
            emit_fail: categories.iter().map(|c| c.emit_fail).sum(),
            setup_skip: categories.iter().map(|c| c.setup_skip).sum(),
            parse_skip: categories.iter().map(|c| c.parse_skip).sum(),
            note: "row multisets compared (ordered when the original SQL has a \
                   top-level ORDER BY). Follow-ups (§2): type/collation/typmod \
                   equivalence, error-behavior equivalence, volatile-function \
                   handling.",
            followups: vec![
                "type/collation/typmod equivalence",
                "error-behavior equivalence",
                "volatile-function handling",
            ],
            categories,
        };

        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_report(&report, quiet);
        }

        // Non-zero exit if any wrong-answer mismatch.
        Ok(report.mismatched == 0)
    }

    /// Run the differential check for one query.
    fn check_query(client: &mut Client, file: &str, sql: &str) -> QueryOutcome {
        // (a) parse original SQL.
        let expr = match sql_to_relexpr(sql) {
            Ok(e) => e,
            Err(e) => {
                return outcome(file, "parse-skip", Some(e.to_string()));
            }
        };
        // (b) optimize.
        let opt = match Optimizer::new().optimize(&expr) {
            Ok(e) => e,
            Err(e) => {
                return outcome(file, "parse-skip", Some(format!("optimize: {e}")));
            }
        };
        // (c) re-emit optimized plan to PostgreSQL SQL.
        let optimized_sql = match emit_sql(&opt, Dialect::PostgreSql) {
            Ok(r) => r.sql,
            Err(e) => {
                return outcome(file, "emit-fail", Some(e.to_string()));
            }
        };

        // (d) execute BOTH sides. A missing table/column (or other executor
        // error on the ORIGINAL side) is a schema-coverage gap, not a
        // wrong-answer — record it as setup-skip.
        let ordered = has_top_level_order_by(sql);
        let original_rows = match fetch_rows(client, sql, ordered) {
            Ok(rows) => rows,
            Err(e) => {
                return outcome(
                    file,
                    "setup-skip",
                    Some(format!("original: {}", pg_msg(&e))),
                );
            }
        };
        // If the ORIGINAL executed but the OPTIMIZED cannot, that is a
        // *re-emission* defect worth surfacing, not a schema gap.
        let optimized_rows = match fetch_rows(client, &optimized_sql, ordered) {
            Ok(rows) => rows,
            Err(e) => {
                return QueryOutcome {
                    file: file.to_string(),
                    status: "emit-fail",
                    detail: Some(format!("optimized SQL failed to execute: {}", pg_msg(&e))),
                    optimized_sql: Some(optimized_sql),
                    original_rows: None,
                    optimized_rows: None,
                };
            }
        };

        // Compare. `fetch_rows` already sorts when unordered, so a direct
        // Vec equality is a multiset compare in that case and an ordered
        // compare when the query has a top-level ORDER BY.
        if original_rows == optimized_rows {
            outcome(file, "matched", None)
        } else {
            QueryOutcome {
                file: file.to_string(),
                status: "mismatch",
                detail: Some(format!(
                    "row multisets differ: original {} row(s), optimized {} row(s){}",
                    original_rows.len(),
                    optimized_rows.len(),
                    if ordered { " (ordered compare)" } else { "" }
                )),
                optimized_sql: Some(optimized_sql),
                original_rows: Some(original_rows),
                optimized_rows: Some(optimized_rows),
            }
        }
    }

    fn outcome(file: &str, status: &'static str, detail: Option<String>) -> QueryOutcome {
        QueryOutcome {
            file: file.to_string(),
            status,
            detail,
            optimized_sql: None,
            original_rows: None,
            optimized_rows: None,
        }
    }

    /// Execute a query and return each row as its canonical PostgreSQL text
    /// tuple. We wrap the query and cast the whole row to `text`
    /// (`SELECT (_sub.*)::text FROM (<sql>) _sub`) so every column — including
    /// duplicate / unnamed / aggregate columns and any data type — collapses
    /// to one comparable string per row, letting PostgreSQL itself do the
    /// value formatting. When `ordered` is false the rows are sorted so the
    /// `Vec` compare is a multiset compare; when true the DB order is kept.
    fn fetch_rows(client: &mut Client, sql: &str, ordered: bool) -> Result<Vec<String>, DbError> {
        let trimmed = sql.trim().trim_end_matches(';');
        let wrapped = format!("SELECT (_sub.*)::text AS __row FROM ({trimmed}) _sub");
        let pg_rows = client.query(wrapped.as_str(), &[]).map_err(DbError)?;
        let mut out: Vec<String> = Vec::with_capacity(pg_rows.len());
        for row in &pg_rows {
            // A NULL whole-row is impossible for a real tuple; missing text
            // renders as empty string.
            let cell: Option<String> = row.get(0);
            out.push(cell.unwrap_or_default());
        }
        if !ordered {
            out.sort();
        }
        Ok(out)
    }

    /// Detect a top-level `ORDER BY` in the original SQL. Heuristic but
    /// sufficient for the corpus: it strips string/line-comment noise and
    /// looks for `ORDER BY` that is not inside parentheses (so ORDER BY inside
    /// a subquery / window frame does not force an ordered top-level compare).
    fn has_top_level_order_by(sql: &str) -> bool {
        let bytes = sql.as_bytes();
        let up = sql.to_ascii_uppercase();
        let up = up.as_bytes();
        let mut depth: i32 = 0;
        let mut in_str = false;
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if in_str {
                if c == '\'' {
                    in_str = false;
                }
                i += 1;
                continue;
            }
            match c {
                '\'' => in_str = true,
                '(' => depth += 1,
                ')' => depth -= 1,
                'O' | 'o' if depth == 0 && up[i..].starts_with(b"ORDER") => {
                    // Match "ORDER" then whitespace then "BY".
                    let mut j = i + 5;
                    while j < up.len() && (up[j] as char).is_ascii_whitespace() {
                        j += 1;
                    }
                    if up[j..].starts_with(b"BY") {
                        return true;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Redact the password in a libpq URL for reporting.
    fn redact(url: &str) -> String {
        // postgresql://user:pass@host/db -> postgresql://user:***@host/db
        if let Some(at) = url.find('@') {
            if let Some(scheme_end) = url.find("://") {
                let creds = &url[scheme_end + 3..at];
                if let Some(colon) = creds.find(':') {
                    let user = &creds[..colon];
                    return format!("{}://{}:***@{}", &url[..scheme_end], user, &url[at + 1..]);
                }
            }
        }
        url.to_string()
    }

    /// Thin wrapper so `postgres::Error` gets a short display.
    struct DbError(postgres::Error);
    fn pg_msg(e: &DbError) -> String {
        // Prefer the DB error message if present.
        e.0.as_db_error()
            .map_or_else(|| e.0.to_string(), |d| d.message().to_string())
    }

    fn print_report(r: &OracleReport, quiet: bool) {
        if !quiet {
            println!("Differential result oracle — corpus: {}", r.corpus);
            println!("Database: {}", r.db);
            println!("{}\n", r.note);
            println!(
                "{:<16} {:>5} {:>7} {:>7} {:>10} {:>9} {:>10} {:>10}",
                "category",
                "total",
                "checked",
                "matched",
                "MISMATCH",
                "emit-fail",
                "setup-skip",
                "parse-skip"
            );
            println!("{}", "-".repeat(80));
            for c in &r.categories {
                println!(
                    "{:<16} {:>5} {:>7} {:>7} {:>10} {:>9} {:>10} {:>10}",
                    c.category,
                    c.total,
                    c.checked,
                    c.matched,
                    c.mismatched,
                    c.emit_fail,
                    c.setup_skip,
                    c.parse_skip
                );
            }
            println!("{}", "-".repeat(80));
        }
        println!(
            "{:<16} {:>5} {:>7} {:>7} {:>10} {:>9} {:>10} {:>10}",
            "TOTAL",
            r.total,
            r.checked,
            r.matched,
            r.mismatched,
            r.emit_fail,
            r.setup_skip,
            r.parse_skip
        );

        // Mismatches — the wrong-answer defects (RA-STEERING #11) — reported
        // verbatim so they can be triaged directly.
        let mismatches: Vec<(&str, &QueryOutcome)> = r
            .categories
            .iter()
            .flat_map(|c| {
                c.outcomes
                    .iter()
                    .filter(|o| o.status == "mismatch")
                    .map(move |o| (c.category.as_str(), o))
            })
            .collect();
        if mismatches.is_empty() {
            println!("\nNo wrong-answer mismatches. ✅");
        } else {
            println!(
                "\n{} WRONG-ANSWER MISMATCH(es) — optimizer changed the result:",
                mismatches.len()
            );
            for (cat, o) in &mismatches {
                println!("\n=== [{cat}] {} ===", o.file);
                if let Some(d) = &o.detail {
                    println!("  {d}");
                }
                if let Some(s) = &o.optimized_sql {
                    println!("  optimized SQL: {s}");
                }
                print_rows("  original", o.original_rows.as_ref());
                print_rows("  optimized", o.optimized_rows.as_ref());
            }
        }

        // Emit-fail and re-emission-execution failures — real gaps, surfaced
        // but distinct from a wrong answer.
        let emit_fails: Vec<(&str, &QueryOutcome)> = r
            .categories
            .iter()
            .flat_map(|c| {
                c.outcomes
                    .iter()
                    .filter(|o| o.status == "emit-fail")
                    .map(move |o| (c.category.as_str(), o))
            })
            .collect();
        if !emit_fails.is_empty() {
            println!("\n{} emit / re-emission gap(s):", emit_fails.len());
            for (cat, o) in &emit_fails {
                println!(
                    "  [{cat}] {}: {}",
                    o.file,
                    o.detail.as_deref().unwrap_or("(no detail)")
                );
            }
        }

        println!(
            "\nCoverage: {}/{} queries checked end-to-end \
             (setup-skip {} = missing schema/exec on PG, parse-skip {} = Ra could not \
             parse/optimize, emit-fail {} = re-emission gap).",
            r.checked, r.total, r.setup_skip, r.parse_skip, r.emit_fail
        );
        println!(
            "Follow-ups (§2, not yet checked): {}.",
            r.followups.join(", ")
        );
    }

    fn print_rows(label: &str, rows: Option<&Vec<String>>) {
        match rows {
            Some(rs) => {
                println!("{label} ({} row(s)):", rs.len());
                for (i, r) in rs.iter().enumerate() {
                    if i >= 25 {
                        println!("    … {} more row(s)", rs.len() - 25);
                        break;
                    }
                    println!("    {r}");
                }
            }
            None => println!("{label}: (not captured)"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::has_top_level_order_by;

        #[test]
        fn detects_top_level_order_by() {
            assert!(has_top_level_order_by("SELECT a FROM t ORDER BY a"));
            assert!(has_top_level_order_by(
                "SELECT a FROM t ORDER   BY a DESC LIMIT 10"
            ));
            // Nested ORDER BY (inside a subquery) must NOT force ordered compare.
            assert!(!has_top_level_order_by(
                "SELECT a FROM (SELECT a FROM t ORDER BY a) s"
            ));
            // ORDER BY only inside a string literal must be ignored.
            assert!(!has_top_level_order_by("SELECT 'ORDER BY x' FROM t"));
            assert!(!has_top_level_order_by("SELECT a FROM t"));
            // Top-level ORDER BY after a parenthesised UNION arm.
            assert!(has_top_level_order_by(
                "(SELECT a FROM t) UNION (SELECT a FROM u) ORDER BY a"
            ));
        }
    }
}
