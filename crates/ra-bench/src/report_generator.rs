//! Phase 5: Comprehensive analysis pipeline and report generator.
//!
//! Reads one or more [`BenchmarkReport`] files produced by [`BenchmarkHarness`]
//! and generates the executive deliverable described in the project plan:
//!
//! - **Executive summary** — overall improvement %, CI, significance
//! - **Per-workload analysis** — OLAP, OLTP, JOB breakdown
//! - **Neural model validation** — prediction accuracy metrics
//! - **Regression detection** — queries where Ra is significantly slower
//! - **Production readiness assessment** — go/no-go recommendation
//!
//! # Usage
//!
//! ```ignore
//! use ra_bench::report_generator::ReportGenerator;
//!
//! let mut gen = ReportGenerator::new();
//! gen.add_report_file("results/tpch_sf1.json").unwrap();
//! gen.add_report_file("results/job_imdb.json").unwrap();
//! println!("{}", gen.generate_markdown());
//! gen.save_markdown("reports/executive_summary.md").unwrap();
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::benchmark_harness::{BenchmarkReport, QueryTimings};
use crate::statistical_analysis::{
    confidence_interval_for_mean, mean, std_dev, AnalyzerConfig, BenchmarkComparison,
    StatisticalAnalyzer,
};

// ---------------------------------------------------------------------------
// Executive summary types
// ---------------------------------------------------------------------------

/// Top-level executive summary across all workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutiveSummary {
    /// Overall mean improvement across all workloads (%).
    pub overall_improvement_pct: f64,
    /// 95% CI lower bound for the overall improvement.
    pub ci_lower: f64,
    /// 95% CI upper bound for the overall improvement.
    pub ci_upper: f64,
    /// Percentage of queries with statistically significant improvement.
    pub pct_significantly_improved: f64,
    /// Total number of regressions detected across all workloads.
    pub total_regressions: usize,
    /// Number of workloads analyzed.
    pub workload_count: usize,
    /// Total queries analyzed (including skipped).
    pub total_queries: usize,
    /// Production readiness recommendation.
    pub recommendation: Recommendation,
    /// Workload with the highest improvement.
    pub best_workload: Option<String>,
    /// Workload with the lowest improvement (or worst regression).
    pub worst_workload: Option<String>,
}

/// Production readiness recommendation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recommendation {
    /// Ra shows consistent improvements — recommend deployment.
    Deploy,
    /// Ra shows improvements on most workloads — recommend staged rollout.
    StagedRollout,
    /// Ra shows mixed results — recommend further tuning before deployment.
    FurtherTuning,
    /// Ra shows regressions — do not deploy until fixed.
    DoNotDeploy,
}

impl Recommendation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deploy => "DEPLOY — consistent improvement across all workloads",
            Self::StagedRollout => "STAGED ROLLOUT — strong improvement, gradual rollout advised",
            Self::FurtherTuning => "FURTHER TUNING — mixed results, additional optimization needed",
            Self::DoNotDeploy => "DO NOT DEPLOY — statistically significant regressions detected",
        }
    }
}

/// A detected performance regression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionAlert {
    /// Workload where the regression was found.
    pub workload: String,
    /// Query identifier.
    pub query_id: String,
    /// How much slower Ra is vs PostgreSQL (positive = Ra is slower).
    pub slowdown_pct: f64,
    /// p-value from the paired t-test (smaller = more significant).
    pub p_value: f64,
    /// Whether this is statistically significant after Bonferroni correction.
    pub significant: bool,
}

// ---------------------------------------------------------------------------
// Report generator
// ---------------------------------------------------------------------------

/// Aggregates multiple `BenchmarkReport` files and generates comprehensive output.
pub struct ReportGenerator {
    reports: Vec<BenchmarkReport>,
    analyzer_config: AnalyzerConfig,
}

impl ReportGenerator {
    /// Create a new generator with default statistical configuration.
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
            analyzer_config: AnalyzerConfig { min_samples: 3, ..Default::default() },
        }
    }

    /// Add a `BenchmarkReport` directly.
    pub fn add_report(&mut self, report: BenchmarkReport) -> &mut Self {
        self.reports.push(report);
        self
    }

    /// Load and add a `BenchmarkReport` from a JSON file.
    pub fn add_report_file(&mut self, path: &str) -> anyhow::Result<&mut Self> {
        let report = crate::benchmark_harness::BenchmarkHarness::load_report(path)?;
        self.reports.push(report);
        Ok(self)
    }

    /// Compute the executive summary across all loaded reports.
    pub fn executive_summary(&self) -> ExecutiveSummary {
        if self.reports.is_empty() {
            return ExecutiveSummary {
                overall_improvement_pct: 0.0,
                ci_lower: 0.0,
                ci_upper: 0.0,
                pct_significantly_improved: 0.0,
                total_regressions: 0,
                workload_count: 0,
                total_queries: 0,
                recommendation: Recommendation::FurtherTuning,
                best_workload: None,
                worst_workload: None,
            };
        }

        // Collect per-workload improvements and per-query comparisons
        let mut all_improvements: Vec<f64> = Vec::new();
        let mut total_regressions = 0usize;
        let mut total_significant_improved = 0usize;
        let mut total_compared = 0usize;
        let mut workload_improvements: Vec<(String, f64)> = Vec::new();

        let analyzer = StatisticalAnalyzer::new(self.analyzer_config.clone());

        for report in &self.reports {
            let comparisons = self.extract_comparisons(report);
            if comparisons.is_empty() {
                continue;
            }
            let wa = analyzer.analyze_workload(&report.workload_name, &comparisons);
            for r in &wa.results {
                all_improvements.push(r.improvement_pct);
                if r.significant_after_correction {
                    if r.improvement_pct < 0.0 {
                        total_regressions += 1;
                    } else {
                        total_significant_improved += 1;
                    }
                }
                total_compared += 1;
            }
            workload_improvements.push((report.workload_name.clone(), wa.mean_improvement_pct));
        }

        let overall = mean(&all_improvements);
        let ci = confidence_interval_for_mean(&all_improvements, 0.95);
        let pct_sig = if total_compared > 0 {
            total_significant_improved as f64 / total_compared as f64 * 100.0
        } else {
            0.0
        };

        let best = workload_improvements
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(name, _)| name.clone());
        let worst = workload_improvements
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(name, _)| name.clone());

        let recommendation = self.recommend(overall, total_regressions, pct_sig);
        let total_queries: usize = self.reports.iter().map(|r| r.query_timings.len()).sum();

        ExecutiveSummary {
            overall_improvement_pct: overall,
            ci_lower: ci.lower,
            ci_upper: ci.upper,
            pct_significantly_improved: pct_sig,
            total_regressions,
            workload_count: self.reports.len(),
            total_queries,
            recommendation,
            best_workload: best,
            worst_workload: worst,
        }
    }

    /// Detect all statistically significant regressions across loaded reports.
    pub fn detect_regressions(&self) -> Vec<RegressionAlert> {
        let mut regressions = Vec::new();
        let analyzer = StatisticalAnalyzer::new(self.analyzer_config.clone());

        for report in &self.reports {
            let comparisons = self.extract_comparisons(report);
            if comparisons.is_empty() {
                continue;
            }
            let wa = analyzer.analyze_workload(&report.workload_name, &comparisons);
            for result in &wa.results {
                if result.improvement_pct < 0.0 {
                    regressions.push(RegressionAlert {
                        workload: report.workload_name.clone(),
                        query_id: result.query_id.clone(),
                        slowdown_pct: -result.improvement_pct,
                        p_value: result.t_test.p_value,
                        significant: result.significant_after_correction,
                    });
                }
            }
        }
        regressions.sort_by(|a, b| {
            b.slowdown_pct.partial_cmp(&a.slowdown_pct).unwrap_or(std::cmp::Ordering::Equal)
        });
        regressions
    }

    /// Generate a comprehensive Markdown report.
    pub fn generate_markdown(&self) -> String {
        let summary = self.executive_summary();
        let regressions = self.detect_regressions();
        let analyzer = StatisticalAnalyzer::new(self.analyzer_config.clone());
        let mut md = String::with_capacity(8192);

        // Title
        md.push_str("# Ra-PostgreSQL Integration: Production Performance Analysis\n\n");

        // Executive summary
        self.write_executive_summary(&mut md, &summary);

        // Detailed per-workload analysis
        md.push_str("\n## Detailed Workload Analysis\n\n");
        for report in &self.reports {
            self.write_workload_section(&mut md, report, &analyzer);
        }

        // Regression report
        if !regressions.is_empty() {
            self.write_regression_section(&mut md, &regressions);
        } else {
            md.push_str("\n## Regression Analysis\n\n");
            md.push_str("No statistically significant regressions detected.\n\n");
        }

        // Ra optimizer timing summary
        md.push_str("\n## Ra Optimizer Timing Summary\n\n");
        self.write_timing_table(&mut md);

        // Production deployment recommendations
        md.push_str("\n## Production Deployment Recommendation\n\n");
        md.push_str(&format!("**{}**\n\n", summary.recommendation.as_str()));
        self.write_deployment_checklist(&mut md, &summary);

        md
    }

    /// Save the generated Markdown report to a file.
    pub fn save_markdown(&self, path: &str) -> anyhow::Result<()> {
        let md = self.generate_markdown();
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, md)?;
        Ok(())
    }

    /// Export the executive summary as JSON.
    pub fn executive_summary_json(&self) -> anyhow::Result<String> {
        let summary = self.executive_summary();
        Ok(serde_json::to_string_pretty(&summary)?)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn extract_comparisons(&self, report: &BenchmarkReport) -> Vec<BenchmarkComparison> {
        report
            .query_timings
            .iter()
            .filter(|t| !t.postgres_times_ms.is_empty() && !t.ra_times_ms.is_empty())
            .map(|t| BenchmarkComparison {
                query_id: t.query_id.clone(),
                ra_times_ms: t.ra_times_ms.clone(),
                postgres_times_ms: t.postgres_times_ms.clone(),
            })
            .collect()
    }

    fn recommend(&self, improvement: f64, regressions: usize, pct_sig: f64) -> Recommendation {
        if regressions > 0 {
            Recommendation::DoNotDeploy
        } else if improvement >= 20.0 && pct_sig >= 50.0 {
            Recommendation::Deploy
        } else if improvement >= 10.0 {
            Recommendation::StagedRollout
        } else {
            Recommendation::FurtherTuning
        }
    }

    fn write_executive_summary(&self, md: &mut String, s: &ExecutiveSummary) {
        md.push_str("## Executive Summary\n\n");
        md.push_str(&format!(
            "- **Overall Performance**: Ra achieved {:.1}% ± {:.1}% improvement (95% CI: [{:.1}%, {:.1}%])\n",
            s.overall_improvement_pct,
            (s.ci_upper - s.ci_lower) / 2.0,
            s.ci_lower,
            s.ci_upper,
        ));
        md.push_str(&format!(
            "- **Statistical Significance**: {:.1}% of queries show significant improvement\n",
            s.pct_significantly_improved,
        ));
        md.push_str(&format!(
            "- **Regression Analysis**: {} regressions detected across {} workloads ({} queries)\n",
            s.total_regressions, s.workload_count, s.total_queries,
        ));
        md.push_str(&format!(
            "- **Production Readiness**: {}\n\n",
            s.recommendation.as_str(),
        ));
        if let Some(best) = &s.best_workload {
            md.push_str(&format!("- **Best workload**: {best}\n"));
        }
        if let Some(worst) = &s.worst_workload {
            md.push_str(&format!("- **Needs attention**: {worst}\n"));
        }
    }

    fn write_workload_section(
        &self,
        md: &mut String,
        report: &BenchmarkReport,
        analyzer: &StatisticalAnalyzer,
    ) {
        md.push_str(&format!("### {}\n\n", report.workload_name));

        let comparisons = self.extract_comparisons(report);
        if comparisons.is_empty() {
            md.push_str("_No PostgreSQL baseline available for this workload._\n\n");
            return;
        }

        let wa = analyzer.analyze_workload(&report.workload_name, &comparisons);

        md.push_str(&format!(
            "- Mean improvement: **{:.1}%** (95% CI: [{:.1}%, {:.1}%])\n",
            wa.mean_improvement_pct, wa.improvement_ci.lower, wa.improvement_ci.upper,
        ));
        md.push_str(&format!(
            "- Queries significantly improved: {:.0}%\n",
            wa.pct_significantly_improved,
        ));
        md.push_str(&format!("- Regressions: {}\n", wa.regression_count));
        md.push_str(&format!(
            "- Queries analyzed: {} (skipped: {})\n\n",
            wa.results.len(),
            wa.skipped_count,
        ));

        if !wa.results.is_empty() {
            md.push_str("| Query | Ra mean (ms) | PG mean (ms) | Improvement | Significant |\n");
            md.push_str("|-------|-------------|-------------|-------------|-------------|\n");
            for r in &wa.results {
                md.push_str(&format!(
                    "| {} | {:.2} | {:.2} | {:.1}% | {} |\n",
                    r.query_id,
                    r.ra_mean_ms,
                    r.postgres_mean_ms,
                    r.improvement_pct,
                    if r.significant_after_correction { "✓" } else { "" },
                ));
            }
            md.push('\n');
        }
    }

    fn write_regression_section(&self, md: &mut String, regressions: &[RegressionAlert]) {
        md.push_str("\n## Regression Analysis\n\n");
        md.push_str(&format!(
            "**{} regression(s) detected** requiring investigation before deployment.\n\n",
            regressions.len()
        ));
        md.push_str("| Workload | Query | Slowdown | p-value | Significant |\n");
        md.push_str("|----------|-------|----------|---------|-------------|\n");
        for r in regressions {
            md.push_str(&format!(
                "| {} | {} | +{:.1}% | {:.4} | {} |\n",
                r.workload,
                r.query_id,
                r.slowdown_pct,
                r.p_value,
                if r.significant { "YES" } else { "no" },
            ));
        }
        md.push('\n');
    }

    fn write_timing_table(&self, md: &mut String) {
        // Aggregate Ra parse + optimize times across all reports
        let mut parse_times: Vec<f64> = Vec::new();
        let mut optimize_times: Vec<f64> = Vec::new();
        let mut by_workload: HashMap<&str, (Vec<f64>, Vec<f64>)> = HashMap::new();

        for report in &self.reports {
            let entry = by_workload.entry(report.workload_name.as_str()).or_default();
            for t in &report.query_timings {
                if t.ra_success_count > 0 {
                    parse_times.push(t.mean_parse_ms);
                    optimize_times.push(t.mean_optimize_ms);
                    entry.0.push(t.mean_parse_ms);
                    entry.1.push(t.mean_optimize_ms);
                }
            }
        }

        if parse_times.is_empty() {
            md.push_str("_No timing data available._\n");
            return;
        }

        md.push_str("| Workload | Avg Parse (ms) | Avg Optimize (ms) | Avg Total (ms) |\n");
        md.push_str("|----------|---------------|------------------|----------------|\n");

        for (name, (parse, opt)) in &by_workload {
            let p = mean(parse);
            let o = mean(opt);
            md.push_str(&format!("| {name} | {p:.3} | {o:.3} | {:.3} |\n", p + o));
        }

        let total_p = mean(&parse_times);
        let total_o = mean(&optimize_times);
        md.push_str(&format!(
            "| **Overall** | **{total_p:.3}** | **{total_o:.3}** | **{:.3}** |\n\n",
            total_p + total_o,
        ));
    }

    fn write_deployment_checklist(&self, md: &mut String, summary: &ExecutiveSummary) {
        md.push_str("### Pre-deployment Checklist\n\n");
        let items: &[(&str, bool)] = &[
            (
                "Zero statistically significant regressions",
                summary.total_regressions == 0,
            ),
            (
                "Overall improvement > 10%",
                summary.overall_improvement_pct > 10.0,
            ),
            (
                "CI lower bound > 0%",
                summary.ci_lower > 0.0,
            ),
            (
                ">50% queries show significant improvement",
                summary.pct_significantly_improved > 50.0,
            ),
        ];

        for (check, passed) in items {
            let icon = if *passed { "✅" } else { "❌" };
            md.push_str(&format!("- {icon} {check}\n"));
        }

        md.push_str("\n### Rollout Strategy\n\n");
        match summary.recommendation {
            Recommendation::Deploy => {
                md.push_str("1. Deploy to production with full query coverage\n");
                md.push_str("2. Monitor prediction accuracy dashboard\n");
                md.push_str("3. Continue online learning with production workload\n");
            }
            Recommendation::StagedRollout => {
                md.push_str("1. Enable Ra for queries with >3 tables initially\n");
                md.push_str("2. Increase coverage 10% per week based on monitoring\n");
                md.push_str("3. Maintain fallback to standard planner at <1% rate\n");
            }
            Recommendation::FurtherTuning | Recommendation::DoNotDeploy => {
                md.push_str("1. Investigate regressions listed above\n");
                md.push_str("2. Re-run training with larger dataset (>10k samples)\n");
                md.push_str("3. Re-benchmark before reconsidering deployment\n");
            }
        }
    }
}

impl Default for ReportGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Workload summary for timing and Ra-only analysis
// ---------------------------------------------------------------------------

/// Summary statistics for Ra performance (without Postgres comparison).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaTimingSummary {
    pub workload_name: String,
    pub query_count: usize,
    pub success_count: usize,
    pub mean_total_ms: f64,
    pub std_total_ms: f64,
    pub mean_parse_ms: f64,
    pub mean_optimize_ms: f64,
}

/// Compute Ra-only timing summaries for all loaded reports.
pub fn ra_timing_summaries(reports: &[BenchmarkReport]) -> Vec<RaTimingSummary> {
    reports
        .iter()
        .map(|r| {
            let successful: Vec<&QueryTimings> =
                r.query_timings.iter().filter(|t| t.ra_success_count > 0).collect();
            let totals: Vec<f64> = successful
                .iter()
                .map(|t| t.mean_parse_ms + t.mean_optimize_ms)
                .collect();
            RaTimingSummary {
                workload_name: r.workload_name.clone(),
                query_count: r.query_timings.len(),
                success_count: successful.len(),
                mean_total_ms: mean(&totals),
                std_total_ms: std_dev(&totals),
                mean_parse_ms: mean(
                    &successful.iter().map(|t| t.mean_parse_ms).collect::<Vec<_>>(),
                ),
                mean_optimize_ms: mean(
                    &successful.iter().map(|t| t.mean_optimize_ms).collect::<Vec<_>>(),
                ),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::benchmark_harness::{BenchmarkHarness, WorkloadConfig};

    fn make_report(name: &str, pg_times: Vec<f64>) -> BenchmarkReport {
        let mut harness = BenchmarkHarness::new(WorkloadConfig {
            ra_repetitions: 5,
            min_samples: 3,
            ..Default::default()
        });
        harness.add_query("Q1", "SELECT 1", &pg_times);
        harness.analyze(name)
    }

    #[test]
    fn test_generator_empty_produces_safe_summary() {
        let gen = ReportGenerator::new();
        let s = gen.executive_summary();
        assert_eq!(s.workload_count, 0);
        assert_eq!(s.total_regressions, 0);
    }

    #[test]
    fn test_generator_single_report_no_pg_data() {
        let mut gen = ReportGenerator::new();
        gen.add_report(make_report("test", vec![]));
        let s = gen.executive_summary();
        assert_eq!(s.workload_count, 1);
    }

    #[test]
    fn test_markdown_generated() {
        let mut gen = ReportGenerator::new();
        gen.add_report(make_report("test_olap", vec![]));
        let md = gen.generate_markdown();
        assert!(md.contains("Executive Summary"), "must contain executive summary");
        assert!(md.contains("Production Deployment"), "must contain deployment section");
    }

    #[test]
    fn test_detect_regressions_empty_when_no_pg_data() {
        let mut gen = ReportGenerator::new();
        gen.add_report(make_report("olap", vec![]));
        let regressions = gen.detect_regressions();
        assert!(regressions.is_empty());
    }

    #[test]
    fn test_recommendation_logic() {
        let gen = ReportGenerator::new();
        assert_eq!(gen.recommend(25.0, 0, 60.0), Recommendation::Deploy);
        assert_eq!(gen.recommend(15.0, 0, 40.0), Recommendation::StagedRollout);
        assert_eq!(gen.recommend(5.0, 0, 20.0), Recommendation::FurtherTuning);
        assert_eq!(gen.recommend(30.0, 1, 80.0), Recommendation::DoNotDeploy);
    }

    #[test]
    fn test_ra_timing_summaries() {
        let report = make_report("timing_test", vec![]);
        let summaries = ra_timing_summaries(&[report]);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].workload_name, "timing_test");
    }
}
