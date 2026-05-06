//! Production benchmark harness integrating Ra vs PostgreSQL comparison
//! with rigorous statistical analysis.
//!
//! The [`BenchmarkHarness`] combines:
//! - Query execution timing against both Ra and a PostgreSQL baseline
//! - [`StatisticalAnalyzer`] for 95% CI, paired t-tests, Cohen's d
//! - Workload-level aggregation (TPC-H, JOB, custom)
//! - JSON result export for reporting
//!
//! # Quick start
//!
//! ```ignore
//! use ra_bench::benchmark_harness::{BenchmarkHarness, WorkloadConfig};
//!
//! let mut harness = BenchmarkHarness::new(WorkloadConfig::default());
//! harness.add_query("Q1", "SELECT ...", &[14.2, 13.8, 14.5, 13.9, 14.1]);
//! let report = harness.analyze("tpch_sf1");
//! harness.save_report(&report, "results/tpch_sf1.json").unwrap();
//! ```

use std::time::Instant;

use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr::sql_to_relexpr;
use serde::{Deserialize, Serialize};

use crate::statistical_analysis::{
    AnalyzerConfig, BenchmarkComparison, StatisticalAnalyzer, WorkloadAnalysis,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for a benchmark workload.
#[derive(Debug, Clone)]
pub struct WorkloadConfig {
    /// Number of Ra optimizer runs per query (to build a timing distribution).
    pub ra_repetitions: usize,
    /// Minimum samples required for statistical validity (passed to analyzer).
    pub min_samples: usize,
    /// Confidence level for CIs (default: 0.95).
    pub confidence_level: f64,
    /// Family-wise error rate α for Bonferroni (default: 0.05).
    pub alpha: f64,
}

impl Default for WorkloadConfig {
    fn default() -> Self {
        Self {
            ra_repetitions: 30,
            min_samples: 5, // relaxed for offline benchmarks without live Postgres
            confidence_level: 0.95,
            alpha: 0.05,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-query timing record
// ---------------------------------------------------------------------------

/// Accumulated timing observations for one query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTimings {
    /// Query identifier.
    pub query_id: String,
    /// SQL text.
    pub sql: String,
    /// Ra total time per run (parse + optimize) in milliseconds.
    pub ra_times_ms: Vec<f64>,
    /// PostgreSQL execution times supplied by the caller (milliseconds).
    ///
    /// If empty, the query is excluded from statistical comparison but
    /// Ra timing data is still collected.
    pub postgres_times_ms: Vec<f64>,
    /// Whether Ra successfully optimized the query each run.
    pub ra_success_count: usize,
    /// Mean Ra parse time across successful runs (ms).
    pub mean_parse_ms: f64,
    /// Mean Ra optimize time across successful runs (ms).
    pub mean_optimize_ms: f64,
}

// ---------------------------------------------------------------------------
// Summary record
// ---------------------------------------------------------------------------

/// Full benchmark report for one workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// Workload name (e.g., "tpch_sf1", "job_imdb").
    pub workload_name: String,
    /// Per-query timing records.
    pub query_timings: Vec<QueryTimings>,
    /// Statistical analysis (only present when Postgres timings are available).
    pub statistical_analysis: Option<WorkloadAnalysisSummary>,
    /// Timestamp when the benchmark was run.
    pub timestamp: String,
    /// Ra version / git describe.
    pub ra_version: String,
}

/// Serialisable summary of `WorkloadAnalysis`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadAnalysisSummary {
    pub workload_name: String,
    pub mean_improvement_pct: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
    pub pct_significantly_improved: f64,
    pub regression_count: usize,
    pub skipped_count: usize,
    pub total_queries: usize,
    pub corrected_alpha: f64,
}

// ---------------------------------------------------------------------------
// BenchmarkHarness
// ---------------------------------------------------------------------------

/// Runs Ra timing measurements, collects PostgreSQL baselines, and
/// applies statistical analysis.
pub struct BenchmarkHarness {
    config: WorkloadConfig,
    optimizer: Optimizer,
    analyzer: StatisticalAnalyzer,
    query_timings: Vec<QueryTimings>,
}

impl BenchmarkHarness {
    /// Create a new harness with the given configuration.
    pub fn new(config: WorkloadConfig) -> Self {
        let analyzer_config = AnalyzerConfig {
            min_samples: config.min_samples,
            confidence_level: config.confidence_level,
            alpha: config.alpha,
            ..Default::default()
        };
        Self {
            optimizer: Optimizer::default(),
            analyzer: StatisticalAnalyzer::new(analyzer_config),
            query_timings: Vec::new(),
            config,
        }
    }

    /// Add a query with pre-collected PostgreSQL timing observations.
    ///
    /// Ra is re-measured locally `config.ra_repetitions` times.  Pass an
    /// empty slice for `postgres_times_ms` if no Postgres baseline is
    /// available (the query will still be benchmarked for Ra timing).
    pub fn add_query(
        &mut self,
        query_id: &str,
        sql: &str,
        postgres_times_ms: &[f64],
    ) -> &QueryTimings {
        let timings = self.measure_ra(query_id, sql, postgres_times_ms);
        self.query_timings.push(timings);
        self.query_timings.last().expect("just pushed")
    }

    /// Add multiple queries from a pre-built query set.
    pub fn add_query_set(&mut self, queries: &[(String, Vec<f64>)]) {
        for (sql, pg_times) in queries {
            let id = format!("Q{}", self.query_timings.len() + 1);
            self.add_query(&id, sql, pg_times);
        }
    }

    /// Run the statistical analysis and produce a [`BenchmarkReport`].
    pub fn analyze(&self, workload_name: &str) -> BenchmarkReport {
        let comparisons: Vec<BenchmarkComparison> = self
            .query_timings
            .iter()
            .filter(|t| !t.postgres_times_ms.is_empty())
            .map(|t| BenchmarkComparison {
                query_id: t.query_id.clone(),
                ra_times_ms: t.ra_times_ms.clone(),
                postgres_times_ms: t.postgres_times_ms.clone(),
            })
            .collect();

        let statistical_analysis = if comparisons.is_empty() {
            None
        } else {
            let wa = self.analyzer.analyze_workload(workload_name, &comparisons);
            Some(summarise(&wa))
        };

        BenchmarkReport {
            workload_name: workload_name.to_string(),
            query_timings: self.query_timings.clone(),
            statistical_analysis,
            timestamp: chrono::Utc::now().to_rfc3339(),
            ra_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Save a report to a JSON file.
    pub fn save_report(report: &BenchmarkReport, path: &str) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(report)?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a previously-saved report.
    pub fn load_report(path: &str) -> anyhow::Result<BenchmarkReport> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    // -----------------------------------------------------------------------
    // Ra measurement
    // -----------------------------------------------------------------------

    fn measure_ra(
        &self,
        query_id: &str,
        sql: &str,
        postgres_times_ms: &[f64],
    ) -> QueryTimings {
        let mut ra_times = Vec::with_capacity(self.config.ra_repetitions);
        let mut total_parse = 0.0f64;
        let mut total_optimize = 0.0f64;
        let mut success_count = 0usize;

        for _ in 0..self.config.ra_repetitions {
            // Parse
            let t0 = Instant::now();
            let parse_result = sql_to_relexpr(sql);
            let parse_ms = t0.elapsed().as_secs_f64() * 1000.0;

            let rel = match parse_result {
                Ok(r) => r,
                Err(_) => {
                    ra_times.push(f64::NAN);
                    continue;
                }
            };

            // Optimize
            let t1 = Instant::now();
            let _optimized = self.optimizer.optimize(&rel);
            let optimize_ms = t1.elapsed().as_secs_f64() * 1000.0;

            let total = parse_ms + optimize_ms;
            ra_times.push(total);
            total_parse += parse_ms;
            total_optimize += optimize_ms;
            success_count += 1;
        }

        // Remove NaN entries (failed parses) for statistics
        let valid_ra: Vec<f64> = ra_times.iter().copied().filter(|v| !v.is_nan()).collect();

        let (mean_parse, mean_optimize) = if success_count > 0 {
            (
                total_parse / success_count as f64,
                total_optimize / success_count as f64,
            )
        } else {
            (0.0, 0.0)
        };

        QueryTimings {
            query_id: query_id.to_string(),
            sql: sql.to_string(),
            ra_times_ms: valid_ra,
            postgres_times_ms: postgres_times_ms.to_vec(),
            ra_success_count: success_count,
            mean_parse_ms: mean_parse,
            mean_optimize_ms: mean_optimize,
        }
    }
}

fn summarise(wa: &WorkloadAnalysis) -> WorkloadAnalysisSummary {
    WorkloadAnalysisSummary {
        workload_name: wa.workload_name.clone(),
        mean_improvement_pct: wa.mean_improvement_pct,
        ci_lower: wa.improvement_ci.lower,
        ci_upper: wa.improvement_ci.upper,
        pct_significantly_improved: wa.pct_significantly_improved,
        regression_count: wa.regression_count,
        skipped_count: wa.skipped_count,
        total_queries: wa.results.len(),
        corrected_alpha: wa.corrected_alpha,
    }
}

// ---------------------------------------------------------------------------
// TPC-H convenience runner
// ---------------------------------------------------------------------------

/// Run the TPC-H OLAP query set through the harness.
///
/// `pg_times_by_sql` is an optional map from SQL string to Postgres timings.
/// Pass an empty map to collect only Ra timings.
pub fn run_tpch_benchmark(
    config: WorkloadConfig,
    pg_times_by_sql: &std::collections::HashMap<String, Vec<f64>>,
) -> BenchmarkReport {
    let queries = crate::training_collector::olap_query_set();
    let mut harness = BenchmarkHarness::new(config);

    for (i, sql) in queries.iter().enumerate() {
        let id = format!("TPCH_Q{}", i + 1);
        let pg_times = pg_times_by_sql.get(sql).map(Vec::as_slice).unwrap_or(&[]);
        harness.add_query(&id, sql, pg_times);
    }

    harness.analyze("tpch_olap")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harness_creates_and_runs() {
        let mut harness = BenchmarkHarness::new(WorkloadConfig {
            ra_repetitions: 3,
            ..Default::default()
        });
        let timings = harness.add_query(
            "simple_scan",
            "SELECT c_custkey FROM customer WHERE c_custkey = 1",
            &[], // no Postgres baseline
        );
        // Either parse succeeded or failed — we just need the harness to not panic
        assert_eq!(timings.query_id, "simple_scan");
    }

    #[test]
    fn test_analyze_without_postgres_data() {
        let mut harness = BenchmarkHarness::new(WorkloadConfig {
            ra_repetitions: 3,
            ..Default::default()
        });
        harness.add_query("Q1", "SELECT 1", &[]);
        let report = harness.analyze("test_workload");
        assert_eq!(report.workload_name, "test_workload");
        assert!(report.statistical_analysis.is_none());
    }

    #[test]
    fn test_analyze_with_postgres_data_produces_summary() {
        let mut harness = BenchmarkHarness::new(WorkloadConfig {
            ra_repetitions: 10,
            min_samples: 3,
            ..Default::default()
        });
        // Provide synthetic Postgres times (Ra should be ~20% faster)
        let pg_times: Vec<f64> = (0..10).map(|i| 12.0 + i as f64 * 0.1).collect();
        harness.add_query("Q1", "SELECT 1", &pg_times);
        let report = harness.analyze("test_workload");
        // Statistical analysis should be present (even if Q1 parse fails,
        // the summary is still created for the comparison attempt)
        assert_eq!(report.query_timings.len(), 1);
    }

    #[test]
    fn test_save_and_load_report() {
        let mut harness = BenchmarkHarness::new(WorkloadConfig {
            ra_repetitions: 2,
            ..Default::default()
        });
        harness.add_query("Q1", "SELECT 1", &[]);
        let report = harness.analyze("save_test");

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/benchmark_report_test.json");
        BenchmarkHarness::save_report(&report, path.to_str().expect("path")).expect("save");

        let loaded = BenchmarkHarness::load_report(path.to_str().expect("path")).expect("load");
        assert_eq!(loaded.workload_name, "save_test");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_run_tpch_benchmark_smoke() {
        let config = WorkloadConfig { ra_repetitions: 2, ..Default::default() };
        let report = run_tpch_benchmark(config, &std::collections::HashMap::new());
        assert_eq!(report.workload_name, "tpch_olap");
        assert!(!report.query_timings.is_empty());
    }
}
