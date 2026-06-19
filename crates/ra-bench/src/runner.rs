//! Per-query measurement and result collection.

use std::time::Instant;

use ra_engine::Optimizer;
use ra_grammar_fuzzer::scoring::{compute_score, QueryDimensions, ScoringWeights};
#[cfg(feature = "live-comparison")]
use ra_grammar_fuzzer::sql_emitter::SqlEmitter;
use ra_parser::sql_to_relexpr::sql_to_relexpr;

/// Result of running a single query through the benchmark harness.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "fields used for JSON reporting")]
pub struct QueryResult {
    /// The original SQL string.
    pub sql: String,
    /// Category tag from the corpus.
    pub category: String,
    /// Whether Ra successfully parsed and optimized the query.
    pub success: bool,
    /// Error message (if `!success`).
    pub error: Option<String>,
    /// Ra parse time in milliseconds.
    pub parse_ms: f64,
    /// Ra optimization time in milliseconds.
    pub optimize_ms: f64,
    /// Postgres EXPLAIN round-trip time in milliseconds (if measured).
    pub pg_explain_ms: Option<f64>,
    /// Structural similarity score [0,1] vs Postgres plan.
    pub structural_sim: Option<f64>,
    /// Cost ratio: Ra estimated cost / Postgres estimated cost.
    pub cost_ratio: Option<f64>,
    /// Actual execution time from EXPLAIN ANALYZE (ms), if measured.
    pub actual_execution_time_ms: Option<f64>,
    /// Actual rows returned from EXPLAIN ANALYZE, if measured.
    pub actual_rows: Option<u64>,
    /// Estimated rows from EXPLAIN (before execution), if measured.
    pub estimated_rows: Option<u64>,
    /// Composite score in \[0, 1\] (if computed).
    pub score: Option<f64>,
    /// Result correctness verification (if enabled).
    /// `None` = not checked, `Some(true)` = results match, `Some(false)` = mismatch.
    pub results_match: Option<bool>,
    /// Details about result mismatch (if any).
    pub results_mismatch_detail: Option<String>,
}

/// Configuration for the benchmark runner.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Scoring weights.
    pub weights: ScoringWeights,
    /// Postgres connection string for live comparison (if any).
    pub pg_connection: Option<String>,
    /// Whether to verify result correctness against Postgres.
    pub verify_results: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            weights: ScoringWeights::default(),
            pg_connection: None,
            verify_results: false,
        }
    }
}

/// Measures a single `(category, sql)` pair and returns a [`QueryResult`].
pub fn run_query(
    sql: &str,
    category: &str,
    config: &RunnerConfig,
    optimizer: &Optimizer,
) -> QueryResult {
    // --- Parse ---
    let t_parse = Instant::now();
    let parse_result = sql_to_relexpr(sql);
    let parse_ms = t_parse.elapsed().as_secs_f64() * 1000.0;

    let plan = match parse_result {
        Err(e) => {
            return QueryResult {
                sql: sql.to_owned(),
                category: category.to_owned(),
                success: false,
                error: Some(e.to_string()),
                parse_ms,
                optimize_ms: 0.0,
                pg_explain_ms: None,
                structural_sim: None,
                cost_ratio: None,
                actual_execution_time_ms: None,
                actual_rows: None,
                estimated_rows: None,
                score: None,
                results_match: None,
                results_mismatch_detail: None,
            };
        }
        Ok(p) => p,
    };

    // --- Optimize ---
    let t_opt = Instant::now();
    let optimized = optimizer.optimize(&plan).unwrap_or_else(|_| plan.clone());
    let optimize_ms = t_opt.elapsed().as_secs_f64() * 1000.0;

    // --- Optional Postgres comparison ---
    let (pg_explain_ms, structural_sim, cost_ratio) = run_pg_comparison(
        sql, &optimized, &config.pg_connection,
    );

    let dims = QueryDimensions {
        parse_ms,
        optimize_ms,
        pg_explain_ms,
        structural_sim,
        cost_ratio,
        actual_rows_ratio: None,
    };

    let score = if dims.structural_sim.is_some()
        || dims.cost_ratio.is_some()
        || dims.pg_explain_ms.is_some()
    {
        Some(compute_score(&dims, &config.weights))
    } else {
        None
    };

    // --- Optional result verification ---
    let (results_match, results_mismatch_detail) = if config.verify_results {
        verify_result_correctness(sql, &optimized, &config.pg_connection)
    } else {
        (None, None)
    };

    QueryResult {
        sql: sql.to_owned(),
        category: category.to_owned(),
        success: true,
        error: None,
        parse_ms,
        optimize_ms,
        pg_explain_ms,
        structural_sim,
        cost_ratio,
        actual_execution_time_ms: None,
        actual_rows: None,
        estimated_rows: None,
        score,
        results_match,
        results_mismatch_detail,
    }
}

// ---------------------------------------------------------------------------
// Postgres comparison helper (feature-gated)
// ---------------------------------------------------------------------------

/// Attempt a Postgres plan comparison.  Returns `(None, None, None)` when
/// either the feature is disabled or no connection string is configured.
#[expect(unused_variables)]
fn run_pg_comparison(
    sql: &str,
    plan: &ra_core::algebra::RelExpr,
    pg_connection: &Option<String>,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    #[cfg(feature = "live-comparison")]
    {
        if let Some(ref conn_str) = *pg_connection {
            use ra_grammar_fuzzer::reference::ReferenceComparator;
            let cmp = ReferenceComparator::new().with_postgresql(conn_str);
            let t_pg = Instant::now();
            if let Ok(result) = cmp.compare_with_postgresql(sql, plan) {
                return (
                    Some(t_pg.elapsed().as_secs_f64() * 1000.0),
                    Some(result.similarity_score),
                    result.cost_ratio,
                );
            }
        }
    }
    (None, None, None)
}

// ---------------------------------------------------------------------------
// Result correctness verification (feature-gated)
// ---------------------------------------------------------------------------

/// Verify that the optimized plan produces identical results to the original SQL.
///
/// Executes both the original SQL and the SQL emitted from the optimized plan
/// against Postgres, then compares the result sets.
///
/// Returns `(Some(true), None)` if results match, `(Some(false), Some(detail))`
/// if they differ, or `(None, None)` if verification cannot be performed.
#[expect(unused_variables)]
fn verify_result_correctness(
    original_sql: &str,
    optimized_plan: &ra_core::algebra::RelExpr,
    pg_connection: &Option<String>,
) -> (Option<bool>, Option<String>) {
    #[cfg(feature = "live-comparison")]
    {
        if let Some(ref conn_str) = *pg_connection {
            return do_verify_results(original_sql, optimized_plan, conn_str);
        }
    }
    (None, None)
}

/// Inner verification logic (only compiled with live-comparison feature).
#[cfg(feature = "live-comparison")]
fn do_verify_results(
    original_sql: &str,
    optimized_plan: &ra_core::algebra::RelExpr,
    conn_str: &str,
) -> (Option<bool>, Option<String>) {
    use tracing::debug;

    // Emit optimized plan back to SQL
    let emitter = SqlEmitter::new();
    let optimized_sql = emitter.emit(optimized_plan);

    // Skip verification for queries with non-deterministic output
    // that can't be made deterministic (e.g., queries with RANDOM())
    let lower = original_sql.to_lowercase();
    if lower.contains("random()") || lower.contains("now()") || lower.contains("current_timestamp") {
        return (None, None);
    }

    let mut client = match postgres::Client::connect(conn_str, postgres::NoTls) {
        Ok(c) => c,
        Err(e) => {
            debug!("Verification: connection failed: {e}");
            return (None, None);
        }
    };

    // Wrap in a transaction so we can ROLLBACK (no side effects for DML)
    if client.batch_execute("BEGIN").is_err() {
        return (None, None);
    }

    // Execute original SQL and collect results as text rows
    let rows_a = match execute_as_text(&mut client, original_sql) {
        Ok(rows) => rows,
        Err(e) => {
            debug!("Verification: original query failed: {e}");
            let _ = client.batch_execute("ROLLBACK");
            return (None, None);
        }
    };

    // Execute optimized SQL
    let rows_b = match execute_as_text(&mut client, &optimized_sql) {
        Ok(rows) => rows,
        Err(e) => {
            debug!("Verification: optimized query failed: {e}");
            let _ = client.batch_execute("ROLLBACK");
            // Emit failure: the optimized SQL is invalid
            return (
                Some(false),
                Some(format!("Optimized SQL execution failed: {e}")),
            );
        }
    };

    let _ = client.batch_execute("ROLLBACK");

    // Compare result sets
    compare_result_sets(rows_a, rows_b, original_sql)
}

/// Execute a SQL query and return rows as sorted vectors of text columns.
#[cfg(feature = "live-comparison")]
fn execute_as_text(
    client: &mut postgres::Client,
    sql: &str,
) -> Result<Vec<Vec<String>>, String> {
    let rows = client
        .query(sql, &[])
        .map_err(|e| e.to_string())?;

    let mut result: Vec<Vec<String>> = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut cols = Vec::with_capacity(row.len());
        for i in 0..row.len() {
            // Get each column as text representation
            let val: Option<String> = row.try_get::<_, Option<String>>(i)
                .or_else(|_| {
                    // Try as i64
                    row.try_get::<_, Option<i64>>(i)
                        .map(|v| v.map(|n| n.to_string()))
                })
                .or_else(|_| {
                    // Try as f64
                    row.try_get::<_, Option<f64>>(i)
                        .map(|v| v.map(|n| format!("{n}")))
                })
                .or_else(|_| {
                    // Try as bool
                    row.try_get::<_, Option<bool>>(i)
                        .map(|v| v.map(|b| b.to_string()))
                })
                .unwrap_or(None);
            cols.push(val.unwrap_or_else(|| "NULL".to_owned()));
        }
        result.push(cols);
    }

    Ok(result)
}

/// Compare two result sets, sorting both to handle order differences.
///
/// Returns `(Some(true), None)` on match or `(Some(false), Some(detail))` on mismatch.
#[cfg(feature = "live-comparison")]
fn compare_result_sets(
    mut rows_a: Vec<Vec<String>>,
    mut rows_b: Vec<Vec<String>>,
    _original_sql: &str,
) -> (Option<bool>, Option<String>) {
    // Sort both for order-independent comparison
    rows_a.sort();
    rows_b.sort();

    if rows_a == rows_b {
        return (Some(true), None);
    }

    // Build a useful mismatch detail
    let detail = if rows_a.len() != rows_b.len() {
        format!(
            "Row count mismatch: original={}, optimized={}",
            rows_a.len(),
            rows_b.len()
        )
    } else if !rows_a.is_empty()
        && !rows_b.is_empty()
        && rows_a[0].len() != rows_b[0].len()
    {
        format!(
            "Column count mismatch: original={}, optimized={}",
            rows_a[0].len(),
            rows_b[0].len()
        )
    } else {
        // Find first differing row
        let first_diff = rows_a
            .iter()
            .zip(rows_b.iter())
            .position(|(a, b)| a != b);
        match first_diff {
            Some(idx) => format!(
                "Row {idx} differs: original={:?}, optimized={:?}",
                rows_a[idx], rows_b[idx]
            ),
            None => "Unknown difference after sorting".to_owned(),
        }
    };

    (Some(false), Some(detail))
}
