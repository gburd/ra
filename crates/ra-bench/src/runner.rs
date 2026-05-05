//! Per-query measurement and result collection.

use std::time::Instant;

use ra_engine::Optimizer;
use ra_grammar_fuzzer::scoring::{compute_score, QueryDimensions, ScoringWeights};
use ra_parser::sql_to_relexpr::sql_to_relexpr;

/// Result of running a single query through the benchmark harness.
#[derive(Debug, Clone)]
#[allow(dead_code)] // sql and pg_explain_ms are for JSON reporting and future use
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
}

/// Configuration for the benchmark runner.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Scoring weights.
    pub weights: ScoringWeights,
    /// Postgres connection string for live comparison (if any).
    pub pg_connection: Option<String>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            weights: ScoringWeights::default(),
            pg_connection: None,
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
        actual_execution_time_ms: None,  // Set by analyze_query when enabled
        actual_rows: None,
        estimated_rows: None,
        score,
    }
}

// ---------------------------------------------------------------------------
// Postgres comparison helper (feature-gated)
// ---------------------------------------------------------------------------

/// Attempt a Postgres plan comparison.  Returns `(None, None, None)` when
/// either the feature is disabled or no connection string is configured.
#[allow(unused_variables)]
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
                    Some(result.structurally_similar.then_some(1.0).unwrap_or(0.0)),
                    result.cost_ratio,
                );
            }
        }
    }
    (None, None, None)
}
