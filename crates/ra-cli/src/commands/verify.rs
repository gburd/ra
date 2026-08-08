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
    file: Option<&Path>,
    format: &str,
    quiet: bool,
    db: Option<&str>,
) -> Result<bool> {
    if tier > 1 {
        bail!(
            "tier {tier} is not wired yet. Tiers 0 (the 120-query \
             qualification corpus) and 1 (PostgreSQL src/test/regress, the \
             multi-statement result oracle) run today. Tiers 2-4 \
             (sqllogictest, workload results, differential fuzzing) are \
             tracked in the issue tracker (RA-STEERING §5.3, §6 Gate 1)."
        );
    }

    // Tier 1 (PostgreSQL src/test/regress) is a differential result oracle
    // over multi-statement files and *requires* --db + --corpus (the regress
    // .sql dir is not in the repo).
    if tier == 1 {
        let url = db.ok_or_else(|| {
            anyhow::anyhow!(
                "tier 1 (the PostgreSQL src/test/regress result oracle) requires \
                 --db <libpq-url> and the `pg-oracle` build feature \
                 (RA-STEERING §5.3, §6 Gate 1)."
            )
        })?;
        // Internal single-file mode (subprocess isolation).
        if let Some(f) = file {
            return run_tier1_single(f, url);
        }
        let corpus_dir = corpus.ok_or_else(|| {
            anyhow::anyhow!(
                "tier 1 requires --corpus <dir> pointing at the PostgreSQL \
                 src/test/regress .sql files (a flat directory of *.sql). The \
                 regress corpus is not bundled in the repo."
            )
        })?;
        if !corpus_dir.is_dir() {
            bail!(
                "corpus directory not found: {}. Pass --corpus <dir> with the \
                 regress .sql files.",
                corpus_dir.display()
            );
        }
        return run_tier1(corpus_dir, url, format, quiet);
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

/// Dispatch the tier-1 (PostgreSQL src/test/regress) multi-statement result
/// oracle. Gated behind `pg-oracle`.
#[cfg(feature = "pg-oracle")]
fn run_tier1(corpus_dir: &Path, url: &str, format: &str, quiet: bool) -> Result<bool> {
    oracle::run_tier1_oracle(corpus_dir, url, format, quiet)
}

#[cfg(not(feature = "pg-oracle"))]
fn run_tier1(_corpus_dir: &Path, _url: &str, _format: &str, _quiet: bool) -> Result<bool> {
    bail!(
        "tier 1 (the PostgreSQL src/test/regress result oracle) requires the \
         `pg-oracle` build feature. Rebuild with `--features pg-oracle` \
         (RA-STEERING §5.3, Gate 1)."
    );
}

/// Internal single-file tier-1 mode: process one regress file and print its
/// per-file result as JSON (used by the subprocess isolation runner).
#[cfg(feature = "pg-oracle")]
fn run_tier1_single(file: &Path, url: &str) -> Result<bool> {
    oracle::run_tier1_single_file(file, url)
}

#[cfg(not(feature = "pg-oracle"))]
fn run_tier1_single(_file: &Path, _url: &str) -> Result<bool> {
    bail!("tier 1 requires the `pg-oracle` build feature.");
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
    use serde::{Deserialize, Serialize};

    /// Per-query outcome of the differential check.
    #[derive(Serialize, Deserialize)]
    struct QueryOutcome {
        #[serde(default)]
        file: String,
        /// One of: matched, mismatch, parse-skip, emit-fail, setup-skip.
        status: String,
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
                let mut outcome = check_dql(&mut client, &sql);
                outcome.file.clone_from(&file);
                match outcome.status.as_str() {
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

    /// Run the differential DQL check for one read-only statement. Shared by
    /// tier 0 (one query per file) and tier 1 (each SELECT/WITH/VALUES/TABLE
    /// statement in a regress file). The returned `QueryOutcome.file` is left
    /// empty; the caller fills in a file/statement label.
    ///
    /// Fallback (RA-STEERING §5.1, Codeberg #9): `parse-skip` (sql_to_relexpr
    /// or optimize failed) and `emit-fail` (re-emission failed / would not
    /// execute) are exactly the statements the PG extension would hand back to
    /// the native planner.
    fn check_dql(client: &mut Client, sql: &str) -> QueryOutcome {
        // (a) parse original SQL.
        let expr = match sql_to_relexpr(sql) {
            Ok(e) => e,
            Err(e) => {
                return outcome("parse-skip", Some(e.to_string()));
            }
        };
        // (b) optimize.
        let opt = match Optimizer::new().optimize(&expr) {
            Ok(e) => e,
            Err(e) => {
                return outcome("parse-skip", Some(format!("optimize: {e}")));
            }
        };
        // (c) re-emit optimized plan to PostgreSQL SQL.
        let optimized_sql = match emit_sql(&opt, Dialect::PostgreSql) {
            Ok(r) => r.sql,
            Err(e) => {
                return outcome("emit-fail", Some(e.to_string()));
            }
        };

        // (d) execute BOTH sides. A missing table/column (or other executor
        // error on the ORIGINAL side) is a schema-coverage gap, not a
        // wrong-answer — record it as setup-skip.
        let ordered = has_top_level_order_by(sql);
        let original_rows = match fetch_rows(client, sql, ordered) {
            Ok(rows) => rows,
            Err(e) => {
                return outcome("setup-skip", Some(format!("original: {}", pg_msg(&e))));
            }
        };
        // If the ORIGINAL executed but the OPTIMIZED cannot, that is a
        // *re-emission* defect worth surfacing, not a schema gap.
        let optimized_rows = match fetch_rows(client, &optimized_sql, ordered) {
            Ok(rows) => rows,
            Err(e) => {
                return QueryOutcome {
                    file: String::new(),
                    status: "emit-fail".to_string(),
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
            outcome("matched", None)
        } else {
            QueryOutcome {
                file: String::new(),
                status: "mismatch".to_string(),
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

    fn outcome(status: &'static str, detail: Option<String>) -> QueryOutcome {
        QueryOutcome {
            file: String::new(),
            status: status.to_string(),
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

    /// Classify a statement by its leading SQL keyword.
    ///
    /// A statement is *checked DQL* (returns `true`) only when it is read-only
    /// and can be run through the Ra optimize/re-emit/diff oracle: `SELECT`,
    /// `WITH` (CTE query), `VALUES`, or `TABLE name`. Everything else
    /// (`CREATE`/`ALTER`/`DROP`/`INSERT`/`SET`/`BEGIN`/`ANALYZE`/…) is
    /// DDL/utility/DML that advances session state and is executed directly.
    ///
    /// Leading line/block comments and whitespace are skipped. Note a `WITH`
    /// that carries a data-modifying CTE (`WITH x AS (INSERT …) …`) is not
    /// distinguished here — the optimizer/emitter simply fails such input and
    /// it lands in the fallback counter, which is the honest outcome.
    fn is_checked_dql(sql: &str) -> bool {
        let kw = leading_keyword(sql);
        matches!(kw.as_str(), "SELECT" | "WITH" | "VALUES" | "TABLE")
    }

    /// First SQL keyword of a statement, upper-cased, after stripping leading
    /// whitespace, `--` line comments, and `/* … */` block comments.
    fn leading_keyword(sql: &str) -> String {
        let bytes = sql.as_bytes();
        let mut i = 0;
        loop {
            // Skip whitespace.
            while i < bytes.len() && (bytes[i] as char).is_ascii_whitespace() {
                i += 1;
            }
            // Skip a line comment.
            if sql[i..].starts_with("--") {
                match sql[i..].find('\n') {
                    Some(nl) => i += nl + 1,
                    None => return String::new(),
                }
                continue;
            }
            // Skip a block comment.
            if sql[i..].starts_with("/*") {
                match sql[i..].find("*/") {
                    Some(end) => i += end + 2,
                    None => return String::new(),
                }
                continue;
            }
            break;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        sql[start..i].to_ascii_uppercase()
    }

    /// Per-file result of the tier-1 oracle over a regress `.sql` file.
    #[derive(Default, Serialize, Deserialize)]
    struct FileOracle {
        file: String,
        /// Statements found after splitting + meta-line stripping.
        statements: usize,
        ddl_exec: usize,
        dql_checked: usize,
        matched: usize,
        mismatched: usize,
        emit_fail: usize,
        parse_skip: usize,
        /// setup-skip = executor error (missing schema, aborted txn, psql
        /// `:var` substitution left in, volatile function, etc.) on a DQL
        /// statement.
        setup_skip: usize,
        /// DDL/utility statements that errored on PG (recorded, not fatal).
        ddl_errors: usize,
        /// True if the subprocess processing this file crashed (uncatchable
        /// stack overflow in the optimizer/emitter). The file's counts are
        /// whatever was reported before the crash (zero if it never printed).
        #[serde(default)]
        crashed: bool,
        /// Kept for triage: DQL outcomes that carry detail (mismatch / gaps).
        outcomes: Vec<QueryOutcome>,
    }

    #[derive(Serialize)]
    struct Tier1Report {
        mode: &'static str,
        tier: u8,
        corpus: String,
        db: String,
        // Roll-ups.
        files: usize,
        /// Files whose subprocess crashed (uncatchable stack overflow in Ra's
        /// recursive optimizer/emitter on a deeply-nested regress query).
        crashed_files: usize,
        statements: usize,
        ddl_exec: usize,
        ddl_errors: usize,
        dql_checked: usize,
        matched: usize,
        mismatched: usize,
        emit_fail: usize,
        parse_skip: usize,
        setup_skip: usize,
        /// RA-STEERING §5.1 / Codeberg #9: statements Ra could NOT plan
        /// end-to-end (= parse_skip + emit_fail). Exactly what the PG extension
        /// would hand back to the native planner.
        fallback: usize,
        /// fallback / (dql statements Ra attempted end-to-end). "Attempted" =
        /// every DQL statement whose original SQL executed on PG *or* that
        /// fell into fallback before execution — i.e. matched + mismatch +
        /// fallback. setup-skip (couldn't even run the original) is excluded
        /// from the denominator: Ra never got a fair shot there.
        fallback_rate: f64,
        fallback_note: &'static str,
        note: &'static str,
        files_detail: Vec<FileOracle>,
    }

    /// Internal tier-1 mode: process ONE regress file, print its per-file
    /// `FileOracle` as JSON on stdout, exit 0. Invoked as a subprocess by
    /// `run_tier1_oracle` so an uncatchable stack overflow (Ra's recursive
    /// optimizer/emitter on a deeply-nested regress query) aborts only this
    /// child, not the whole corpus run.
    pub fn run_tier1_single_file(file: &Path, url: &str) -> Result<bool> {
        let mut client = Client::connect(url, NoTls)
            .with_context(|| format!("connecting to PostgreSQL at {url}"))?;
        let name = file
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Read lossily: some regress files carry non-UTF-8 bytes.
        let text = match std::fs::read(file) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => anyhow::bail!("reading {}: {e}", file.display()),
        };
        let f = run_regress_file(&mut client, &name, &text);
        println!("{}", serde_json::to_string(&f)?);
        Ok(true)
    }

    /// Tier 1: run the differential result oracle over PostgreSQL
    /// src/test/regress `.sql` files (RA-STEERING §5.3). Each file is a
    /// mixed DDL+DQL script; statements run IN ORDER in a single per-file
    /// transaction that is rolled back afterwards so files stay isolated.
    ///
    /// Each file is processed in its OWN subprocess (`--file`): a handful of
    /// deeply-nested regress queries overflow Ra's recursive optimizer/emitter
    /// stack — an uncatchable abort. Isolating each file in a child process
    /// lets the harness survive those and report the file as `crashed` instead
    /// of dying. Returns `Ok(false)` if any wrong-answer mismatch.
    pub fn run_tier1_oracle(
        corpus_dir: &Path,
        url: &str,
        format: &str,
        quiet: bool,
    ) -> Result<bool> {
        let mut sql_files: Vec<PathBuf> = std::fs::read_dir(corpus_dir)?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sql"))
            .collect();
        sql_files.sort();

        let exe = std::env::current_exe().context("locating the ra binary for subprocesses")?;
        let mut files_detail: Vec<FileOracle> = Vec::with_capacity(sql_files.len());
        for path in &sql_files {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let out = std::process::Command::new(&exe)
                .arg("verify")
                .arg("--tier")
                .arg("1")
                .arg("--db")
                .arg(url)
                .arg("--file")
                .arg(path)
                .arg("--format")
                .arg("json")
                .stderr(std::process::Stdio::null())
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    // The child prints exactly one JSON line (its FileOracle).
                    let line = String::from_utf8_lossy(&o.stdout);
                    match serde_json::from_str::<FileOracle>(line.trim()) {
                        Ok(fo) => files_detail.push(fo),
                        Err(_) => files_detail.push(crashed_file(&name)),
                    }
                }
                // Non-zero exit or spawn failure = the child aborted (stack
                // overflow) or errored: record the file as crashed.
                _ => files_detail.push(crashed_file(&name)),
            }
        }

        let sum = |f: fn(&FileOracle) -> usize| files_detail.iter().map(f).sum::<usize>();
        let parse_skip = sum(|f| f.parse_skip);
        let emit_fail = sum(|f| f.emit_fail);
        let matched = sum(|f| f.matched);
        let mismatched = sum(|f| f.mismatched);
        let fallback = parse_skip + emit_fail;
        // Denominator: DQL statements Ra actually attempted end-to-end.
        let attempted = matched + mismatched + fallback;
        let fallback_rate = if attempted == 0 {
            0.0
        } else {
            fallback as f64 / attempted as f64
        };

        let report = Tier1Report {
            mode: "differential-result-oracle",
            tier: 1,
            corpus: corpus_dir.display().to_string(),
            db: redact(url),
            files: files_detail.len(),
            crashed_files: files_detail.iter().filter(|f| f.crashed).count(),
            statements: sum(|f| f.statements),
            ddl_exec: sum(|f| f.ddl_exec),
            ddl_errors: sum(|f| f.ddl_errors),
            dql_checked: sum(|f| f.dql_checked),
            matched,
            mismatched,
            emit_fail,
            parse_skip,
            setup_skip: sum(|f| f.setup_skip),
            fallback,
            fallback_rate,
            fallback_note: "fallback = parse-skip + emit-fail: DQL statements Ra \
                            could NOT plan end-to-end (would fall back to the \
                            native planner). Rate = fallback / (matched + \
                            mismatch + fallback). RA-STEERING §5.1, Codeberg #9. \
                            The PG-extension in-process counter is separate and \
                            blocked on pgrx.",
            note: "row multisets compared (ordered when the original SQL has a \
                   top-level ORDER BY). Each file runs in its own transaction \
                   (BEGIN/ROLLBACK); each statement runs inside a SAVEPOINT so \
                   a failed/erroring statement rolls back to the savepoint and \
                   the rest of the file's DQL still runs. Volatile / \
                   side-effecting queries (advisory locks, sequences, random, \
                   timestamps) are excluded as setup-skip (§2 follow-up).",
            files_detail,
        };

        if format == "json" {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_tier1_report(&report, quiet);
        }

        Ok(report.mismatched == 0)
    }

    /// A `FileOracle` marker for a file whose subprocess crashed.
    fn crashed_file(name: &str) -> FileOracle {
        FileOracle {
            file: name.to_string(),
            crashed: true,
            ..Default::default()
        }
    }

    /// Run one regress file: split into statements, then execute in order in
    /// a single per-file transaction that is rolled back at the end (so no
    /// file's DDL leaks into the next).
    ///
    /// Each statement runs inside its own SAVEPOINT: a failure rolls back to
    /// the savepoint and keeps the transaction alive, so one bad/intentionally
    /// erroring statement does not poison the rest of the file's checked DQL
    /// (regress files are full of statements that error on purpose or depend
    /// on cross-file schema / psql `:var` substitution we do not perform).
    fn run_regress_file(client: &mut Client, name: &str, text: &str) -> FileOracle {
        let mut f = FileOracle {
            file: name.to_string(),
            ..Default::default()
        };

        let cleaned = strip_meta_lines(text);
        let statements = ra_oracle::split_statements(&cleaned).unwrap_or_default();
        f.statements = statements.len();

        // Isolate the file: everything runs in one transaction, rolled back at
        // the end. A failed BEGIN means the connection is unusable for this
        // file; record nothing and move on.
        if client.batch_execute("BEGIN").is_err() {
            return f;
        }

        for stmt in &statements {
            if is_checked_dql(stmt) {
                // A statement that calls a volatile / side-effecting function
                // (advisory locks, sequence bumps, random, timestamps) cannot
                // be run twice (original then optimized) and compared — the
                // second call sees mutated state. This is the §2
                // "volatile-function handling" follow-up: record as setup-skip
                // rather than a false-positive mismatch.
                if calls_volatile(stmt) {
                    f.setup_skip += 1;
                    continue;
                }
                let _ = client.batch_execute("SAVEPOINT ra_sp");
                let mut outcome = check_dql(client, stmt);
                outcome.file = format!("{name}: {}", stmt_label(stmt));
                match outcome.status.as_str() {
                    "matched" => {
                        f.dql_checked += 1;
                        f.matched += 1;
                    }
                    "mismatch" => {
                        f.dql_checked += 1;
                        f.mismatched += 1;
                        f.outcomes.push(outcome);
                    }
                    "emit-fail" => {
                        f.emit_fail += 1;
                        f.outcomes.push(outcome);
                    }
                    "parse-skip" => {
                        f.parse_skip += 1;
                        f.outcomes.push(outcome);
                    }
                    _ => f.setup_skip += 1, // setup-skip
                }
                // Recover the txn: the SELECT (original or optimized) may have
                // raised and aborted it. Roll back to the savepoint so the
                // rest of the file's statements can still run.
                let _ = client.batch_execute("ROLLBACK TO SAVEPOINT ra_sp");
                let _ = client.batch_execute("RELEASE SAVEPOINT ra_sp");
            } else {
                // DDL/utility/DML: advance session state. Errors are recorded
                // (regress files sometimes intentionally error) but rolled back
                // to the savepoint so they do not poison the whole file.
                let _ = client.batch_execute("SAVEPOINT ra_sp");
                match client.batch_execute(stmt) {
                    Ok(()) => {
                        f.ddl_exec += 1;
                        let _ = client.batch_execute("RELEASE SAVEPOINT ra_sp");
                    }
                    Err(_e) => {
                        f.ddl_errors += 1;
                        let _ = client.batch_execute("ROLLBACK TO SAVEPOINT ra_sp");
                        let _ = client.batch_execute("RELEASE SAVEPOINT ra_sp");
                    }
                }
            }
        }

        // Discard the file's schema/state so files stay isolated.
        let _ = client.batch_execute("ROLLBACK");
        f
    }

    /// Heuristic: does the statement call a volatile / side-effecting function
    /// whose result changes between two executions? Such queries cannot be run
    /// twice (original vs optimized) and multiset-compared. RA-STEERING §2
    /// lists volatile-function handling as an explicit not-yet-checked
    /// follow-up; we exclude them from the checked surface rather than report
    /// a false-positive wrong answer. Name-based and deliberately conservative.
    fn calls_volatile(sql: &str) -> bool {
        const VOLATILE: &[&str] = &[
            "PG_ADVISORY",     // advisory lock/unlock (returns t then f)
            "NEXTVAL",         // sequence advance
            "SETVAL",          // sequence set
            "RANDOM(",         // RNG
            "GEN_RANDOM",      // gen_random_uuid / bytes
            "UUID_GENERATE",   // uuid-ossp
            "CLOCK_TIMESTAMP", // wall-clock (moves between calls)
            "STATEMENT_TIMESTAMP",
            "TIMEOFDAY",
            "NOW(",         // transaction timestamp — but compared against
            "CURRENT_TIME", // volatile time-of-day forms differ across calls
            "LOCALTIME",
            "PG_SLEEP",
            "CURRVAL",      // depends on session nextval state
            "LASTVAL",      // ditto
            "TXID_CURRENT", // transaction id, and pg_current_xact_id
            "PG_CURRENT_XACT_ID",
            "PG_TRY_ADVISORY",
            // Index-maintenance functions that DO work on first call and
            // report zero on the second (side effect, not a wrong answer).
            "BRIN_SUMMARIZE",
            "BRIN_DESUMMARIZE",
            "GIN_CLEAN_PENDING_LIST",
            // Catalog views that reflect mutating session state (prepared
            // statement names change between the two executions).
            "PG_PREPARED_STATEMENTS",
            "PG_STAT", // cumulative statistics move between calls
        ];
        let up = sql.to_ascii_uppercase();
        VOLATILE.iter().any(|v| up.contains(v))
    }

    /// A short one-line label for a statement (first ~80 chars, single line).
    fn stmt_label(sql: &str) -> String {
        let one = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        if one.len() > 80 {
            format!("{}…", &one[..80])
        } else {
            one
        }
    }

    /// Remove psql meta-command lines (lines whose first non-space char is
    /// `\`, e.g. `\d`, `\gset`, `\copy`, `\.`). These are not SQL and confuse
    /// the scanner. Mirrors the batch corpus preprocessing.
    fn strip_meta_lines(text: &str) -> String {
        text.lines()
            .filter(|l| !l.trim_start().starts_with('\\'))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn print_tier1_report(r: &Tier1Report, quiet: bool) {
        if !quiet {
            println!("Tier 1 differential result oracle — corpus: {}", r.corpus);
            println!("Database: {}", r.db);
            println!("{}\n", r.note);
        }
        println!("files scanned:        {}", r.files);
        println!(
            "  crashed (optimizer/emitter stack overflow, isolated): {}",
            r.crashed_files
        );
        println!("statements considered:{:>7}", r.statements);
        println!(
            "DDL/utility executed: {:>7}  (errored on PG: {})",
            r.ddl_exec, r.ddl_errors
        );
        println!("DQL checked:          {:>7}", r.dql_checked);
        println!("  matched:            {:>7}", r.matched);
        println!("  MISMATCH:           {:>7}", r.mismatched);
        println!("  emit-fail:          {:>7}", r.emit_fail);
        println!("  parse-skip:         {:>7}", r.parse_skip);
        println!("  setup-skip:         {:>7}", r.setup_skip);
        println!(
            "FALLBACK (parse-skip+emit-fail): {}   rate: {:.4} ({}/{})",
            r.fallback,
            r.fallback_rate,
            r.fallback,
            r.matched + r.mismatched + r.fallback
        );
        println!("  {}", r.fallback_note);

        // Wrong-answer mismatches — the real Gate-1 defects.
        let mismatches: Vec<&QueryOutcome> = r
            .files_detail
            .iter()
            .flat_map(|f| f.outcomes.iter().filter(|o| o.status == "mismatch"))
            .collect();
        if mismatches.is_empty() {
            println!("\nNo wrong-answer mismatches. ✅");
        } else {
            println!(
                "\n{} WRONG-ANSWER MISMATCH(es) — optimizer changed the result \
                 (highest-priority Gate-1 defects):",
                mismatches.len()
            );
            for o in mismatches.iter().take(25) {
                println!("\n=== {} ===", o.file);
                if let Some(d) = &o.detail {
                    println!("  {d}");
                }
                if let Some(s) = &o.optimized_sql {
                    println!("  optimized SQL: {s}");
                }
                print_rows("  original", o.original_rows.as_ref());
                print_rows("  optimized", o.optimized_rows.as_ref());
            }
            if mismatches.len() > 25 {
                println!("\n… {} more mismatch(es)", mismatches.len() - 25);
            }
        }

        // Top fallback reasons grouped (feeds Codeberg #25).
        print_top_reasons(
            "parse-skip reasons (Ra could not parse/optimize)",
            r,
            "parse-skip",
        );
        print_top_reasons("emit-fail reasons (re-emission gap)", r, "emit-fail");

        println!(
            "\nTier 1 is FAR from complete coverage: this is the measurement \
             harness + honest baseline, not a pass. fallback_rate={:.4} means \
             {}/{} attempted DQL statements would fall back to the native \
             planner.",
            r.fallback_rate,
            r.fallback,
            r.matched + r.mismatched + r.fallback
        );
    }

    /// Group the outcomes of a given status by their normalized first-line
    /// reason and print the top buckets.
    fn print_top_reasons(header: &str, r: &Tier1Report, status: &str) {
        use std::collections::HashMap;
        let mut buckets: HashMap<String, usize> = HashMap::new();
        for f in &r.files_detail {
            for o in f.outcomes.iter().filter(|o| o.status == status) {
                let reason = o
                    .detail
                    .as_deref()
                    .map_or_else(|| "(no detail)".to_string(), normalize_reason);
                *buckets.entry(reason).or_insert(0) += 1;
            }
        }
        if buckets.is_empty() {
            return;
        }
        let mut items: Vec<(String, usize)> = buckets.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        println!("\n{header}:");
        for (reason, n) in items.iter().take(15) {
            println!("  {n:>5}  {reason}");
        }
    }

    /// Reduce an error message to a stable bucket key: first line, cut at the
    /// first quote/paren/digit-bearing token, truncated. Mirrors the batch
    /// corpus reason grouping.
    fn normalize_reason(err: &str) -> String {
        let line = err.lines().next().unwrap_or(err).trim();
        let cut = line
            .char_indices()
            .find(|(_, c)| *c == '\'' || *c == '"' || *c == '(')
            .map_or(line.len(), |(i, _)| i);
        let mut key = line[..cut].trim().to_owned();
        if key.is_empty() {
            line.clone_into(&mut key);
        }
        key.truncate(80);
        key
    }

    #[cfg(test)]
    mod tests {
        use super::{calls_volatile, has_top_level_order_by, is_checked_dql, leading_keyword};

        #[test]
        fn classifies_leading_keyword() {
            assert_eq!(leading_keyword("SELECT 1"), "SELECT");
            assert_eq!(leading_keyword("  select 1"), "SELECT");
            assert_eq!(leading_keyword("-- c\nCREATE TABLE t(a int)"), "CREATE");
            assert_eq!(
                leading_keyword("/* x */ INSERT INTO t VALUES (1)"),
                "INSERT"
            );
            assert_eq!(
                leading_keyword("\n\n\tWITH a AS (SELECT 1) SELECT * FROM a"),
                "WITH"
            );
            assert_eq!(leading_keyword("   "), "");
        }

        #[test]
        fn dql_vs_ddl_classification() {
            // Checked DQL surface.
            assert!(is_checked_dql("SELECT a FROM t"));
            assert!(is_checked_dql("  select 1"));
            assert!(is_checked_dql("WITH a AS (SELECT 1) SELECT * FROM a"));
            assert!(is_checked_dql("VALUES (1),(2)"));
            assert!(is_checked_dql("TABLE foo"));
            assert!(is_checked_dql("-- comment\nSELECT 1"));
            // DDL / utility / DML — executed, not checked.
            for ddl in [
                "CREATE TABLE t(a int)",
                "ALTER TABLE t ADD b int",
                "DROP TABLE t",
                "INSERT INTO t VALUES (1)",
                "UPDATE t SET a = 1",
                "DELETE FROM t",
                "SET work_mem = '1MB'",
                "RESET work_mem",
                "BEGIN",
                "COMMIT",
                "ROLLBACK",
                "TRUNCATE t",
                "ANALYZE t",
                "VACUUM t",
                "GRANT SELECT ON t TO r",
                "COMMENT ON TABLE t IS 'x'",
                "COPY t FROM stdin",
                "PREPARE p AS SELECT 1",
            ] {
                assert!(!is_checked_dql(ddl), "should be DDL/utility: {ddl}");
            }
        }

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

        #[test]
        fn detects_volatile_side_effecting() {
            assert!(calls_volatile("SELECT pg_advisory_unlock(1)"));
            assert!(calls_volatile("SELECT nextval('s')"));
            assert!(calls_volatile("SELECT brin_summarize_range('i', 2)"));
            assert!(calls_volatile("select gin_clean_pending_list('idx')"));
            assert!(calls_volatile("SELECT name FROM pg_prepared_statements"));
            assert!(calls_volatile(
                "SELECT now()::timetz = current_time::timetz"
            ));
            assert!(!calls_volatile("SELECT a FROM t WHERE a > 1"));
            assert!(!calls_volatile("SELECT count(*) FROM t"));
        }

        // Integration test: run tier 1 over a tiny synthetic 2-file corpus
        // (CREATE TABLE + SELECT). Requires a reachable PostgreSQL; the URL
        // comes from RA_ORACLE_DB. Skipped (not failed) when unset so the
        // default `cargo test` run does not require a database.
        #[cfg(feature = "pg-oracle")]
        #[test]
        fn tier1_synthetic_corpus_matches() {
            let Ok(url) = std::env::var("RA_ORACLE_DB") else {
                eprintln!("skipping tier1_synthetic_corpus_matches: RA_ORACLE_DB unset");
                return;
            };
            let dir = std::env::temp_dir().join(format!("ra_t1_syn_{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            std::fs::write(
                dir.join("a.sql"),
                "CREATE TABLE ra_t1_syn(a int, b text);\n\
                 INSERT INTO ra_t1_syn VALUES (1,'x'),(2,'y'),(3,'z');\n\
                 SELECT a, b FROM ra_t1_syn WHERE a > 1 ORDER BY a;\n",
            )
            .expect("write a.sql");
            std::fs::write(
                dir.join("b.sql"),
                "CREATE TABLE ra_t1_syn2(x int);\n\
                 INSERT INTO ra_t1_syn2 VALUES (10),(20);\n\
                 SELECT x FROM ra_t1_syn2 WHERE x = 10;\n",
            )
            .expect("write b.sql");

            let mut total_matched = 0usize;
            let mut total_mismatch = 0usize;
            for name in ["a.sql", "b.sql"] {
                let mut client = postgres::Client::connect(&url, postgres::NoTls)
                    .expect("connect to RA_ORACLE_DB");
                let text = std::fs::read_to_string(dir.join(name)).expect("read synthetic file");
                let f = super::run_regress_file(&mut client, name, &text);
                total_matched += f.matched;
                total_mismatch += f.mismatched;
            }
            let _ = std::fs::remove_dir_all(&dir);
            assert!(
                total_matched >= 1,
                "expected at least one matched DQL, got matched={total_matched}"
            );
            assert_eq!(total_mismatch, 0, "synthetic corpus should not mismatch");
        }
    }
}
