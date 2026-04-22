//! CLI commands for timeline-based optimization.
//!
//! Provides timeline command for optimizing queries through
//! evolving database fingerprints with different output formats,
//! test validation, and TUI visualization.

#![allow(clippy::print_stderr)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::too_many_lines)]

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;
use colored::Colorize;
use regex::Regex;

use ra_engine::{Optimizer, OptimizerConfig, TimelineConfig};
use ra_hardware::HardwareProfile;
use ra_parser::sql_to_relexpr;

use crate::display::format_plan_tree;

/// Timeline command for optimizing queries through evolving fingerprints.
#[derive(Parser)]
pub struct TimelineCommand {
    /// Path to timeline configuration file.
    #[arg(long)]
    pub timeline: PathBuf,

    /// Output format: text, json, markdown, toml.
    #[arg(long, default_value = "text")]
    pub output: String,

    /// Test mode - validate expectations and exit with pass/fail.
    #[arg(long)]
    pub test: bool,

    /// Launch TUI visualization.
    #[arg(long)]
    pub tui: bool,

    /// Filter snapshots by comma-separated indices (e.g., "0,2,5").
    #[arg(long)]
    pub snapshots: Option<String>,

    /// Verbose output - show detailed rule logging.
    #[arg(short, long)]
    pub verbose: bool,
}

/// Output format for timeline command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text with ASCII tables.
    Text,
    /// Machine-readable JSON.
    Json,
    /// Report-style markdown.
    Markdown,
    /// Config-like TOML.
    Toml,
}

impl OutputFormat {
    fn from_str_arg(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "markdown" | "md" => Ok(Self::Markdown),
            "toml" => Ok(Self::Toml),
            other => bail!("unknown format: {other}\nvalid formats: text, json, markdown, toml"),
        }
    }
}

/// Result of optimizing a single snapshot.
#[derive(Debug, Clone)]
struct SnapshotResult {
    snapshot_index: usize,
    label: String,
    time_offset: u64,
    hardware_profile: String,
    initial_cost: f64,
    optimized_cost: f64,
    cost_reduction_pct: f64,
    plan: String,
    rules_applied: Vec<String>,
    optimization_time_ms: u64,
}

/// Result of validating a test expectation.
#[derive(Debug, Clone)]
struct ExpectationResult {
    passed: bool,
    failures: Vec<String>,
}

/// Execute the timeline command.
pub fn cmd_timeline(cmd: &TimelineCommand, quiet: bool) -> Result<()> {
    let format = OutputFormat::from_str_arg(&cmd.output)?;

    // Load timeline configuration
    let timeline = TimelineConfig::from_file(&cmd.timeline)
        .with_context(|| format!("loading timeline: {}", cmd.timeline.display()))?;

    // Parse snapshot filter
    let snapshot_filter = if let Some(ref filter_str) = cmd.snapshots {
        parse_snapshot_filter(filter_str)?
    } else {
        None
    };

    // TUI mode
    if cmd.tui {
        return launch_timeline_tui(&timeline, quiet);
    }

    // Get query from timeline metadata
    let query = timeline.metadata.query.as_ref().ok_or_else(|| {
        anyhow::anyhow!("timeline metadata must specify a query for optimization")
    })?;

    // Test mode - validate expectations
    if cmd.test {
        return run_test_mode(&timeline, query, &snapshot_filter, cmd.verbose, quiet);
    }

    // Normal mode - optimize and output results
    run_optimize_mode(
        &timeline,
        query,
        &snapshot_filter,
        format,
        cmd.verbose,
        quiet,
    )
}

/// Parse snapshot filter string into set of indices.
fn parse_snapshot_filter(filter_str: &str) -> Result<Option<Vec<usize>>> {
    let indices: Result<Vec<usize>> = filter_str
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<usize>()
                .with_context(|| format!("invalid snapshot index: {s}"))
        })
        .collect();
    Ok(Some(indices?))
}

/// Run optimization mode - optimize query at each snapshot and output results.
fn run_optimize_mode(
    timeline: &TimelineConfig,
    query: &str,
    snapshot_filter: &Option<Vec<usize>>,
    format: OutputFormat,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let results = optimize_snapshots(timeline, query, snapshot_filter, verbose, quiet)?;

    match format {
        OutputFormat::Text => output_text(&timeline.metadata.name, &results),
        OutputFormat::Json => output_json(&timeline.metadata.name, &results),
        OutputFormat::Markdown => output_markdown(&timeline.metadata.name, &results),
        OutputFormat::Toml => output_toml(&timeline.metadata.name, &results),
    }
}

/// Run test mode - validate expectations and report pass/fail.
fn run_test_mode(
    timeline: &TimelineConfig,
    query: &str,
    snapshot_filter: &Option<Vec<usize>>,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    if timeline.expectations.is_empty() {
        bail!("timeline has no test expectations to validate");
    }

    if !quiet {
        eprintln!("{}", "Running timeline tests...".bold());
        eprintln!();
    }

    let results = optimize_snapshots(timeline, query, snapshot_filter, verbose, quiet)?;
    let mut all_passed = true;
    let mut test_results = Vec::new();

    for expectation in &timeline.expectations {
        // Skip if filtered out
        if let Some(ref filter) = snapshot_filter {
            if !filter.contains(&expectation.snapshot_index) {
                continue;
            }
        }

        let result = results
            .iter()
            .find(|r| r.snapshot_index == expectation.snapshot_index);

        let Some(result) = result else {
            if !quiet {
                eprintln!(
                    "  {} Snapshot {} - no optimization result",
                    "SKIP".yellow(),
                    expectation.snapshot_index
                );
            }
            continue;
        };

        let validation = validate_expectation(expectation, result)?;

        if !quiet {
            let status = if validation.passed {
                "PASS".green()
            } else {
                "FAIL".red()
            };
            eprintln!("  {} Snapshot {}", status, expectation.snapshot_index);

            if !validation.failures.is_empty() {
                for failure in &validation.failures {
                    eprintln!("       {}", failure.red());
                }
            }
        }

        if !validation.passed {
            all_passed = false;
        }

        test_results.push(validation);
    }

    if !quiet {
        eprintln!();
        let passed = test_results.iter().filter(|r| r.passed).count();
        let failed = test_results.iter().filter(|r| !r.passed).count();

        if all_passed {
            eprintln!("{} {} passed", "Success:".green().bold(), passed);
        } else {
            eprintln!("{} {} passed, {} failed", "Results:".bold(), passed, failed);
        }
    }

    if all_passed {
        Ok(())
    } else {
        bail!("timeline tests failed")
    }
}

/// Optimize query at each snapshot.
fn optimize_snapshots(
    timeline: &TimelineConfig,
    query: &str,
    snapshot_filter: &Option<Vec<usize>>,
    verbose: bool,
    quiet: bool,
) -> Result<Vec<SnapshotResult>> {
    let mut results = Vec::new();

    for (idx, snapshot) in timeline.snapshots.iter().enumerate() {
        // Skip if filtered out
        if let Some(ref filter) = snapshot_filter {
            if !filter.contains(&idx) {
                continue;
            }
        }

        if !quiet && verbose {
            eprintln!(
                "Optimizing snapshot {} ({})...",
                idx,
                snapshot.label.as_deref().unwrap_or("unlabeled")
            );
        }

        let result = optimize_single_snapshot(timeline, snapshot, idx, query, verbose)?;
        results.push(result);
    }

    Ok(results)
}

/// Optimize query at a single snapshot.
fn optimize_single_snapshot(
    _timeline: &TimelineConfig,
    snapshot: &ra_engine::FingerPrintSnapshot,
    snapshot_index: usize,
    query: &str,
    _verbose: bool,
) -> Result<SnapshotResult> {
    use std::time::Instant;

    // Get hardware profile - use predefined profiles for now
    // TODO: Map HardwareProfileDef to HardwareProfile properly
    let hardware = match snapshot.hardware_profile.to_lowercase().as_str() {
        "laptop" | "mobile" | "desktop" => HardwareProfile::cpu_only(),
        "server" | "cloud" => HardwareProfile::cpu_only(),
        "gpu-server" | "gpu" => HardwareProfile::gpu_server(),
        _ => HardwareProfile::cpu_only(),
    };

    // Parse SQL to RelExpr
    let rel_expr = sql_to_relexpr(query)?;

    // Create optimizer with snapshot facts
    // TODO: Integrate SnapshotFactsProvider when API is available
    let mut optimizer_config = OptimizerConfig::default();
    optimizer_config.iter_limit = 20;
    optimizer_config.node_limit = 100_000;

    let mut optimizer = Optimizer::with_config(optimizer_config);
    optimizer.set_hardware_profile(hardware.clone());

    // Estimate initial cost - use simple heuristic for now
    let initial_cost = 1000.0; // TODO: Improve cost estimation

    // Optimize with tracking
    let start = Instant::now();
    let opt_result = optimizer.optimize_with_tracking(&rel_expr)?;
    let optimization_time_ms = start.elapsed().as_millis() as u64;

    // Calculate cost reduction
    let cost_reduction_pct = if initial_cost > 0.0 {
        ((initial_cost - opt_result.cost) / initial_cost) * 100.0
    } else {
        0.0
    };

    // Get applied rules
    let rules_applied = if let Some(tracking) = opt_result.rule_tracking {
        tracking.applied.iter().map(|r| r.name.clone()).collect()
    } else {
        Vec::new()
    };

    // Format plan
    let plan = format_plan_tree(&opt_result.plan);

    Ok(SnapshotResult {
        snapshot_index,
        label: snapshot
            .label
            .clone()
            .unwrap_or_else(|| format!("Snapshot {snapshot_index}")),
        time_offset: snapshot.time_offset,
        hardware_profile: snapshot.hardware_profile.clone(),
        initial_cost,
        optimized_cost: opt_result.cost,
        cost_reduction_pct,
        plan,
        rules_applied,
        optimization_time_ms,
    })
}

/// Validate a test expectation against optimization result.
fn validate_expectation(
    expectation: &ra_engine::TestExpectation,
    result: &SnapshotResult,
) -> Result<ExpectationResult> {
    let mut failures = Vec::new();

    // Validate plan pattern
    if let Some(ref pattern) = expectation.expected_plan_pattern {
        let regex =
            Regex::new(pattern).with_context(|| format!("invalid regex pattern: {pattern}"))?;
        if !regex.is_match(&result.plan) {
            failures.push(format!("plan does not match pattern: {pattern}"));
        }
    }

    // Validate cost range
    if let Some([min_cost, max_cost]) = expectation.expected_cost_range {
        if result.optimized_cost < min_cost || result.optimized_cost > max_cost {
            failures.push(format!(
                "cost {:.2} outside range [{:.2}, {:.2}]",
                result.optimized_cost, min_cost, max_cost
            ));
        }
    }

    // Validate rules that must be applied
    for rule in &expectation.rules_applied_must_include {
        if !result.rules_applied.contains(rule) {
            failures.push(format!("required rule not applied: {rule}"));
        }
    }

    // Validate rules that must not be applied
    for rule in &expectation.rules_applied_must_not_include {
        if result.rules_applied.contains(rule) {
            failures.push(format!("forbidden rule was applied: {rule}"));
        }
    }

    Ok(ExpectationResult {
        passed: failures.is_empty(),
        failures,
    })
}

/// Output results in text format.
fn output_text(timeline_name: &str, results: &[SnapshotResult]) -> Result<()> {
    println!("{}", timeline_name.bold().underline());
    println!();

    for result in results {
        println!(
            "{}",
            format!("Snapshot {} - {}", result.snapshot_index, result.label).bold()
        );
        println!("  Time Offset:     {}s", result.time_offset);
        println!("  Hardware:        {}", result.hardware_profile);
        println!("  Initial Cost:    {:.2}", result.initial_cost);
        println!("  Optimized Cost:  {:.2}", result.optimized_cost);
        println!("  Cost Reduction:  {:.2}%", result.cost_reduction_pct);
        println!("  Optimization:    {}ms", result.optimization_time_ms);

        if !result.rules_applied.is_empty() {
            println!("  Rules Applied:   {}", result.rules_applied.join(", "));
        }

        println!();
        println!("  Plan:");
        for line in result.plan.lines() {
            println!("    {line}");
        }
        println!();
    }

    Ok(())
}

/// Output results in JSON format.
fn output_json(timeline_name: &str, results: &[SnapshotResult]) -> Result<()> {
    use serde_json::json;

    let output = json!({
        "timeline": timeline_name,
        "snapshots": results.iter().map(|r| json!({
            "snapshot_index": r.snapshot_index,
            "label": r.label,
            "time_offset": r.time_offset,
            "hardware_profile": r.hardware_profile,
            "initial_cost": r.initial_cost,
            "optimized_cost": r.optimized_cost,
            "cost_reduction_pct": r.cost_reduction_pct,
            "optimization_time_ms": r.optimization_time_ms,
            "rules_applied": r.rules_applied,
            "plan": r.plan,
        })).collect::<Vec<_>>(),
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Output results in Markdown format.
fn output_markdown(timeline_name: &str, results: &[SnapshotResult]) -> Result<()> {
    println!("# {timeline_name}");
    println!();

    println!("## Summary");
    println!();
    println!("| Snapshot | Label | Time | Hardware | Initial Cost | Optimized Cost | Reduction |");
    println!("|----------|-------|------|----------|--------------|----------------|-----------|");
    for result in results {
        println!(
            "| {} | {} | {}s | {} | {:.2} | {:.2} | {:.2}% |",
            result.snapshot_index,
            result.label,
            result.time_offset,
            result.hardware_profile,
            result.initial_cost,
            result.optimized_cost,
            result.cost_reduction_pct
        );
    }
    println!();

    println!("## Detailed Results");
    println!();

    for result in results {
        println!("### Snapshot {} - {}", result.snapshot_index, result.label);
        println!();
        println!("**Configuration:**");
        println!("- Time Offset: {}s", result.time_offset);
        println!("- Hardware Profile: {}", result.hardware_profile);
        println!("- Optimization Time: {}ms", result.optimization_time_ms);
        println!();

        println!("**Cost Analysis:**");
        println!("- Initial Cost: {:.2}", result.initial_cost);
        println!("- Optimized Cost: {:.2}", result.optimized_cost);
        println!("- Cost Reduction: {:.2}%", result.cost_reduction_pct);
        println!();

        if !result.rules_applied.is_empty() {
            println!("**Rules Applied:**");
            for rule in &result.rules_applied {
                println!("- {rule}");
            }
            println!();
        }

        println!("**Optimized Plan:**");
        println!("```");
        println!("{}", result.plan);
        println!("```");
        println!();
    }

    Ok(())
}

/// Output results in TOML format.
fn output_toml(timeline_name: &str, results: &[SnapshotResult]) -> Result<()> {
    println!("[timeline]");
    println!("name = {timeline_name:?}");
    println!();

    for result in results {
        println!("[[snapshot]]");
        println!("index = {}", result.snapshot_index);
        println!("label = {:?}", result.label);
        println!("time_offset = {}", result.time_offset);
        println!("hardware_profile = {:?}", result.hardware_profile);
        println!("initial_cost = {}", result.initial_cost);
        println!("optimized_cost = {}", result.optimized_cost);
        println!("cost_reduction_pct = {}", result.cost_reduction_pct);
        println!("optimization_time_ms = {}", result.optimization_time_ms);
        if !result.rules_applied.is_empty() {
            println!("rules_applied = {:?}", result.rules_applied);
        }
        println!();
    }

    Ok(())
}

/// Launch TUI visualization for timeline.
fn launch_timeline_tui(_timeline: &TimelineConfig, quiet: bool) -> Result<()> {
    if !quiet {
        eprintln!(
            "{}",
            "TUI mode is not yet implemented for timeline command".yellow()
        );
        eprintln!("Use 'ra-cli tui --timeline <file>' instead for now");
    }
    Ok(())
}
