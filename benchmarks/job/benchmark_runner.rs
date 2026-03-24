//! Comprehensive benchmark runner for the Join Order Benchmark (JOB).
//!
//! Collects all 5 benchmark dimensions for each of the 113 JOB queries:
//! 1. Planning efficiency (time to optimize)
//! 2. Planning accuracy (cost estimate vs actual)
//! 3. Execution time (query runtime)
//! 4. Resource consumption (CPU, memory, I/O)
//! 5. Correctness (result equivalence)
//!
//! Outputs JSON metrics to `benchmarks/job/results/metrics.json` and
//! generates a comprehensive markdown report.
//!
//! Build and run:
//!   rustc benchmarks/job/benchmark_runner.rs -o target/benchmark_runner
//!   # Or use as a module reference for the benchmark harness.

use std::collections::HashMap;

/// All metrics collected for a single query across 5 dimensions.
#[derive(Debug, Clone)]
pub struct BenchmarkMetrics {
    /// Query identifier (e.g., "1a", "13c", "33b").
    pub query_id: String,

    // -- Dimension 1: Planning Efficiency --
    /// Time spent by Ra optimizer to produce a plan (ms).
    pub plan_time_ms: f64,
    /// Estimated cost from Ra's cost model.
    pub plan_cost_estimate: f64,
    /// Number of rewrite rules applied during optimization.
    pub rules_applied: usize,
    /// Number of e-graph nodes at saturation.
    pub egraph_nodes: usize,
    /// Whether the plan was served from the plan cache.
    pub cache_hit: bool,

    // -- Dimension 2: Planning Accuracy --
    /// Actual execution cost (from EXPLAIN ANALYZE).
    pub actual_cost: f64,
    /// Q-error: max(estimated/actual, actual/estimated).
    /// 1.0 = perfect estimate; higher = worse.
    pub q_error: f64,
    /// Estimated row count from the optimizer.
    pub estimated_rows: f64,
    /// Actual row count from execution.
    pub actual_rows: f64,

    // -- Dimension 3: Execution Time --
    /// PostgreSQL execution time (ms).
    pub pg_exec_time_ms: f64,
    /// Ra-optimized execution time (ms).
    pub ra_exec_time_ms: f64,
    /// Number of rows returned by the query.
    pub rows_returned: u64,

    // -- Dimension 4: Resource Consumption --
    /// Peak memory usage during optimization (MB).
    pub peak_memory_mb: f64,
    /// CPU time consumed during optimization (ms).
    pub cpu_time_ms: f64,
    /// Bytes read from disk during execution.
    pub io_bytes_read: u64,
    /// Bytes written to disk during execution.
    pub io_bytes_written: u64,

    // -- Dimension 5: Correctness --
    /// SHA-256 hash of the sorted result set.
    pub result_hash: String,
    /// Whether Ra and PostgreSQL produced identical results.
    pub results_match: bool,
}

impl BenchmarkMetrics {
    /// Create a new metrics instance with defaults for a given query.
    pub fn new(query_id: &str) -> Self {
        Self {
            query_id: query_id.to_owned(),
            plan_time_ms: 0.0,
            plan_cost_estimate: 0.0,
            rules_applied: 0,
            egraph_nodes: 0,
            cache_hit: false,
            actual_cost: 0.0,
            q_error: 1.0,
            estimated_rows: 0.0,
            actual_rows: 0.0,
            pg_exec_time_ms: 0.0,
            ra_exec_time_ms: 0.0,
            rows_returned: 0,
            peak_memory_mb: 0.0,
            cpu_time_ms: 0.0,
            io_bytes_read: 0,
            io_bytes_written: 0,
            result_hash: String::new(),
            results_match: false,
        }
    }

    /// Compute Q-error from estimated and actual costs.
    ///
    /// Q-error = max(estimated/actual, actual/estimated).
    /// Returns 1.0 for perfect estimates, higher for worse.
    pub fn compute_q_error(estimated: f64, actual: f64) -> f64 {
        if estimated <= 0.0 || actual <= 0.0 {
            return f64::INFINITY;
        }
        let ratio = estimated / actual;
        if ratio >= 1.0 { ratio } else { 1.0 / ratio }
    }

    /// Compute execution speedup (PG time / Ra time).
    ///
    /// Returns > 1.0 when Ra is faster, < 1.0 when PG is faster.
    pub fn exec_speedup(&self) -> f64 {
        if self.ra_exec_time_ms <= 0.0 {
            return 0.0;
        }
        self.pg_exec_time_ms / self.ra_exec_time_ms
    }

    /// Compute planning speedup vs a baseline planning time.
    pub fn plan_speedup(&self, pg_plan_time_ms: f64) -> f64 {
        if self.plan_time_ms <= 0.0 {
            return 0.0;
        }
        pg_plan_time_ms / self.plan_time_ms
    }
}

/// Aggregated summary across all queries.
#[derive(Debug, Clone)]
pub struct BenchmarkSummary {
    pub total_queries: usize,
    pub queries_measured: usize,
    pub queries_correct: usize,

    // Planning efficiency
    pub median_plan_time_ms: f64,
    pub p95_plan_time_ms: f64,
    pub max_plan_time_ms: f64,
    pub total_plan_time_ms: f64,

    // Planning accuracy
    pub median_q_error: f64,
    pub p95_q_error: f64,
    pub max_q_error: f64,
    pub geometric_mean_q_error: f64,

    // Execution performance
    pub pg_total_exec_ms: f64,
    pub ra_total_exec_ms: f64,
    pub median_speedup: f64,
    pub queries_ra_faster: usize,
    pub queries_pg_faster: usize,

    // Resource consumption
    pub avg_peak_memory_mb: f64,
    pub max_peak_memory_mb: f64,
    pub avg_cpu_time_ms: f64,

    // Correctness
    pub correctness_pct: f64,
}

impl BenchmarkSummary {
    /// Compute summary statistics from a collection of per-query metrics.
    pub fn from_metrics(metrics: &[BenchmarkMetrics]) -> Self {
        let total_queries = 113;
        let queries_measured = metrics.len();
        let queries_correct =
            metrics.iter().filter(|m| m.results_match).count();

        // Planning times
        let mut plan_times: Vec<f64> =
            metrics.iter().map(|m| m.plan_time_ms).collect();
        plan_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_plan_time_ms = percentile(&plan_times, 50.0);
        let p95_plan_time_ms = percentile(&plan_times, 95.0);
        let max_plan_time_ms =
            plan_times.last().copied().unwrap_or(0.0);
        let total_plan_time_ms: f64 = plan_times.iter().sum();

        // Q-errors
        let mut q_errors: Vec<f64> =
            metrics.iter().map(|m| m.q_error).collect();
        q_errors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_q_error = percentile(&q_errors, 50.0);
        let p95_q_error = percentile(&q_errors, 95.0);
        let max_q_error =
            q_errors.last().copied().unwrap_or(1.0);
        let geometric_mean_q_error = geometric_mean(&q_errors);

        // Execution times
        let pg_total_exec_ms: f64 =
            metrics.iter().map(|m| m.pg_exec_time_ms).sum();
        let ra_total_exec_ms: f64 =
            metrics.iter().map(|m| m.ra_exec_time_ms).sum();

        let mut speedups: Vec<f64> =
            metrics.iter().map(|m| m.exec_speedup()).collect();
        speedups.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median_speedup = percentile(&speedups, 50.0);
        let queries_ra_faster =
            metrics.iter().filter(|m| m.exec_speedup() > 1.0).count();
        let queries_pg_faster =
            metrics.iter().filter(|m| m.exec_speedup() < 1.0).count();

        // Resources
        let avg_peak_memory_mb = if queries_measured > 0 {
            metrics.iter().map(|m| m.peak_memory_mb).sum::<f64>()
                / queries_measured as f64
        } else {
            0.0
        };
        let max_peak_memory_mb = metrics
            .iter()
            .map(|m| m.peak_memory_mb)
            .fold(0.0_f64, f64::max);
        let avg_cpu_time_ms = if queries_measured > 0 {
            metrics.iter().map(|m| m.cpu_time_ms).sum::<f64>()
                / queries_measured as f64
        } else {
            0.0
        };

        let correctness_pct = if queries_measured > 0 {
            (queries_correct as f64 / queries_measured as f64) * 100.0
        } else {
            0.0
        };

        Self {
            total_queries,
            queries_measured,
            queries_correct,
            median_plan_time_ms,
            p95_plan_time_ms,
            max_plan_time_ms,
            total_plan_time_ms,
            median_q_error,
            p95_q_error,
            max_q_error,
            geometric_mean_q_error,
            pg_total_exec_ms,
            ra_total_exec_ms,
            median_speedup,
            queries_ra_faster,
            queries_pg_faster,
            avg_peak_memory_mb,
            max_peak_memory_mb,
            avg_cpu_time_ms,
            correctness_pct,
        }
    }
}

/// Generate a comprehensive markdown report from benchmark metrics.
pub fn generate_report(
    metrics: &[BenchmarkMetrics],
    pg_plan_times: &HashMap<String, f64>,
) -> String {
    let summary = BenchmarkSummary::from_metrics(metrics);
    let mut report = String::with_capacity(16384);

    // Header
    report.push_str("# JOB Comprehensive Benchmark Report\n\n");
    report.push_str("## Overview\n\n");
    report.push_str(&format!(
        "- **Queries measured**: {}/{}\n",
        summary.queries_measured, summary.total_queries
    ));
    report.push_str(&format!(
        "- **Correctness**: {:.1}% ({}/{})\n",
        summary.correctness_pct,
        summary.queries_correct,
        summary.queries_measured
    ));
    report.push_str(&format!(
        "- **Execution speedup (median)**: {:.2}x\n",
        summary.median_speedup
    ));
    report.push_str(&format!(
        "- **Ra faster on**: {} queries | **PG faster on**: {} queries\n\n",
        summary.queries_ra_faster, summary.queries_pg_faster
    ));

    // Dimension 1: Planning Efficiency
    report.push_str("## 1. Planning Efficiency\n\n");
    report.push_str(
        "| Query | PG Plan (ms) | Ra Plan (ms) | Speedup | Rules | E-graph Nodes | Cache |\n",
    );
    report.push_str(
        "|-------|-------------|-------------|---------|-------|---------------|-------|\n",
    );
    for m in metrics {
        let pg_pt = pg_plan_times
            .get(&m.query_id)
            .copied()
            .unwrap_or(0.0);
        let speedup = m.plan_speedup(pg_pt);
        report.push_str(&format!(
            "| {} | {:.1} | {:.1} | {:.2}x | {} | {} | {} |\n",
            m.query_id,
            pg_pt,
            m.plan_time_ms,
            speedup,
            m.rules_applied,
            m.egraph_nodes,
            if m.cache_hit { "HIT" } else { "-" },
        ));
    }
    report.push_str(&format!(
        "\n**Summary**: Median={:.1}ms, P95={:.1}ms, Max={:.1}ms, Total={:.0}ms\n\n",
        summary.median_plan_time_ms,
        summary.p95_plan_time_ms,
        summary.max_plan_time_ms,
        summary.total_plan_time_ms,
    ));

    // Dimension 2: Planning Accuracy (Q-Error)
    report.push_str("## 2. Planning Accuracy (Q-Error)\n\n");
    report.push_str(
        "| Query | Est. Cost | Actual Cost | Q-Error | Est. Rows | Actual Rows |\n",
    );
    report.push_str(
        "|-------|-----------|-------------|---------|-----------|-------------|\n",
    );
    for m in metrics {
        report.push_str(&format!(
            "| {} | {:.0} | {:.0} | {:.2} | {:.0} | {:.0} |\n",
            m.query_id,
            m.plan_cost_estimate,
            m.actual_cost,
            m.q_error,
            m.estimated_rows,
            m.actual_rows,
        ));
    }
    report.push_str(&format!(
        "\n**Summary**: Median Q-Error={:.2}, P95={:.2}, Max={:.2}, Geometric Mean={:.2}\n\n",
        summary.median_q_error,
        summary.p95_q_error,
        summary.max_q_error,
        summary.geometric_mean_q_error,
    ));

    // Dimension 3: Execution Performance
    report.push_str("## 3. Execution Performance\n\n");
    report.push_str(
        "| Query | PG Exec (ms) | Ra Exec (ms) | Speedup | Rows |\n",
    );
    report.push_str(
        "|-------|-------------|-------------|---------|------|\n",
    );
    for m in metrics {
        let speedup = m.exec_speedup();
        let speedup_str = if speedup == 0.0 {
            "N/A".to_owned()
        } else {
            format!("{speedup:.2}x")
        };
        report.push_str(&format!(
            "| {} | {:.1} | {:.1} | {} | {} |\n",
            m.query_id,
            m.pg_exec_time_ms,
            m.ra_exec_time_ms,
            speedup_str,
            m.rows_returned,
        ));
    }
    report.push_str(&format!(
        "\n**Summary**: PG Total={:.0}ms, Ra Total={:.0}ms, Median Speedup={:.2}x\n\n",
        summary.pg_total_exec_ms,
        summary.ra_total_exec_ms,
        summary.median_speedup,
    ));

    // Dimension 4: Resource Consumption
    report.push_str("## 4. Resource Consumption\n\n");
    report.push_str(
        "| Query | Peak Mem (MB) | CPU Time (ms) | I/O Read (KB) | I/O Write (KB) |\n",
    );
    report.push_str(
        "|-------|-------------|-------------|-------------|---------------|\n",
    );
    for m in metrics {
        report.push_str(&format!(
            "| {} | {:.1} | {:.1} | {} | {} |\n",
            m.query_id,
            m.peak_memory_mb,
            m.cpu_time_ms,
            m.io_bytes_read / 1024,
            m.io_bytes_written / 1024,
        ));
    }
    report.push_str(&format!(
        "\n**Summary**: Avg Peak Memory={:.1}MB, Max Peak Memory={:.1}MB, Avg CPU={:.1}ms\n\n",
        summary.avg_peak_memory_mb,
        summary.max_peak_memory_mb,
        summary.avg_cpu_time_ms,
    ));

    // Dimension 5: Correctness
    report.push_str("## 5. Correctness Verification\n\n");
    report.push_str(
        "| Query | Result Hash | Match |\n",
    );
    report.push_str(
        "|-------|-------------|-------|\n",
    );
    for m in metrics {
        let hash_display = if m.result_hash.len() > 16 {
            format!("{}...", &m.result_hash[..16])
        } else if m.result_hash.is_empty() {
            "-".to_owned()
        } else {
            m.result_hash.clone()
        };
        let match_str = if m.results_match { "PASS" } else { "FAIL" };
        report.push_str(&format!(
            "| {} | {} | {} |\n",
            m.query_id, hash_display, match_str,
        ));
    }
    report.push_str(&format!(
        "\n**Correctness**: {:.1}% ({}/{} queries match)\n\n",
        summary.correctness_pct,
        summary.queries_correct,
        summary.queries_measured,
    ));

    // Dimension scorecard
    report.push_str("## Dimension Scorecard\n\n");
    report.push_str("| Dimension | Metric | Ra | Target | Status |\n");
    report.push_str("|-----------|--------|-----|--------|--------|\n");
    report.push_str(&format!(
        "| Planning Efficiency | Median plan time | {:.1}ms | <100ms | {} |\n",
        summary.median_plan_time_ms,
        if summary.median_plan_time_ms < 100.0 { "PASS" } else { "FAIL" },
    ));
    report.push_str(&format!(
        "| Planning Accuracy | Median Q-error | {:.2} | <2.0 | {} |\n",
        summary.median_q_error,
        if summary.median_q_error < 2.0 { "PASS" } else { "FAIL" },
    ));
    report.push_str(&format!(
        "| Execution Time | Median speedup | {:.2}x | >1.0x | {} |\n",
        summary.median_speedup,
        if summary.median_speedup > 1.0 { "PASS" } else { "FAIL" },
    ));
    report.push_str(&format!(
        "| Resource Consumption | Max memory | {:.1}MB | <512MB | {} |\n",
        summary.max_peak_memory_mb,
        if summary.max_peak_memory_mb < 512.0 { "PASS" } else { "FAIL" },
    ));
    report.push_str(&format!(
        "| Correctness | Match rate | {:.1}% | 100% | {} |\n",
        summary.correctness_pct,
        if (summary.correctness_pct - 100.0).abs() < f64::EPSILON {
            "PASS"
        } else {
            "FAIL"
        },
    ));

    report
}

/// Compute the value at a given percentile from a sorted slice.
fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = (pct / 100.0) * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - lo as f64;
    if lo == hi {
        sorted[lo]
    } else {
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

/// Compute geometric mean of positive values, ignoring non-positive.
fn geometric_mean(values: &[f64]) -> f64 {
    let positive: Vec<f64> =
        values.iter().copied().filter(|v| *v > 0.0).collect();
    if positive.is_empty() {
        return 0.0;
    }
    let log_sum: f64 =
        positive.iter().map(|v| v.ln()).sum();
    (log_sum / positive.len() as f64).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_error_perfect_estimate() {
        let q = BenchmarkMetrics::compute_q_error(100.0, 100.0);
        assert!((q - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_overestimate() {
        let q = BenchmarkMetrics::compute_q_error(200.0, 100.0);
        assert!((q - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_underestimate() {
        let q = BenchmarkMetrics::compute_q_error(50.0, 100.0);
        assert!((q - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn q_error_zero_returns_infinity() {
        let q = BenchmarkMetrics::compute_q_error(0.0, 100.0);
        assert!(q.is_infinite());
    }

    #[test]
    fn speedup_ra_faster() {
        let mut m = BenchmarkMetrics::new("test");
        m.pg_exec_time_ms = 200.0;
        m.ra_exec_time_ms = 100.0;
        assert!((m.exec_speedup() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn speedup_pg_faster() {
        let mut m = BenchmarkMetrics::new("test");
        m.pg_exec_time_ms = 100.0;
        m.ra_exec_time_ms = 200.0;
        assert!((m.exec_speedup() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn percentile_basic() {
        let vals = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&vals, 50.0) - 3.0).abs() < f64::EPSILON);
        assert!((percentile(&vals, 0.0) - 1.0).abs() < f64::EPSILON);
        assert!((percentile(&vals, 100.0) - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn geometric_mean_basic() {
        let vals = vec![1.0, 2.0, 4.0];
        let gm = geometric_mean(&vals);
        assert!((gm - 2.0).abs() < 0.01);
    }

    #[test]
    fn summary_from_empty() {
        let summary = BenchmarkSummary::from_metrics(&[]);
        assert_eq!(summary.queries_measured, 0);
        assert!((summary.correctness_pct - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn report_generation() {
        let mut m1 = BenchmarkMetrics::new("1a");
        m1.plan_time_ms = 10.0;
        m1.pg_exec_time_ms = 50.0;
        m1.ra_exec_time_ms = 25.0;
        m1.q_error = 1.5;
        m1.results_match = true;
        m1.result_hash = "abc123def456".to_owned();

        let metrics = vec![m1];
        let pg_times: HashMap<String, f64> =
            [("1a".to_owned(), 15.0)].into_iter().collect();
        let report = generate_report(&metrics, &pg_times);

        assert!(report.contains("Planning Efficiency"));
        assert!(report.contains("Planning Accuracy"));
        assert!(report.contains("Execution Performance"));
        assert!(report.contains("Resource Consumption"));
        assert!(report.contains("Correctness Verification"));
        assert!(report.contains("Dimension Scorecard"));
    }
}
