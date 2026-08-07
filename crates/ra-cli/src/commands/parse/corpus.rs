//! Corpus/batch mode for `ra parse --compare-pg` (RA-STEERING §5.2).
//!
//! Reads `.sql` files (a whole directory via `--corpus`, or one file via
//! `--sql-file`), splits them into individual statements, runs the PostgreSQL
//! parse oracle on each, and prints a ranked inventory of Ra parser gaps
//! (statements PG-17 accepts but Ra rejects or parses differently).
//!
//! This is a *first-increment* corpus checker: PG regression `.sql` files are
//! messy (psql `\` meta-commands, `COPY ... FROM stdin` data, dollar-quoted
//! bodies, comments). We do a pragmatic split, not a full psql lexer, and skip
//! statements we cannot cleanly isolate — the skip count is reported honestly.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use super::ParseFormat;

/// Aggregate result of a corpus run.
#[derive(Debug, Default, Serialize)]
struct CorpusSummary {
    files_scanned: usize,
    statements_considered: usize,
    fed_to_oracle: usize,
    skipped_non_dql: usize,
    skipped_unterminated: usize,
    equivalent: usize,
    diverged: usize,
    /// Divergence kind -> (count, up to a few example SQL snippets).
    #[serde(serialize_with = "ser_kinds")]
    kinds: BTreeMap<String, KindStat>,
}

#[derive(Debug, Default, Serialize)]
struct KindStat {
    count: usize,
    examples: Vec<String>,
}

fn ser_kinds<S>(m: &BTreeMap<String, KindStat>, s: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut ranked: Vec<(&String, &KindStat)> = m.iter().collect();
    ranked.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(b.0)));
    let mut seq = s.serialize_seq(Some(ranked.len()))?;
    for (k, v) in ranked {
        seq.serialize_element(&serde_json::json!({
            "kind": k,
            "count": v.count,
            "examples": v.examples,
        }))?;
    }
    seq.end()
}

impl CorpusSummary {
    fn record_divergence(&mut self, kind: String, sql: &str) {
        self.diverged += 1;
        let entry = self.kinds.entry(kind).or_default();
        entry.count += 1;
        if entry.examples.len() < 3 {
            entry.examples.push(truncate_sql(sql));
        }
    }
}

fn truncate_sql(sql: &str) -> String {
    let one_line: String = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.len() > 120 {
        format!("{}...", &one_line[..117])
    } else {
        one_line
    }
}

/// Run the parse oracle over a corpus directory (all `*.sql`) or a single file.
pub fn run(corpus: Option<&Path>, sql_file: Option<&Path>, format: ParseFormat) -> Result<()> {
    #![expect(clippy::print_stdout, reason = "CLI output")]
    let files = collect_files(corpus, sql_file)?;
    if files.is_empty() {
        bail!("no .sql files found for corpus run");
    }

    let mut summary = CorpusSummary::default();

    // Ra's parser can panic on some inputs (a defect we catalogue below via
    // catch_unwind). Silence the default panic hook for the run so hundreds of
    // caught panics don't spam stderr; restore it afterward.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = run_inner(&files, &mut summary);
    std::panic::set_hook(prev_hook);
    result?;

    match format {
        ParseFormat::Json => {
            let json = serde_json::to_string_pretty(&summary)?;
            println!("{json}");
        }
        ParseFormat::Text => print_text_summary(&summary),
    }

    if summary.diverged > 0 {
        bail!(
            "{} divergence(s) found across {} statement(s)",
            summary.diverged,
            summary.fed_to_oracle
        );
    }
    Ok(())
}

/// The file-walking + oracle loop, factored out so the panic-hook swap in
/// `run` cleanly brackets it.
fn run_inner(files: &[std::path::PathBuf], summary: &mut CorpusSummary) -> Result<()> {
    for file in files {
        summary.files_scanned += 1;
        // Some regress files are non-UTF8 (e.g. win1252 collate tests). Skip
        // them rather than aborting the whole corpus run.
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let stmts = split_sql_statements(&text);
        for stmt in stmts {
            match classify_statement(&stmt) {
                StmtClass::Dql => {}
                StmtClass::NonDql => {
                    summary.statements_considered += 1;
                    summary.skipped_non_dql += 1;
                    continue;
                }
                StmtClass::Unterminated => {
                    summary.skipped_unterminated += 1;
                    continue;
                }
            }
            summary.statements_considered += 1;
            summary.fed_to_oracle += 1;

            // One bad statement must not abort the whole run. `compare` calls
            // into Ra's parser, which can *panic* (not just Err) on some inputs
            // — that panic is itself a parser defect we want to catalogue, so
            // catch it and record it as a divergence rather than aborting.
            let stmt_ref = &stmt;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ra_oracle::compare(stmt_ref)
            }));
            match result {
                Ok(Ok(cmp)) => {
                    if cmp.is_equivalent() {
                        summary.equivalent += 1;
                    } else {
                        let kind = normalize_divergence(&cmp.divergences);
                        summary.record_divergence(kind, &stmt);
                    }
                }
                Ok(Err(e)) => {
                    summary.record_divergence(
                        format!("oracle-error: {}", first_line(&e.to_string())),
                        &stmt,
                    );
                }
                Err(_) => {
                    summary.record_divergence("Ra-panic: parser panicked".to_owned(), &stmt);
                }
            }
        }
    }
    Ok(())
}

fn print_text_summary(s: &CorpusSummary) {
    #![expect(clippy::print_stdout, reason = "CLI output")]
    println!("=== parse-oracle corpus summary ===");
    println!("files scanned:          {}", s.files_scanned);
    println!("statements considered:  {}", s.statements_considered);
    println!("  fed to oracle:        {}", s.fed_to_oracle);
    println!("  skipped (non-DQL/DML):{}", s.skipped_non_dql);
    println!("  skipped (unterminated):{}", s.skipped_unterminated);
    println!("equivalent:             {}", s.equivalent);
    println!("diverged:               {}", s.diverged);
    println!();
    if s.kinds.is_empty() {
        println!("no divergences.");
        return;
    }
    let mut ranked: Vec<(&String, &KindStat)> = s.kinds.iter().collect();
    ranked.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(b.0)));
    println!("ranked divergence inventory (top 30 by count):");
    for (kind, stat) in ranked.iter().take(30) {
        println!("  {:>5}  {}", stat.count, kind);
        if let Some(ex) = stat.examples.first() {
            println!("         e.g. {ex}");
        }
    }
    if ranked.len() > 30 {
        println!("  ... and {} more kind(s)", ranked.len() - 30);
    }
}

fn collect_files(
    corpus: Option<&Path>,
    sql_file: Option<&Path>,
) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if let Some(f) = sql_file {
        files.push(f.to_path_buf());
    }
    if let Some(dir) = corpus {
        let rd = std::fs::read_dir(dir)
            .with_context(|| format!("reading corpus directory {}", dir.display()))?;
        for entry in rd {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("sql") {
                files.push(path);
            }
        }
        files.sort();
    }
    Ok(files)
}

/// Classification of a split statement.
enum StmtClass {
    /// SELECT/INSERT/UPDATE/DELETE/WITH/VALUES/TABLE — feed to the oracle.
    Dql,
    /// DDL / utility / meta — count as skipped.
    NonDql,
    /// Empty or obviously not a statement.
    Unterminated,
}

fn classify_statement(stmt: &str) -> StmtClass {
    let trimmed = strip_leading_comments(stmt);
    let Some(first) = trimmed.split_whitespace().next() else {
        return StmtClass::Unterminated;
    };
    let kw = first.trim_start_matches('(').to_ascii_uppercase();
    match kw.as_str() {
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "WITH" | "VALUES" | "TABLE" => StmtClass::Dql,
        _ => StmtClass::NonDql,
    }
}

/// Drop leading `--` line comments and blank lines so the first keyword test
/// sees the real statement start.
fn strip_leading_comments(stmt: &str) -> &str {
    let mut rest = stmt.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            // Skip to end of line.
            rest = after.split_once('\n').map_or("", |x| x.1).trim_start();
        } else {
            return rest;
        }
    }
}

fn normalize_divergence(divs: &[String]) -> String {
    // A statement usually has one dominant divergence; key on the first, which
    // for parse failures is "PG parsed but Ra failed: <err>" etc.
    let Some(first) = divs.first() else {
        return "unknown".to_owned();
    };
    if let Some(err) = first.strip_prefix("PG parsed but Ra failed: ") {
        return format!("Ra-failed: {}", normalize_err(err));
    }
    if let Some(err) = first.strip_prefix("Ra parsed but PG failed: ") {
        return format!("PG-failed(PG17): {}", normalize_err(err));
    }
    // Fact divergence: key on the field name (before ':').
    let field = first.split(':').next().unwrap_or(first);
    if field == "tables" && is_case_only_table_diff(first) {
        // PG folds unquoted identifiers to lowercase; Ra preserves case. A
        // table set that differs *only* by case is that normalization gap, not
        // a missing/extra table — bucket it separately so it doesn't drown out
        // real table divergences.
        return "fact-diverge: tables (case-only, PG lowercases idents)".to_owned();
    }
    format!("fact-diverge: {field}")
}

/// True when a `tables: PG {..} vs Ra {..}` divergence differs only by ASCII
/// case (PG's unquoted-identifier lowercasing vs Ra preserving case).
fn is_case_only_table_diff(div: &str) -> bool {
    let Some(rest) = div.strip_prefix("tables: PG ") else {
        return false;
    };
    let Some((pg, ra)) = rest.split_once(" vs Ra ") else {
        return false;
    };
    let norm = |s: &str| -> Vec<String> {
        s.trim_matches(['{', '}'])
            .split(',')
            .filter(|x| !x.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let (pg_n, ra_n) = (norm(pg), norm(ra));
    // Case-only iff sets differ but become equal once lowercased, and neither
    // side is empty (an empty side is a real missing-table gap).
    pg_n == ra_n && !pg_n.is_empty()
}

/// Reduce a parser error message to a stable bucket key: keep the leading
/// phrase up to the first delimiter that carries variable content.
fn normalize_err(err: &str) -> String {
    let line = first_line(err);
    // Cut at the first quote / paren / digit run so token-specific detail
    // (offsets, the offending identifier) does not fragment the buckets.
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

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s).trim()
}

/// Split a `.sql` file into individual statements.
///
/// Strategy: strip psql `\` meta-command lines, then split on `;` at the top
/// level using PostgreSQL's own scanner (`pg_query::split_with_scanner`, which
/// correctly ignores `;` inside single-quoted, dollar-quoted, and comment
/// text). If PG's scanner rejects the input (rare in practice), fall back to a
/// small hand-rolled state machine that tracks `'` and `$tag$` quoting.
pub fn split_sql_statements(text: &str) -> Vec<String> {
    let cleaned = strip_meta_lines(text);

    if let Ok(parts) = ra_oracle::split_statements(&cleaned) {
        return parts;
    }

    fallback_split(&cleaned)
}

/// Remove psql meta-command lines (lines whose first non-space char is `\`).
/// These are not SQL and confuse the scanner (`\d`, `\copy`, `\.` etc.).
fn strip_meta_lines(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('\\'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Hand-rolled fallback splitter: split on top-level `;`, skipping `;` inside
/// single-quoted strings and `$tag$...$tag$` dollar-quoted bodies. Used only
/// when PG's scanner errors. Unterminated trailing text is dropped.
fn fallback_split(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_single = false;
    let mut dollar_tag: Option<String> = None;

    while i < bytes.len() {
        if let Some(tag) = &dollar_tag {
            if text[i..].starts_with(tag.as_str()) {
                i += tag.len();
                dollar_tag = None;
                continue;
            }
            i += 1;
            continue;
        }
        let c = bytes[i] as char;
        if in_single {
            if c == '\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\'' => {
                in_single = true;
                i += 1;
            }
            '$' => {
                if let Some(tag) = scan_dollar_tag(&text[i..]) {
                    let len = tag.len();
                    dollar_tag = Some(tag);
                    i += len;
                } else {
                    i += 1;
                }
            }
            ';' => {
                let stmt = text[start..i].trim();
                if !stmt.is_empty() {
                    out.push(stmt.to_owned());
                }
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    // Trailing text with no closing `;` and no open quote: keep it; otherwise
    // (open string / dollar body) drop it as unterminated.
    if !in_single && dollar_tag.is_none() {
        let tail = text[start..].trim();
        if !tail.is_empty() {
            out.push(tail.to_owned());
        }
    }
    out
}

/// If `s` starts with a dollar-quote open tag (`$$` or `$ident$`), return the
/// tag text (e.g. `$$` or `$body$`); otherwise `None`.
fn scan_dollar_tag(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.first() != Some(&b'$') {
        return None;
    }
    let mut j = 1;
    while j < b.len() {
        let c = b[j] as char;
        if c == '$' {
            return Some(s[..=j].to_owned());
        }
        if c.is_ascii_alphanumeric() || c == '_' {
            j += 1;
        } else {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_simple_statements() {
        let s = split_sql_statements("SELECT 1; SELECT 2;");
        assert_eq!(s, vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn does_not_split_on_semicolon_in_single_quote() {
        let s = split_sql_statements("SELECT 'a;b'; SELECT 2;");
        assert_eq!(s, vec!["SELECT 'a;b'", "SELECT 2"]);
    }

    #[test]
    fn does_not_split_inside_dollar_quote() {
        let s = split_sql_statements(
            "CREATE FUNCTION f() RETURNS int AS $$ SELECT 1; SELECT 2; $$ LANGUAGE sql; SELECT 3;",
        );
        assert_eq!(s.len(), 2);
        assert!(s[0].contains("SELECT 1; SELECT 2;"));
        assert_eq!(s[1], "SELECT 3");
    }

    #[test]
    fn strips_psql_meta_command_lines() {
        let s = split_sql_statements("\\d onek\nSELECT 1; SELECT 2;");
        assert_eq!(s, vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn tagged_dollar_quote_not_split() {
        let s = split_sql_statements("SELECT $body$x;y$body$; SELECT 4;");
        assert_eq!(s.len(), 2);
        assert!(s[0].contains("x;y"));
        assert_eq!(s[1], "SELECT 4");
    }

    #[test]
    fn fallback_splitter_matches_on_dollar_and_quote() {
        // Exercise the hand-rolled fallback directly (independent of PG scanner).
        let s = fallback_split("SELECT 'a;b'; SELECT $$c;d$$; SELECT 3;");
        assert_eq!(s, vec!["SELECT 'a;b'", "SELECT $$c;d$$", "SELECT 3"]);
    }

    #[test]
    fn classify_first_keyword() {
        assert!(matches!(classify_statement("SELECT 1"), StmtClass::Dql));
        assert!(matches!(
            classify_statement("  with x as (select 1) select * from x"),
            StmtClass::Dql
        ));
        assert!(matches!(
            classify_statement("CREATE TABLE t (a int)"),
            StmtClass::NonDql
        ));
        assert!(matches!(
            classify_statement("-- c\nSELECT 1"),
            StmtClass::Dql
        ));
        assert!(matches!(classify_statement("   "), StmtClass::Unterminated));
    }

    #[test]
    fn normalize_err_buckets_ignore_token_detail() {
        let a = normalize_err("syntax error at or near \"USING\"");
        let b = normalize_err("syntax error at or near \"OVERLAPS\"");
        assert_eq!(a, b);
    }

    #[test]
    fn case_only_table_diff_detected() {
        assert!(is_case_only_table_diff(
            "tables: PG {pktable} vs Ra {PKTABLE}"
        ));
        // Real missing table (Ra empty) is NOT case-only.
        assert!(!is_case_only_table_diff("tables: PG {onek} vs Ra {}"));
        // Genuinely different names are NOT case-only.
        assert!(!is_case_only_table_diff("tables: PG {a} vs Ra {b}"));
    }
}
