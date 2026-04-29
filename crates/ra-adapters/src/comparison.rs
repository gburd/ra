//! Query performance comparison between native database and Ra-optimized execution.

#[cfg(any(feature = "postgres", feature = "mysql"))]
use crate::AdapterError;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// Comparison result for native vs Ra-optimized execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    /// Query text.
    pub query: String,
    /// Native `PostgreSQL` execution metrics.
    pub native: ExecutionMetrics,
    /// Ra-optimized execution metrics.
    pub ra: ExecutionMetrics,
    /// Speedup ratio (`native_time` / `ra_time`).
    pub speedup: f64,
    /// Performance improvement percentage.
    pub improvement_pct: f64,
}

/// Execution metrics for a single query run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    /// Execution time in milliseconds.
    pub execution_time_ms: u64,
    /// Number of rows returned.
    pub rows_returned: usize,
    /// Number of rows scanned (from EXPLAIN).
    pub rows_scanned: Option<u64>,
    /// Index usage information.
    pub index_usage: Vec<String>,
    /// Cost estimates from EXPLAIN.
    pub cost_estimate: Option<f64>,
    /// Planning time in milliseconds.
    pub planning_time_ms: Option<f64>,
}

impl ExecutionMetrics {
    /// Create metrics from `PostgreSQL` execution result.
    #[cfg(feature = "postgres")]
    #[must_use]
    pub fn from_postgres_result(result: &crate::postgres::ExecutionResult) -> Self {
        Self {
            execution_time_ms: result.execution_time_ms,
            rows_returned: result.row_count,
            rows_scanned: None,
            index_usage: Vec::new(),
            cost_estimate: None,
            planning_time_ms: None,
        }
    }

    /// Create metrics from `MySQL` execution result.
    #[cfg(feature = "mysql")]
    #[must_use]
    pub fn from_mysql_result(result: &crate::mysql::ExecutionResult) -> Self {
        Self {
            execution_time_ms: result.duration.as_millis() as u64,
            rows_returned: result.row_count,
            rows_scanned: None,
            index_usage: Vec::new(),
            cost_estimate: None,
            planning_time_ms: None,
        }
    }

    /// Enhance metrics with EXPLAIN plan data.
    pub fn with_plan(&mut self, plan: &serde_json::Value) {
        if let Some(plan_array) = plan.as_array() {
            if let Some(plan_obj) = plan_array.first() {
                if let Some(plan_data) = plan_obj.get("Plan") {
                    self.extract_plan_metrics(plan_data);
                }

                if let Some(planning) = plan_obj.get("Planning Time") {
                    self.planning_time_ms = planning.as_f64();
                }
            }
        }
    }

    fn extract_plan_metrics(&mut self, plan: &serde_json::Value) {
        if let Some(rows) = plan.get("Actual Rows") {
            self.rows_scanned = rows.as_u64();
        }

        if let Some(cost) = plan.get("Total Cost") {
            self.cost_estimate = cost.as_f64();
        }

        if let Some(node_type) = plan.get("Node Type") {
            if let Some(node_str) = node_type.as_str() {
                if node_str.contains("Index") {
                    if let Some(index_name) = plan.get("Index Name") {
                        if let Some(name) = index_name.as_str() {
                            self.index_usage.push(name.to_string());
                        }
                    }
                }
            }
        }

        if let Some(plans) = plan.get("Plans") {
            if let Some(plans_array) = plans.as_array() {
                for child_plan in plans_array {
                    self.extract_plan_metrics(child_plan);
                }
            }
        }
    }

    /// Enhance metrics with `MySQL` EXPLAIN plan data.
    #[cfg(feature = "mysql")]
    pub fn with_mysql_plan(&mut self, plan: &crate::mysql::ExplainPlan) {
        if let Some(query_block) = plan.json.get("query_block") {
            self.extract_mysql_plan_metrics(query_block);
        }
    }

    #[cfg(feature = "mysql")]
    fn extract_mysql_plan_metrics(&mut self, plan: &serde_json::Value) {
        if let Some(rows) = plan.get("rows_examined_per_scan") {
            self.rows_scanned = rows.as_u64();
        }

        if let Some(cost) = plan.get("cost_info").and_then(|c| c.get("query_cost")) {
            self.cost_estimate = cost.as_f64();
        }

        if let Some(table) = plan.get("table") {
            if let Some(key) = table.get("key") {
                if let Some(key_str) = key.as_str() {
                    self.index_usage.push(key_str.to_string());
                }
            }

            if let Some(possible_keys) = table.get("possible_keys") {
                if let Some(keys_array) = possible_keys.as_array() {
                    for key in keys_array {
                        if let Some(key_str) = key.as_str() {
                            if !self.index_usage.contains(&key_str.to_string()) {
                                self.index_usage.push(key_str.to_string());
                            }
                        }
                    }
                }
            }
        }

        if let Some(nested_loop) = plan.get("nested_loop") {
            if let Some(tables) = nested_loop.as_array() {
                for table in tables {
                    self.extract_mysql_plan_metrics(table);
                }
            }
        }
    }
}

impl ComparisonResult {
    /// Create a comparison result from native and Ra metrics.
    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "timing values fit in f64 mantissa")]
    pub fn new(query: String, native: ExecutionMetrics, ra: ExecutionMetrics) -> Self {
        let speedup = if ra.execution_time_ms > 0 {
            native.execution_time_ms as f64 / ra.execution_time_ms as f64
        } else {
            0.0
        };

        let improvement_pct = if native.execution_time_ms > 0 {
            ((native.execution_time_ms as f64 - ra.execution_time_ms as f64)
                / native.execution_time_ms as f64)
                * 100.0
        } else {
            0.0
        };

        Self {
            query,
            native,
            ra,
            speedup,
            improvement_pct,
        }
    }

    /// Check if Ra optimization improved performance.
    #[must_use]
    pub fn is_improved(&self) -> bool {
        self.speedup > 1.0
    }

    /// Check if improvement is significant (>10%).
    #[must_use]
    pub fn is_significant(&self) -> bool {
        self.improvement_pct > 10.0
    }
}

/// Detailed comparison report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// Timestamp of report generation.
    pub timestamp: String,
    /// Total queries compared.
    pub total_queries: usize,
    /// Number of queries with improvements.
    pub improved_queries: usize,
    /// Number of queries with regressions.
    pub regressed_queries: usize,
    /// Average speedup across all queries.
    pub avg_speedup: f64,
    /// Median speedup.
    pub median_speedup: f64,
    /// Maximum speedup.
    pub max_speedup: f64,
    /// Minimum speedup.
    pub min_speedup: f64,
    /// Individual query results.
    pub results: Vec<ComparisonResult>,
}

impl ComparisonReport {
    /// Create a report from comparison results.
    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "query counts fit in f64 mantissa")]
    pub fn new(results: Vec<ComparisonResult>) -> Self {
        let total_queries = results.len();
        let improved_queries = results.iter().filter(|r| r.speedup > 1.0).count();
        let regressed_queries = results.iter().filter(|r| r.speedup < 1.0).count();

        let speedups: Vec<f64> = results.iter().map(|r| r.speedup).collect();
        let avg_speedup = if speedups.is_empty() {
            0.0
        } else {
            speedups.iter().sum::<f64>() / speedups.len() as f64
        };

        let mut sorted_speedups = speedups.clone();
        sorted_speedups.sort_by(f64::total_cmp);
        let median_speedup = if sorted_speedups.is_empty() {
            0.0
        } else {
            sorted_speedups[sorted_speedups.len() / 2]
        };

        let max_speedup = speedups
            .iter()
            .max_by(|a, b| a.total_cmp(b))
            .copied()
            .unwrap_or(0.0);

        let min_speedup = speedups
            .iter()
            .min_by(|a, b| a.total_cmp(b))
            .copied()
            .unwrap_or(0.0);

        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            total_queries,
            improved_queries,
            regressed_queries,
            avg_speedup,
            median_speedup,
            max_speedup,
            min_speedup,
            results,
        }
    }

    /// Generate a JSON report.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the report cannot be converted to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Generate a Markdown report.
    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "query counts fit in f64 mantissa")]
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# PostgreSQL vs Ra Performance Comparison\n\n");
        let _ = writeln!(md, "**Generated:** {}\n", self.timestamp);

        md.push_str("## Summary\n\n");
        let _ = writeln!(md, "- **Total Queries:** {}", self.total_queries);
        let _ = writeln!(
            md,
            "- **Improved:** {} ({:.1}%)",
            self.improved_queries,
            (self.improved_queries as f64 / self.total_queries as f64) * 100.0
        );
        let _ = writeln!(
            md,
            "- **Regressed:** {} ({:.1}%)",
            self.regressed_queries,
            (self.regressed_queries as f64 / self.total_queries as f64) * 100.0
        );
        let _ = writeln!(md, "- **Average Speedup:** {:.2}x", self.avg_speedup);
        let _ = writeln!(md, "- **Median Speedup:** {:.2}x", self.median_speedup);
        let _ = writeln!(md, "- **Max Speedup:** {:.2}x", self.max_speedup);
        let _ = writeln!(md, "- **Min Speedup:** {:.2}x\n", self.min_speedup);

        md.push_str("## Detailed Results\n\n");
        md.push_str("| Query | Native (ms) | Ra (ms) | Speedup | Improvement |\n");
        md.push_str("|-------|-------------|---------|---------|-------------|\n");

        for result in &self.results {
            let query_preview = if result.query.len() > 50 {
                format!("{}...", &result.query[..47])
            } else {
                result.query.clone()
            };

            let _ = writeln!(
                md,
                "| {query_preview} | {} | {} | {:.2}x | {:.1}% |",
                result.native.execution_time_ms,
                result.ra.execution_time_ms,
                result.speedup,
                result.improvement_pct
            );
        }

        md.push('\n');
        md
    }
}

/// Compare queries between native `PostgreSQL` and Ra-optimized execution.
#[cfg(feature = "postgres")]
pub fn compare_queries(
    adapter: &crate::postgres::PostgresAdapter,
    queries: &[String],
) -> Result<ComparisonReport, AdapterError> {
    let mut results = Vec::new();

    for query in queries {
        let result = compare_single_query(adapter, query)?;
        results.push(result);
    }

    Ok(ComparisonReport::new(results))
}

/// Compare a single query between native `PostgreSQL` and Ra execution.
#[cfg(feature = "postgres")]
pub fn compare_single_query(
    adapter: &crate::postgres::PostgresAdapter,
    query: &str,
) -> Result<ComparisonResult, AdapterError> {
    let native_result = adapter.execute_native(query)?;
    let mut native_metrics = ExecutionMetrics::from_postgres_result(&native_result);

    let native_plan = adapter.get_explain_plan(query)?;
    native_metrics.with_plan(&native_plan);

    let ra_result = adapter.execute_with_ra(query)?;
    let mut ra_metrics = ExecutionMetrics::from_postgres_result(&ra_result);

    if let Some(ref plan) = ra_result.plan {
        ra_metrics.with_plan(plan);
    }

    Ok(ComparisonResult::new(
        query.to_string(),
        native_metrics,
        ra_metrics,
    ))
}

/// Compare queries between native `MySQL` and Ra-optimized execution.
#[cfg(feature = "mysql")]
pub fn compare_mysql_queries(
    adapter: &crate::mysql::MySQLAdapter,
    queries: &[String],
) -> Result<ComparisonReport, AdapterError> {
    let mut results = Vec::new();

    for query in queries {
        let result = compare_single_mysql_query(adapter, query)?;
        results.push(result);
    }

    Ok(ComparisonReport::new(results))
}

/// Compare a single query between native `MySQL` and Ra execution.
#[cfg(feature = "mysql")]
pub fn compare_single_mysql_query(
    adapter: &crate::mysql::MySQLAdapter,
    query: &str,
) -> Result<ComparisonResult, AdapterError> {
    let native_result = adapter.execute_native(query)?;
    let mut native_metrics = ExecutionMetrics::from_mysql_result(&native_result);

    let native_plan = adapter.get_explain_plan(query)?;
    native_metrics.with_mysql_plan(&native_plan);

    let ra_result = adapter.execute_with_ra(query)?;
    let ra_metrics = ExecutionMetrics::from_mysql_result(&ra_result);

    Ok(ComparisonResult::new(
        query.to_string(),
        native_metrics,
        ra_metrics,
    ))
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "exact float literals in tests")]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "postgres")]
    fn execution_metrics_from_postgres_result() {
        let result = crate::postgres::ExecutionResult {
            rows: vec![],
            row_count: 100,
            execution_time_ms: 50,
            plan: None,
        };

        let metrics = ExecutionMetrics::from_postgres_result(&result);
        assert_eq!(metrics.execution_time_ms, 50);
        assert_eq!(metrics.rows_returned, 100);
        assert!(metrics.rows_scanned.is_none());
    }

    #[test]
    fn comparison_result_speedup() {
        let native = ExecutionMetrics {
            execution_time_ms: 100,
            rows_returned: 50,
            rows_scanned: None,
            index_usage: vec![],
            cost_estimate: None,
            planning_time_ms: None,
        };

        let ra = ExecutionMetrics {
            execution_time_ms: 50,
            rows_returned: 50,
            rows_scanned: None,
            index_usage: vec![],
            cost_estimate: None,
            planning_time_ms: None,
        };

        let result = ComparisonResult::new("SELECT 1".to_string(), native, ra);
        assert_eq!(result.speedup, 2.0);
        assert_eq!(result.improvement_pct, 50.0);
        assert!(result.is_improved());
        assert!(result.is_significant());
    }

    #[test]
    fn comparison_result_regression() {
        let native = ExecutionMetrics {
            execution_time_ms: 50,
            rows_returned: 50,
            rows_scanned: None,
            index_usage: vec![],
            cost_estimate: None,
            planning_time_ms: None,
        };

        let ra = ExecutionMetrics {
            execution_time_ms: 100,
            rows_returned: 50,
            rows_scanned: None,
            index_usage: vec![],
            cost_estimate: None,
            planning_time_ms: None,
        };

        let result = ComparisonResult::new("SELECT 1".to_string(), native, ra);
        assert_eq!(result.speedup, 0.5);
        assert!(result.improvement_pct < 0.0);
        assert!(!result.is_improved());
    }

    #[test]
    fn comparison_report_statistics() {
        let results = vec![
            ComparisonResult {
                query: "Q1".to_string(),
                native: ExecutionMetrics {
                    execution_time_ms: 100,
                    rows_returned: 10,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                ra: ExecutionMetrics {
                    execution_time_ms: 50,
                    rows_returned: 10,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                speedup: 2.0,
                improvement_pct: 50.0,
            },
            ComparisonResult {
                query: "Q2".to_string(),
                native: ExecutionMetrics {
                    execution_time_ms: 200,
                    rows_returned: 20,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                ra: ExecutionMetrics {
                    execution_time_ms: 100,
                    rows_returned: 20,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                speedup: 2.0,
                improvement_pct: 50.0,
            },
        ];

        let report = ComparisonReport::new(results);
        assert_eq!(report.total_queries, 2);
        assert_eq!(report.improved_queries, 2);
        assert_eq!(report.regressed_queries, 0);
        assert_eq!(report.avg_speedup, 2.0);
    }

    #[test]
    fn markdown_report_generation() {
        let results = vec![ComparisonResult {
            query: "SELECT * FROM users".to_string(),
            native: ExecutionMetrics {
                execution_time_ms: 100,
                rows_returned: 50,
                rows_scanned: None,
                index_usage: vec![],
                cost_estimate: None,
                planning_time_ms: None,
            },
            ra: ExecutionMetrics {
                execution_time_ms: 50,
                rows_returned: 50,
                rows_scanned: None,
                index_usage: vec![],
                cost_estimate: None,
                planning_time_ms: None,
            },
            speedup: 2.0,
            improvement_pct: 50.0,
        }];

        let report = ComparisonReport::new(results);
        let markdown = report.to_markdown();

        assert!(markdown.contains("# PostgreSQL vs Ra Performance Comparison"));
        assert!(markdown.contains("Total Queries"));
        assert!(markdown.contains("Average Speedup"));
        assert!(markdown.contains("| Query | Native (ms) | Ra (ms)"));
    }
}
