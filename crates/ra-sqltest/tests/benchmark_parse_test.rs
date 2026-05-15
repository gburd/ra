//! Exhaustive parse+optimize test for all benchmark SQL files.
//!
//! Ensures every SELECT query in TPC-H, JOB, TPC-DS, and book-queries
//! directories can be parsed and optimized by Ra with zero regressions.

use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use std::fs;
use std::path::Path;

fn parse_and_optimize(sql: &str) -> Result<(), String> {
    let rel = sql_to_relexpr::sql_to_relexpr(sql)
        .map_err(|e| format!("PARSE: {e}"))?;
    let opt = Optimizer::new();
    opt.optimize(&rel).map_err(|e| format!("OPT: {e}"))?;
    Ok(())
}

fn test_sql_dir(dir: &Path) -> (usize, usize, Vec<String>) {
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut errors = Vec::new();

    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map(|x| x == "sql").unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();
    files.sort();

    for file in &files {
        let content = fs::read_to_string(file).unwrap_or_default();
        for raw_stmt in content.split(';') {
            let stmt: String = raw_stmt
                .lines()
                .filter(|l| !l.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            let upper = stmt.to_uppercase();
            let first = upper.split_whitespace().next().unwrap_or("");
            // Skip DDL/DML/utility statements
            if matches!(
                first,
                "CREATE"
                    | "DROP"
                    | "ALTER"
                    | "INSERT"
                    | "UPDATE"
                    | "DELETE"
                    | "SET"
                    | "BEGIN"
                    | "COMMIT"
                    | "ROLLBACK"
                    | "GRANT"
                    | "REVOKE"
                    | "TRUNCATE"
                    | "VACUUM"
                    | "ANALYZE"
                    | "COPY"
                    | "\\"
            ) {
                continue;
            }
            match parse_and_optimize(stmt) {
                Ok(()) => pass += 1,
                Err(e) => {
                    fail += 1;
                    let snippet: String =
                        stmt.chars().take(80).collect();
                    let snippet = snippet.replace('\n', " ");
                    errors.push(format!(
                        "  [{}] {} → {}",
                        file.file_name()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or("?"),
                        snippet,
                        e
                    ));
                }
            }
        }
    }
    (pass, fail, errors)
}

fn base_dir() -> &'static Path {
    // Walk up from the manifest dir to find the workspace root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
}

#[test]
fn tpch_all_queries_parse_and_optimize() {
    let dir = base_dir().join("benchmarks/tpch/queries");
    let (pass, fail, errors) = test_sql_dir(&dir);
    if fail > 0 {
        eprintln!("TPC-H failures ({fail}/{}):", pass + fail);
        for e in &errors {
            eprintln!("{e}");
        }
    }
    assert_eq!(fail, 0, "TPC-H: {fail} queries failed");
}

#[test]
fn job_all_queries_parse_and_optimize() {
    let dir = base_dir().join("benchmarks/job/queries");
    let (pass, fail, errors) = test_sql_dir(&dir);
    if fail > 0 {
        eprintln!("JOB failures ({fail}/{}):", pass + fail);
        for e in &errors {
            eprintln!("{e}");
        }
    }
    assert_eq!(fail, 0, "JOB: {fail} queries failed");
}

#[test]
fn tpcds_all_queries_parse_and_optimize() {
    let dir = base_dir().join("benchmarks/tpcds/queries");
    let (pass, fail, errors) = test_sql_dir(&dir);
    if fail > 0 {
        eprintln!("TPC-DS failures ({fail}/{}):", pass + fail);
        for e in &errors {
            eprintln!("{e}");
        }
    }
    assert_eq!(fail, 0, "TPC-DS: {fail} queries failed");
}

#[test]
fn book_queries_parse_and_optimize() {
    let dir = base_dir().join("tests/book-queries");
    let (pass, fail, errors) = test_sql_dir(&dir);
    if fail > 0 {
        eprintln!("Book query failures ({fail}/{}):", pass + fail);
        for e in &errors {
            eprintln!("{e}");
        }
    }
    assert_eq!(fail, 0, "Book queries: {fail} queries failed");
}
