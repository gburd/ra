//! Ra vs PostgreSQL planner comparison harness.
//!
//! Executes each query twice against a live PostgreSQL instance:
//! 1. With `ra_planner.enabled = on` (Ra path)
//! 2. With `ra_planner.enabled = off` (PG path)
//!
//! Collects timing, IO, and correctness metrics for comparison.

#[cfg(feature = "live-comparison")]
use anyhow::Result;
// `Write` is needed for `stdout().flush()` inside the
// live-comparison-gated `run_comparison`; gate the import the same
// way so it isn't an unused import in the default build.
#[cfg(feature = "live-comparison")]
use std::io::Write;
use serde::{Deserialize, Serialize};

/// Configuration for the comparison run.
pub struct CompareConfig {
    /// PostgreSQL connection string (must have ra_planner extension installed).
    pub db_url: String,
    /// Number of repetitions per query for statistical stability.
    pub repetitions: usize,
    /// Whether to compare result sets for correctness.
    pub verify_results: bool,
}

/// Metrics for a single execution path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathMetrics {
    /// Planning time in milliseconds (from EXPLAIN ANALYZE).
    pub plan_ms: f64,
    /// Execution time in milliseconds (from EXPLAIN ANALYZE).
    pub exec_ms: f64,
    /// Shared buffers hit.
    pub shared_hit_blocks: i64,
    /// Shared buffers read from disk.
    pub shared_read_blocks: i64,
    /// Total rows returned.
    pub rows_returned: i64,
}

/// Result of comparing Ra vs PG for a single query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    /// The SQL query.
    pub sql: String,
    /// Category tag.
    pub category: String,
    /// Ra path metrics (averaged over repetitions). None if Ra failed.
    pub ra_metrics: Option<PathMetrics>,
    /// PG path metrics (averaged over repetitions).
    pub pg_metrics: Option<PathMetrics>,
    /// Whether Ra produced correct results (matches PG output).
    pub results_correct: Option<bool>,
    /// Error message if Ra path failed.
    pub ra_error: Option<String>,
    /// Planning speedup: pg_plan_ms / ra_plan_ms (>1 means Ra is faster).
    pub plan_speedup: Option<f64>,
    /// Execution speedup: pg_exec_ms / ra_exec_ms (>1 means Ra plan executes faster).
    pub exec_speedup: Option<f64>,
}

/// Full comparison report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareReport {
    /// Per-query results.
    pub results: Vec<CompareResult>,
    /// Summary statistics.
    pub summary: CompareSummary,
}

/// Aggregate summary of the comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareSummary {
    /// Total queries attempted.
    pub total_queries: usize,
    /// Queries where Ra produced a plan.
    pub ra_succeeded: usize,
    /// Queries where Ra results matched PG results.
    pub results_correct: usize,
    /// Queries where results were checked.
    pub results_checked: usize,
    /// Median planning speedup (Ra vs PG).
    pub median_plan_speedup: f64,
    /// Median execution speedup (Ra plan vs PG plan).
    pub median_exec_speedup: f64,
    /// Queries where Ra plan was faster to execute.
    pub ra_exec_faster_count: usize,
    /// Queries where PG plan was faster to execute.
    pub pg_exec_faster_count: usize,
}

/// Run the full comparison across all queries.
#[cfg(feature = "live-comparison")]
pub fn run_comparison(
    queries: &[(String, String)],
    config: &CompareConfig,
) -> Result<CompareReport> {
    use postgres::{Client, NoTls};

    let mut client = Client::connect(&config.db_url, NoTls)?;

    // Verify extension is available
    client.execute("SELECT 1", &[])?;

    let mut results = Vec::with_capacity(queries.len());

    for (i, (category, sql)) in queries.iter().enumerate() {
        if i % 10 == 0 {
            print!("\r  Comparing {}/{}", i, queries.len());
            std::io::stdout().flush().ok();
        }

        let result = compare_single_query(&mut client, sql, category, config);
        results.push(result);
    }
    println!("\r  Compared {}/{} queries", queries.len(), queries.len());

    let summary = compute_summary(&results);
    Ok(CompareReport { results, summary })
}

/// Compare a single query: Ra path vs PG path.
#[cfg(feature = "live-comparison")]
fn compare_single_query(
    client: &mut postgres::Client,
    sql: &str,
    category: &str,
    config: &CompareConfig,
) -> CompareResult {
    // Run PG path
    let pg_metrics = run_with_planner(client, sql, false, config.repetitions);

    // Run Ra path
    let ra_metrics = run_with_planner(client, sql, true, config.repetitions);

    // Verify correctness if requested
    let results_correct = if config.verify_results {
        verify_results(client, sql)
    } else {
        None
    };

    let ra_error = if ra_metrics.is_none() {
        Some("Ra planner failed or unsupported query".to_string())
    } else {
        None
    };

    let plan_speedup = match (&ra_metrics, &pg_metrics) {
        (Some(ra), Some(pg)) if ra.plan_ms > 0.0 => Some(pg.plan_ms / ra.plan_ms),
        _ => None,
    };

    let exec_speedup = match (&ra_metrics, &pg_metrics) {
        (Some(ra), Some(pg)) if ra.exec_ms > 0.0 => Some(pg.exec_ms / ra.exec_ms),
        _ => None,
    };

    CompareResult {
        sql: sql.to_string(),
        category: category.to_string(),
        ra_metrics,
        pg_metrics,
        results_correct,
        ra_error,
        plan_speedup,
        exec_speedup,
    }
}

/// Execute a query with Ra enabled or disabled, collecting EXPLAIN ANALYZE metrics.
#[cfg(feature = "live-comparison")]
fn run_with_planner(
    client: &mut postgres::Client,
    sql: &str,
    ra_enabled: bool,
    repetitions: usize,
) -> Option<PathMetrics> {
    let enable_sql = if ra_enabled {
        "SET ra_planner.enabled = on"
    } else {
        "SET ra_planner.enabled = off"
    };

    if client.execute(enable_sql, &[]).is_err() {
        return None;
    }

    let explain_sql = format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {sql}");

    let mut plan_times = Vec::with_capacity(repetitions);
    let mut exec_times = Vec::with_capacity(repetitions);
    let mut shared_hit = 0i64;
    let mut shared_read = 0i64;
    let mut rows = 0i64;

    for _ in 0..repetitions {
        let row = match client.query_one(&explain_sql, &[]) {
            Ok(r) => r,
            Err(_) => return None,
        };

        let json_val: serde_json::Value = match row.try_get(0) {
            Ok(v) => v,
            Err(_) => return None,
        };

        // EXPLAIN JSON output is an array with one element
        let plan = json_val.as_array()
            .and_then(|arr| arr.first())
            .unwrap_or(&json_val);

        if let Some(pt) = plan.get("Planning Time").and_then(|v| v.as_f64()) {
            plan_times.push(pt);
        }
        if let Some(et) = plan.get("Execution Time").and_then(|v| v.as_f64()) {
            exec_times.push(et);
        }

        // Extract buffer info from the top plan node
        if let Some(top_plan) = plan.get("Plan") {
            shared_hit = top_plan
                .get("Shared Hit Blocks")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            shared_read = top_plan
                .get("Shared Read Blocks")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            rows = top_plan
                .get("Actual Rows")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
        }
    }

    if plan_times.is_empty() || exec_times.is_empty() {
        return None;
    }

    // Use median for stability
    plan_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    exec_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_plan = plan_times[plan_times.len() / 2];
    let median_exec = exec_times[exec_times.len() / 2];

    Some(PathMetrics {
        plan_ms: median_plan,
        exec_ms: median_exec,
        shared_hit_blocks: shared_hit,
        shared_read_blocks: shared_read,
        rows_returned: rows,
    })
}

/// Verify that Ra and PG produce the same results.
#[cfg(feature = "live-comparison")]
fn verify_results(client: &mut postgres::Client, sql: &str) -> Option<bool> {
    // Only read-only queries are verifiable by re-execution: DML/DDL mutate
    // state, so running the statement twice (PG path, then Ra path) and
    // comparing is meaningless. Report those as "not verifiable" (None).
    let head = sql.trim_start();
    let verb = head
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    if matches!(
        verb.as_str(),
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "CREATE" | "DROP" | "ALTER" | "TRUNCATE"
    ) {
        return None;
    }

    // Order-insensitive, type-agnostic content digest computed server-side:
    // md5 over the result rows sorted by their text representation. This
    // compares actual result *content* (the previous version only compared row
    // counts, which missed value differences and flagged order-only diffs).
    let body = head.trim_end().trim_end_matches(';');
    let digest_sql =
        format!("SELECT md5(coalesce(string_agg(t::text, ',' ORDER BY t::text), '')) FROM ({body}) t");

    // A query error on either path (e.g. Ra falling back / an unsupported
    // feature) is "not verifiable" (None), NOT a content mismatch — the prior
    // version conflated the two, counting Ra errors as wrong results.
    client.execute("SET ra_planner.enabled = off", &[]).ok()?;
    let pg_digest: String = client.query_one(&digest_sql, &[]).ok()?.try_get(0).ok()?;
    client.execute("SET ra_planner.enabled = on", &[]).ok()?;
    let ra_digest: String = client.query_one(&digest_sql, &[]).ok()?.try_get(0).ok()?;

    Some(pg_digest == ra_digest)
}

/// Compute aggregate summary from individual results.
fn compute_summary(results: &[CompareResult]) -> CompareSummary {
    let total_queries = results.len();
    let ra_succeeded = results.iter().filter(|r| r.ra_metrics.is_some()).count();
    let results_checked = results.iter().filter(|r| r.results_correct.is_some()).count();
    let results_correct = results
        .iter()
        .filter(|r| r.results_correct == Some(true))
        .count();

    let mut plan_speedups: Vec<f64> = results
        .iter()
        .filter_map(|r| r.plan_speedup)
        .collect();
    let mut exec_speedups: Vec<f64> = results
        .iter()
        .filter_map(|r| r.exec_speedup)
        .collect();

    plan_speedups.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    exec_speedups.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_plan_speedup = if plan_speedups.is_empty() {
        1.0
    } else {
        plan_speedups[plan_speedups.len() / 2]
    };

    let median_exec_speedup = if exec_speedups.is_empty() {
        1.0
    } else {
        exec_speedups[exec_speedups.len() / 2]
    };

    let ra_exec_faster_count = exec_speedups.iter().filter(|&&s| s > 1.0).count();
    let pg_exec_faster_count = exec_speedups.iter().filter(|&&s| s < 1.0).count();

    CompareSummary {
        total_queries,
        ra_succeeded,
        results_correct,
        results_checked,
        median_plan_speedup,
        median_exec_speedup,
        ra_exec_faster_count,
        pg_exec_faster_count,
    }
}

/// Print a human-readable summary to stdout.
pub fn print_summary(report: &CompareReport) {
    let s = &report.summary;
    println!("╔══════════════════════════════════════════════╗");
    println!("║   Ra vs PostgreSQL Planner Comparison        ║");
    println!("╠══════════════════════════════════════════════╣");
    println!("║ Total queries:          {:>6}               ║", s.total_queries);
    println!("║ Ra planned successfully: {:>6}               ║", s.ra_succeeded);
    if s.results_checked > 0 {
        println!("║ Results correct:   {:>6}/{:<6}             ║", s.results_correct, s.results_checked);
    }
    println!("╠══════════════════════════════════════════════╣");
    println!("║ Median planning speedup:  {:>6.2}x             ║", s.median_plan_speedup);
    println!("║ Median execution speedup: {:>6.2}x             ║", s.median_exec_speedup);
    println!("║ Ra exec faster:           {:>6}               ║", s.ra_exec_faster_count);
    println!("║ PG exec faster:           {:>6}               ║", s.pg_exec_faster_count);
    println!("╚══════════════════════════════════════════════╝");
}
