//! Human-readable and JSON output for benchmark results.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::runner::QueryResult;

/// Summary statistics for one category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySummary {
    /// Category name.
    pub category: String,
    /// Total queries in this category.
    pub total: usize,
    /// Queries Ra successfully parsed.
    pub parsed: usize,
    /// Queries Ra failed to parse.
    pub failed: usize,
    /// Average parse time (ms) for successful parses.
    pub avg_parse_ms: f64,
    /// Average optimize time (ms) for successful parses.
    pub avg_optimize_ms: f64,
    /// Average Postgres EXPLAIN time (ms), if measured.
    pub avg_pg_explain_ms: Option<f64>,
    /// Average structural similarity [0,1] vs Postgres.
    pub avg_structural_sim: Option<f64>,
    /// Average cost ratio (Ra/Postgres).
    pub avg_cost_ratio: Option<f64>,
    /// Average composite score (0..1), or None if no scored queries.
    pub avg_score: Option<f64>,
}

/// Top-level report for a benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    /// Per-category summaries.
    pub categories: Vec<CategorySummary>,
    /// Grand totals.
    pub total_queries: usize,
    /// Queries Ra successfully parsed.
    pub total_parsed: usize,
    /// Queries Ra failed to parse.
    pub total_failed: usize,
    /// Overall average parse time (ms).
    pub overall_avg_parse_ms: f64,
    /// Overall average optimize time (ms).
    pub overall_avg_optimize_ms: f64,
    /// Overall average composite score, if available.
    pub overall_avg_score: Option<f64>,
}

impl BenchReport {
    /// Build a report from a list of [`QueryResult`] values.
    #[must_use]
    pub fn from_results(results: &[QueryResult]) -> Self {
        let mut by_cat: HashMap<&str, Vec<&QueryResult>> = HashMap::new();
        for r in results {
            by_cat.entry(r.category.as_str()).or_default().push(r);
        }

        let mut categories: Vec<CategorySummary> = by_cat
            .iter()
            .map(|(cat, items)| {
                let parsed: Vec<&&QueryResult> = items.iter().filter(|r| r.success).collect();
                let failed = items.len() - parsed.len();

                let avg_parse_ms = average(parsed.iter().map(|r| r.parse_ms));
                let avg_optimize_ms = average(parsed.iter().map(|r| r.optimize_ms));

                let pg_times: Vec<f64> = parsed.iter().filter_map(|r| r.pg_explain_ms).collect();
                let avg_pg_explain_ms = if pg_times.is_empty() {
                    None
                } else {
                    Some(pg_times.iter().sum::<f64>() / pg_times.len() as f64)
                };

                let structural_sims: Vec<f64> = parsed.iter().filter_map(|r| r.structural_sim).collect();
                let avg_structural_sim = if structural_sims.is_empty() {
                    None
                } else {
                    Some(structural_sims.iter().sum::<f64>() / structural_sims.len() as f64)
                };

                let cost_ratios: Vec<f64> = parsed.iter().filter_map(|r| r.cost_ratio).collect();
                let avg_cost_ratio = if cost_ratios.is_empty() {
                    None
                } else {
                    Some(cost_ratios.iter().sum::<f64>() / cost_ratios.len() as f64)
                };

                let scores: Vec<f64> = parsed.iter().filter_map(|r| r.score).collect();
                let avg_score = if scores.is_empty() {
                    None
                } else {
                    Some(scores.iter().sum::<f64>() / scores.len() as f64)
                };

                CategorySummary {
                    category: (*cat).to_owned(),
                    total: items.len(),
                    parsed: parsed.len(),
                    failed,
                    avg_parse_ms,
                    avg_optimize_ms,
                    avg_pg_explain_ms,
                    avg_structural_sim,
                    avg_cost_ratio,
                    avg_score,
                }
            })
            .collect();

        categories.sort_by(|a, b| a.category.cmp(&b.category));

        let total = results.len();
        let total_parsed = results.iter().filter(|r| r.success).count();
        let total_failed = total - total_parsed;
        let success: Vec<&QueryResult> = results.iter().filter(|r| r.success).collect();
        let overall_avg_parse_ms = average(success.iter().map(|r| r.parse_ms));
        let overall_avg_optimize_ms = average(success.iter().map(|r| r.optimize_ms));
        let all_scores: Vec<f64> = success.iter().filter_map(|r| r.score).collect();
        let overall_avg_score = if all_scores.is_empty() {
            None
        } else {
            Some(all_scores.iter().sum::<f64>() / all_scores.len() as f64)
        };

        Self {
            categories,
            total_queries: total,
            total_parsed,
            total_failed,
            overall_avg_parse_ms,
            overall_avg_optimize_ms,
            overall_avg_score,
        }
    }

    /// Write the report as JSON to `path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn write_json(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Print a human-readable summary table to stdout.
    pub fn print_summary(&self) {
        use std::io::Write;
        let mut out = std::io::stdout().lock();

        writeln!(
            out,
            "\n{:<22} {:>6} {:>6} {:>10} {:>10} {:>10} {:>8} {:>8} {:>8}",
            "Category", "Total", "Parse%", "AvgParse", "AvgOpt", "PG-ms", "StructSim", "CostRatio", "Score"
        )
        .ok();
        writeln!(out, "{}", "-".repeat(104)).ok();

        for cat in &self.categories {
            let pct = if cat.total == 0 {
                0.0
            } else {
                100.0 * cat.parsed as f64 / cat.total as f64
            };
            let pg_str = cat.avg_pg_explain_ms.map_or("-".to_owned(), |t| format!("{:.2}ms", t));
            let struct_str = cat.avg_structural_sim.map_or("-".to_owned(), |s| format!("{:.3}", s));
            let cost_str = cat.avg_cost_ratio.map_or("-".to_owned(), |c| format!("{:.3}", c));
            let score_str = cat.avg_score.map_or("-".to_owned(), |s| format!("{:.3}", s));
            writeln!(
                out,
                "{:<22} {:>6} {:>5.0}% {:>9.2}ms {:>9.2}ms {:>10} {:>8} {:>8} {:>8}",
                cat.category,
                cat.total,
                pct,
                cat.avg_parse_ms,
                cat.avg_optimize_ms,
                pg_str,
                struct_str,
                cost_str,
                score_str,
            )
            .ok();
        }

        writeln!(out, "{}", "-".repeat(104)).ok();
        let pct = if self.total_queries == 0 {
            0.0
        } else {
            100.0 * self.total_parsed as f64 / self.total_queries as f64
        };
        let score_str = self
            .overall_avg_score
            .map_or("-".to_owned(), |s| format!("{:.3}", s));
        writeln!(
            out,
            "{:<22} {:>6} {:>5.0}% {:>9.2}ms {:>9.2}ms {:>10} {:>8} {:>8} {:>8}",
            "TOTAL",
            self.total_queries,
            pct,
            self.overall_avg_parse_ms,
            self.overall_avg_optimize_ms,
            "-",
            "-",
            "-",
            score_str,
        )
        .ok();
        writeln!(out).ok();
    }
}

fn average<I: Iterator<Item = f64>>(iter: I) -> f64 {
    let (sum, count) = iter.fold((0.0_f64, 0usize), |(s, n), v| (s + v, n + 1));
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}
