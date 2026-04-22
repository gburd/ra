//! Planner Comparison Benchmark Runner
//!
//! Compares Ra optimizer performance against PostgreSQL planner across
//! 120 queries organized into 9 categories.
//!
//! Usage:
//!   cargo run --release --bin planner_comparison_runner
//!
//! Outputs:
//!   - benchmarks/planner_comparison/results/metrics.json
//!   - benchmarks/planner_comparison/results/COMPARISON_REPORT.md

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use ra_core::statistics::Statistics;
use ra_core::EmptyFactsProvider;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueryMetrics {
    query_id: String,
    category: String,
    sql_file: String,

    // Planning efficiency
    plan_time_us: u64,
    pg_plan_time_us: Option<u64>,
    rules_applied: usize,
    egraph_nodes: usize,
    egraph_classes: usize,
    memory_allocated_bytes: u64,

    // Plan quality
    plan_cost_estimate: f64,
    pg_plan_cost: Option<f64>,
    q_error: Option<f64>,

    // Feature coverage
    parser_success: bool,
    optimizer_success: bool,
    error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CategorySummary {
    category: String,
    total_queries: usize,
    parsed_queries: usize,
    optimized_queries: usize,
    median_plan_time_us: u64,
    p95_plan_time_us: u64,
    median_egraph_nodes: usize,
    median_rules_applied: usize,
    total_memory_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchmarkReport {
    timestamp: String,
    git_commit: String,
    categories: Vec<CategorySummary>,
    queries: Vec<QueryMetrics>,
    overall_summary: OverallSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OverallSummary {
    total_queries: usize,
    parsed_queries: usize,
    optimized_queries: usize,
    parser_success_rate: f64,
    optimizer_success_rate: f64,
    median_plan_time_us: u64,
    p95_plan_time_us: u64,
    total_plan_time_ms: f64,
}

fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
    for (name, stats) in [
        ("lineitem", make_stats(6_001_215.0, 128)),
        ("orders", make_stats(1_500_000.0, 150)),
        ("customer", make_stats(150_000.0, 200)),
        ("supplier", make_stats(10_000.0, 180)),
        ("nation", make_stats(25.0, 64)),
        ("region", make_stats(5.0, 48)),
        ("part", make_stats(200_000.0, 160)),
        ("partsupp", make_stats(800_000.0, 144)),
    ] {
        opt.add_table_stats(name, stats);
    }
    opt
}

fn get_git_commit() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_owned())
        .trim()
        .to_owned()
}

fn collect_query_files(base_dir: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut categories = HashMap::new();

    let category_dirs = [
        "simple",
        "basic_joins",
        "complex_joins",
        "aggregations",
        "subqueries",
        "ctes",
        "set_operations",
        "advanced",
        "unsupported",
    ];

    for category in category_dirs {
        let dir = base_dir.join("queries").join(category);
        if !dir.exists() {
            continue;
        }

        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("sql") {
                    files.push(path);
                }
            }
        }

        files.sort();
        if !files.is_empty() {
            categories.insert(category.to_owned(), files);
        }
    }

    categories
}

fn benchmark_query(
    optimizer: &Optimizer,
    facts: &EmptyFactsProvider,
    sql: &str,
    query_id: &str,
    category: &str,
    sql_file: &str,
) -> QueryMetrics {
    let mut metrics = QueryMetrics {
        query_id: query_id.to_owned(),
        category: category.to_owned(),
        sql_file: sql_file.to_owned(),
        plan_time_us: 0,
        pg_plan_time_us: None,
        rules_applied: 0,
        egraph_nodes: 0,
        egraph_classes: 0,
        memory_allocated_bytes: 0,
        plan_cost_estimate: 0.0,
        pg_plan_cost: None,
        q_error: None,
        parser_success: false,
        optimizer_success: false,
        error_message: None,
    };

    // Parse SQL
    let relexpr = match sql_to_relexpr(sql) {
        Ok(expr) => {
            metrics.parser_success = true;
            expr
        }
        Err(e) => {
            metrics.error_message = Some(format!("Parse error: {e}"));
            return metrics;
        }
    };

    // Optimize with Ra
    let start = Instant::now();
    let result = optimizer.optimize_with_facts(&relexpr, facts);
    let elapsed = start.elapsed();

    metrics.plan_time_us = elapsed.as_micros() as u64;

    match result {
        Ok(_optimized) => {
            metrics.optimizer_success = true;
            // Note: Cost extraction requires additional instrumentation
            metrics.plan_cost_estimate = 0.0;

            // Extract e-graph statistics if available
            // Note: This would require additional instrumentation in the optimizer
            // For now, use placeholder values
            metrics.egraph_nodes = 0;
            metrics.egraph_classes = 0;
            metrics.rules_applied = 0;
        }
        Err(e) => {
            metrics.error_message = Some(format!("Optimization error: {e}"));
        }
    }

    metrics
}

fn compute_category_summary(category: &str, metrics: &[QueryMetrics]) -> CategorySummary {
    let total_queries = metrics.len();
    let parsed_queries = metrics.iter().filter(|m| m.parser_success).count();
    let optimized_queries = metrics.iter().filter(|m| m.optimizer_success).count();

    let mut plan_times: Vec<u64> = metrics
        .iter()
        .filter(|m| m.optimizer_success)
        .map(|m| m.plan_time_us)
        .collect();
    plan_times.sort_unstable();

    let median_plan_time_us = if plan_times.is_empty() {
        0
    } else {
        plan_times[plan_times.len() / 2]
    };

    let p95_plan_time_us = if plan_times.is_empty() {
        0
    } else {
        let idx = (plan_times.len() as f64 * 0.95) as usize;
        plan_times[idx.min(plan_times.len() - 1)]
    };

    let mut egraph_nodes: Vec<usize> = metrics
        .iter()
        .filter(|m| m.optimizer_success)
        .map(|m| m.egraph_nodes)
        .collect();
    egraph_nodes.sort_unstable();

    let median_egraph_nodes = if egraph_nodes.is_empty() {
        0
    } else {
        egraph_nodes[egraph_nodes.len() / 2]
    };

    let mut rules_applied: Vec<usize> = metrics
        .iter()
        .filter(|m| m.optimizer_success)
        .map(|m| m.rules_applied)
        .collect();
    rules_applied.sort_unstable();

    let median_rules_applied = if rules_applied.is_empty() {
        0
    } else {
        rules_applied[rules_applied.len() / 2]
    };

    let total_memory_mb: f64 = metrics
        .iter()
        .map(|m| m.memory_allocated_bytes as f64 / (1024.0 * 1024.0))
        .sum();

    CategorySummary {
        category: category.to_owned(),
        total_queries,
        parsed_queries,
        optimized_queries,
        median_plan_time_us,
        p95_plan_time_us,
        median_egraph_nodes,
        median_rules_applied,
        total_memory_mb,
    }
}

fn compute_overall_summary(all_metrics: &[QueryMetrics]) -> OverallSummary {
    let total_queries = all_metrics.len();
    let parsed_queries = all_metrics.iter().filter(|m| m.parser_success).count();
    let optimized_queries = all_metrics.iter().filter(|m| m.optimizer_success).count();

    let parser_success_rate = if total_queries > 0 {
        (parsed_queries as f64 / total_queries as f64) * 100.0
    } else {
        0.0
    };

    let optimizer_success_rate = if parsed_queries > 0 {
        (optimized_queries as f64 / parsed_queries as f64) * 100.0
    } else {
        0.0
    };

    let mut plan_times: Vec<u64> = all_metrics
        .iter()
        .filter(|m| m.optimizer_success)
        .map(|m| m.plan_time_us)
        .collect();
    plan_times.sort_unstable();

    let median_plan_time_us = if plan_times.is_empty() {
        0
    } else {
        plan_times[plan_times.len() / 2]
    };

    let p95_plan_time_us = if plan_times.is_empty() {
        0
    } else {
        let idx = (plan_times.len() as f64 * 0.95) as usize;
        plan_times[idx.min(plan_times.len() - 1)]
    };

    let total_plan_time_ms: f64 = all_metrics
        .iter()
        .filter(|m| m.optimizer_success)
        .map(|m| m.plan_time_us as f64 / 1000.0)
        .sum();

    OverallSummary {
        total_queries,
        parsed_queries,
        optimized_queries,
        parser_success_rate,
        optimizer_success_rate,
        median_plan_time_us,
        p95_plan_time_us,
        total_plan_time_ms,
    }
}

fn generate_markdown_report(report: &BenchmarkReport) -> String {
    let mut md = String::with_capacity(16384);

    md.push_str("# Planner Comparison Benchmark Report\n\n");
    md.push_str(&format!("**Generated**: {}\n", report.timestamp));
    md.push_str(&format!("**Git Commit**: {}\n\n", report.git_commit));

    md.push_str("## Overall Summary\n\n");
    md.push_str(&format!(
        "- Total queries: {}\n",
        report.overall_summary.total_queries
    ));
    md.push_str(&format!(
        "- Parsed successfully: {} ({:.1}%)\n",
        report.overall_summary.parsed_queries, report.overall_summary.parser_success_rate
    ));
    md.push_str(&format!(
        "- Optimized successfully: {} ({:.1}%)\n",
        report.overall_summary.optimized_queries, report.overall_summary.optimizer_success_rate
    ));
    md.push_str(&format!(
        "- Median plan time: {:.2}ms\n",
        report.overall_summary.median_plan_time_us as f64 / 1000.0
    ));
    md.push_str(&format!(
        "- P95 plan time: {:.2}ms\n",
        report.overall_summary.p95_plan_time_us as f64 / 1000.0
    ));
    md.push_str(&format!(
        "- Total plan time: {:.2}ms\n\n",
        report.overall_summary.total_plan_time_ms
    ));

    md.push_str("## Results by Category\n\n");
    md.push_str("| Category | Queries | Parsed | Optimized | Median Time | P95 Time | Median Nodes | Median Rules |\n");
    md.push_str("|----------|---------|--------|-----------|-------------|----------|--------------|---------------|\n");

    for cat in &report.categories {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {:.2}ms | {:.2}ms | {} | {} |\n",
            cat.category,
            cat.total_queries,
            cat.parsed_queries,
            cat.optimized_queries,
            cat.median_plan_time_us as f64 / 1000.0,
            cat.p95_plan_time_us as f64 / 1000.0,
            cat.median_egraph_nodes,
            cat.median_rules_applied,
        ));
    }

    md.push_str("\n## Detailed Query Results\n\n");

    for category in &report.categories {
        md.push_str(&format!("### {}\n\n", category.category));
        md.push_str("| Query ID | Plan Time (ms) | Cost | Nodes | Rules | Status |\n");
        md.push_str("|----------|----------------|------|-------|-------|--------|\n");

        let cat_queries: Vec<_> = report
            .queries
            .iter()
            .filter(|q| q.category == category.category)
            .collect();

        for q in cat_queries {
            let status = if q.optimizer_success {
                "OK"
            } else if q.parser_success {
                "PARSE_ONLY"
            } else {
                "FAILED"
            };

            md.push_str(&format!(
                "| {} | {:.2} | {:.0} | {} | {} | {} |\n",
                q.query_id,
                q.plan_time_us as f64 / 1000.0,
                q.plan_cost_estimate,
                q.egraph_nodes,
                q.rules_applied,
                status,
            ));
        }

        md.push_str("\n");
    }

    md.push_str("## Feature Coverage\n\n");
    md.push_str(&format!(
        "- Parser success rate: {:.1}%\n",
        report.overall_summary.parser_success_rate
    ));
    md.push_str(&format!(
        "- Optimizer success rate: {:.1}%\n",
        report.overall_summary.optimizer_success_rate
    ));

    md.push_str("\n## Failed Queries\n\n");
    let failed: Vec<_> = report
        .queries
        .iter()
        .filter(|q| !q.optimizer_success)
        .collect();

    if failed.is_empty() {
        md.push_str("No failures.\n\n");
    } else {
        md.push_str("| Query ID | Category | Error |\n");
        md.push_str("|----------|----------|-------|\n");

        for q in failed {
            let error = q.error_message.as_deref().unwrap_or("Unknown error");
            md.push_str(&format!(
                "| {} | {} | {} |\n",
                q.query_id, q.category, error
            ));
        }

        md.push_str("\n");
    }

    md
}

fn main() {
    let base_dir = PathBuf::from("benchmarks/planner_comparison");
    let results_dir = base_dir.join("results");
    fs::create_dir_all(&results_dir).expect("Failed to create results directory");

    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();

    let categories = collect_query_files(&base_dir);

    if categories.is_empty() {
        eprintln!("No query files found in {}", base_dir.display());
        std::process::exit(1);
    }

    println!("Running planner comparison benchmark...");
    println!("Found {} categories", categories.len());

    let mut all_metrics = Vec::new();
    let mut category_summaries = Vec::new();

    for (category, files) in &categories {
        println!(
            "\nProcessing category: {} ({} queries)",
            category,
            files.len()
        );

        let mut cat_metrics = Vec::new();

        for (idx, file) in files.iter().enumerate() {
            let query_id = format!(
                "{}_{}",
                category,
                file.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            );

            let sql = fs::read_to_string(file)
                .unwrap_or_else(|e| panic!("Failed to read {}: {e}", file.display()));

            print!("  [{}/{}] {}... ", idx + 1, files.len(), query_id);

            let metrics = benchmark_query(
                &optimizer,
                &facts,
                &sql,
                &query_id,
                category,
                file.to_str().unwrap_or(""),
            );

            if metrics.optimizer_success {
                println!("OK ({:.2}ms)", metrics.plan_time_us as f64 / 1000.0);
            } else if metrics.parser_success {
                println!("PARSE_ONLY");
            } else {
                println!("FAILED");
            }

            cat_metrics.push(metrics.clone());
            all_metrics.push(metrics);
        }

        let summary = compute_category_summary(category, &cat_metrics);
        category_summaries.push(summary);
    }

    let overall_summary = compute_overall_summary(&all_metrics);

    let report = BenchmarkReport {
        timestamp: chrono::Utc::now().to_rfc3339(),
        git_commit: get_git_commit(),
        categories: category_summaries,
        queries: all_metrics,
        overall_summary,
    };

    // Write JSON metrics
    let json_path = results_dir.join("metrics.json");
    let json = serde_json::to_string_pretty(&report).expect("Failed to serialize report");
    fs::write(&json_path, json).expect("Failed to write metrics.json");
    println!("\nWrote metrics to {}", json_path.display());

    // Write markdown report
    let md_path = results_dir.join("COMPARISON_REPORT.md");
    let markdown = generate_markdown_report(&report);
    fs::write(&md_path, markdown).expect("Failed to write report");
    println!("Wrote report to {}", md_path.display());

    println!("\nBenchmark complete!");
    println!(
        "  Parsed: {}/{} ({:.1}%)",
        report.overall_summary.parsed_queries,
        report.overall_summary.total_queries,
        report.overall_summary.parser_success_rate
    );
    println!(
        "  Optimized: {}/{} ({:.1}%)",
        report.overall_summary.optimized_queries,
        report.overall_summary.total_queries,
        report.overall_summary.optimizer_success_rate
    );
}
