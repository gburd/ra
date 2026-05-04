//! Plan analysis and visualization helpers for ra-cli.

use anyhow::{bail, Result};
use colored::Colorize;

use crate::display::format_plan_tree;
use crate::plan_diff;
use crate::rule_explanations;

use crate::cli::RuleDisplayMode;

/// EXPLAIN output format options.
pub enum ExplainOutputFormat {
    Ascii,
    DatabaseText {
        database: DatabaseTextFormat,
        cost_params: ra_metadata::DatabaseCostParams,
    },
}

/// Database-specific text format options.
pub enum DatabaseTextFormat {
    Postgres,
    Mysql,
    Sqlite,
}

/// Parse an EXPLAIN format string.
pub fn parse_explain_format(s: &str) -> Result<ExplainOutputFormat> {
    match s.to_lowercase().as_str() {
        "ascii" => Ok(ExplainOutputFormat::Ascii),
        "postgres" | "postgresql" | "pg" => Ok(ExplainOutputFormat::DatabaseText {
            database: DatabaseTextFormat::Postgres,
            cost_params: ra_metadata::DatabaseCostParams::postgres_default(),
        }),
        "mysql" => Ok(ExplainOutputFormat::DatabaseText {
            database: DatabaseTextFormat::Mysql,
            cost_params: ra_metadata::DatabaseCostParams::mysql_default(),
        }),
        "sqlite" => Ok(ExplainOutputFormat::DatabaseText {
            database: DatabaseTextFormat::Sqlite,
            cost_params: ra_metadata::DatabaseCostParams::postgres_default(),
        }),
        _ => bail!(
            "unknown explain format: '{s}'. \
             Valid options: postgres, mysql, sqlite, ascii"
        ),
    }
}

/// Generate and print EXPLAIN output for an optimized plan.
pub fn print_explain_output(plan: &ra_core::algebra::RelExpr, format_str: &str) -> Result<()> {
    let format = parse_explain_format(format_str)?;

    let output = match format {
        ExplainOutputFormat::Ascii => format_plan_tree(plan),
        ExplainOutputFormat::DatabaseText {
            database,
            cost_params,
        } => {
            let _ = cost_params;
            let explain_node = ra_metadata::relexpr_to_explain_node(plan);
            match database {
                DatabaseTextFormat::Postgres => ra_metadata::format_postgres_explain(&explain_node),
                DatabaseTextFormat::Mysql => ra_metadata::format_mysql_explain(&explain_node),
                DatabaseTextFormat::Sqlite => ra_metadata::format_sqlite_explain(&explain_node),
            }
        }
    };

    eprintln!("{output}");
    Ok(())
}

/// Parse a diff format string into a `DiffFormat`.
pub fn parse_diff_format(s: &str) -> Result<plan_diff::DiffFormat> {
    match s.to_lowercase().as_str() {
        "colored" | "color" => Ok(plan_diff::DiffFormat::Colored),
        "plain" | "text" => Ok(plan_diff::DiffFormat::Plain),
        "side-by-side" | "sbs" => Ok(plan_diff::DiffFormat::SideBySide),
        "compact" | "summary" => Ok(plan_diff::DiffFormat::Compact),
        _ => bail!(
            "unknown diff format: '{s}'. \
             Valid options: colored, plain, side-by-side, compact"
        ),
    }
}

/// Print original and optimized plan, optionally as diff.
pub fn print_plan_output(
    original: &ra_core::algebra::RelExpr,
    optimized: &ra_core::algebra::RelExpr,
    diff_format: Option<&str>,
) -> Result<()> {
    if let Some(fmt_str) = diff_format {
        let fmt = parse_diff_format(fmt_str)?;
        let diff_output = plan_diff::render_diff(original, optimized, fmt);
        eprintln!("{diff_output}");
    } else if original == optimized {
        eprintln!("{}", "Original Plan Unchanged After Optimization:".bold());
        eprintln!("{}", format_plan_tree(original));
    } else {
        eprintln!("{}", "Original Plan:".bold());
        eprintln!("{}", format_plan_tree(original));
        eprintln!();
        eprintln!("{}", "Optimized Plan:".bold());
        eprintln!("{}", format_plan_tree(optimized));
    }
    Ok(())
}

/// Print optimization header with hardware info and formatted SQL.
pub fn print_optimization_header(
    title: &str,
    query: &str,
    hardware: &ra_hardware::HardwareProfile,
    verbose: bool,
) {
    crate::output::print_header(title);

    eprintln!(
        "  {}: {} ({} cores, {} MB L3, {}-bit SIMD)",
        "Hardware".bold(),
        hardware.name,
        hardware.cpu_cores,
        hardware.l3_cache_bytes / (1024 * 1024),
        hardware.simd_width_bits
    );

    if verbose {
        let metrics = ra_hardware::SystemMetrics::collect();
        eprintln!("  {}: {}", "System".bold(), metrics.format());
    }

    eprintln!();

    eprintln!("  {}:", "SQL".bold());
    print_formatted_sql(query);
    eprintln!();
}

/// Format and print a SQL query with indentation.
///
/// Uses the Ra SQL formatter to capitalize keywords and break the query
/// onto multiple lines (one clause per line, CTE bodies expanded).
/// The formatter always succeeds: if `ra-sql-parser` cannot parse the SQL
/// (e.g. queries containing `->>`  or `::` operators), it applies
/// formatting directly to the original text.
pub fn print_formatted_sql(query: &str) {
    // format() no longer returns Err — it degrades gracefully.
    let formatted = ra_parser::formatter::SqlFormatter::default_style()
        .format(query)
        .unwrap_or_else(|_| query.to_owned());
    let indented = formatted
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    eprintln!("{indented}");
}

/// Display resource usage from a bounded optimization result.
pub fn print_resource_usage(result: &ra_engine::OptimizationResult, verbose: bool) {
    let usage = &result.resource_usage;
    let status = match result.status {
        ra_engine::OptimizationStatus::Complete => {
            format!("{}", "complete".green())
        }
        ra_engine::OptimizationStatus::Incomplete => {
            let msg = match usage.budget_exceeded {
                Some(ref r) => format!("stopped ({r})"),
                None => "incomplete".to_owned(),
            };
            format!("{}", msg.yellow())
        }
        ra_engine::OptimizationStatus::Failed => {
            format!("{}", "failed".red())
        }
    };

    eprintln!("{}", "Resource Usage:".bold());
    eprintln!("  {}: {status}", "Status".bold());
    eprintln!(
        "  {}: {:.1}ms",
        "Time".bold(),
        usage.elapsed_time.as_secs_f64() * 1000.0,
    );
    eprintln!("  {}: {}", "Iterations".bold(), usage.iterations_used);
    eprintln!(
        "  {}: {}",
        "Peak e-graph nodes".bold(),
        usage.peak_egraph_nodes,
    );

    if verbose {
        let mem_mb = usage.peak_memory_estimate as f64 / (1024.0 * 1024.0);
        eprintln!("  {}: {mem_mb:.2} MB", "Peak memory (est.)".bold());
        eprintln!("  {}: {:.2}", "Plan cost".bold(), result.cost);
    }
}

pub fn print_optimization_stats(usage: &ra_engine::ResourceUsageReport) {
    eprintln!("{}", "Optimization Statistics:".bold());
    eprintln!(
        "  {}: {:.1}ms",
        "Planning time".bold(),
        usage.elapsed_time.as_secs_f64() * 1000.0,
    );
    eprintln!("  {}: {}", "Iterations used".bold(), usage.iterations_used);
    eprintln!(
        "  {}: {}",
        "Peak e-graph nodes".bold(),
        usage.peak_egraph_nodes,
    );
    let mem_mb = usage.peak_memory_estimate as f64 / (1024.0 * 1024.0);
    eprintln!("  {}: {mem_mb:.2} MB", "Peak memory".bold());
    if let Some(ref exceeded) = usage.budget_exceeded {
        eprintln!("  {}: {exceeded}", "Budget exceeded".bold().yellow());
    }
}

pub fn print_unbounded_stats(elapsed: std::time::Duration) {
    eprintln!("{}", "Optimization Statistics:".bold());
    eprintln!(
        "  {}: {:.1}ms",
        "Planning time".bold(),
        elapsed.as_secs_f64() * 1000.0,
    );
}

pub fn print_rule_tracking(result: &ra_engine::OptimizationResult, mode: RuleDisplayMode) {
    let Some(tracking) = &result.rule_tracking else {
        eprintln!("{}", "Rule tracking not available".yellow());
        eprintln!("This should not happen - tracking was requested but not populated");
        return;
    };

    match mode {
        RuleDisplayMode::None => {}
        RuleDisplayMode::Applied => print_applied_rules(tracking),
        RuleDisplayMode::Evaluated => print_evaluated_rules(tracking),
        RuleDisplayMode::Available => print_available_rules(tracking),
        RuleDisplayMode::All => {
            print_applied_rules(tracking);
            eprintln!();
            print_evaluated_rules(tracking);
            eprintln!();
            print_available_rules(tracking);
        }
    }
}

pub fn print_intermediate_steps(
    tracking: &ra_engine::RuleTrackingResult,
    original_plan: &ra_core::algebra::RelExpr,
) {
    let Some(steps) = &tracking.intermediate_steps else {
        return;
    };

    if steps.is_empty() {
        return;
    }

    eprintln!();
    eprintln!("{}", "Intermediate Optimization Steps:".bold().underline());
    eprintln!();
    eprintln!("{}", "Original Plan:".bold());
    eprintln!("{}", format_plan_tree(original_plan));
    eprintln!();

    let mut i = 0;
    while i < steps.len() {
        let step = &steps[i];

        let mut grouped_steps = vec![step];
        let mut j = i + 1;
        while j < steps.len()
            && rule_explanations::should_group_with_previous(&steps[j].rule_name, &step.rule_name)
        {
            grouped_steps.push(&steps[j]);
            j += 1;
        }

        if grouped_steps.len() > 1 {
            let step_numbers: Vec<_> = grouped_steps.iter().map(|s| s.step_number).collect();
            let step_range = if step_numbers.len() == 2 {
                format!("{} and {}", step_numbers[0], step_numbers[1])
            } else {
                format!(
                    "{}-{}",
                    step_numbers[0],
                    step_numbers[step_numbers.len() - 1]
                )
            };

            eprintln!(
                "{}",
                format!(
                    "Steps {}: Applied {} related rules",
                    step_range,
                    grouped_steps.len()
                )
                .bold()
                .green()
            );

            for s in &grouped_steps {
                eprintln!("  • {}", s.rule_name.dimmed());
            }
        } else {
            eprintln!(
                "{}",
                format!("Step {}: {}", step.step_number, step.rule_name)
                    .bold()
                    .green()
            );
        }

        eprintln!();

        let explanation = rule_explanations::explain_rule(&step.rule_name);

        eprintln!("  {}", explanation.summary);
        eprintln!();

        eprintln!("  {}", "Impact:".bold().cyan());
        for line in explanation.impact.lines() {
            eprintln!("    {line}");
        }

        if let (Some(before), Some(after)) = (explanation.before_example, explanation.after_example)
        {
            eprintln!();
            eprintln!("  {}", "Example transformation:".bold());
            eprintln!("    {}: {}", "Before".dimmed(), before);
            eprintln!("    {}: {}", "After".dimmed(), after);
        }

        eprintln!();
        if let Some(improvement) = step.cost_improvement {
            eprintln!(
                "  {}: {}",
                "Cost Impact".bold().yellow(),
                format_impact(improvement, &step.plan_before, &step.plan_after,)
            );
        } else if let Some(reason) = explanation.why_no_cost_change {
            eprintln!(
                "  {}: No measurable change ({reason})",
                "Cost Impact".bold().yellow(),
            );
        } else {
            eprintln!(
                "  {}: No cost change measured",
                "Cost Impact".bold().yellow()
            );
        }

        eprintln!();

        let Some(last_step) = grouped_steps.last() else {
            continue;
        };
        let before_tree = format_plan_tree(&last_step.plan_before);
        let after_tree = format_plan_tree(&last_step.plan_after);

        if before_tree == after_tree {
            eprintln!(
                "  {}: Plan structure unchanged (added to search space)",
                "Plan Changes".bold().dimmed()
            );
        } else {
            eprintln!("  {}:", "Plan Changes".bold());
            print_plan_with_changes_inline(&last_step.plan_after, &last_step.plan_before);
        }

        eprintln!();

        i = j;
    }

    eprintln!("{}", "Final Optimized Plan:".bold());
    if let Some(last_step) = steps.last() {
        eprintln!("{}", format_plan_tree(&last_step.plan_after));
    }
}

/// Format the impact of an optimization with context.
fn format_impact(
    cost_improvement: f64,
    plan_before: &ra_core::algebra::RelExpr,
    plan_after: &ra_core::algebra::RelExpr,
) -> String {
    let mut impacts = Vec::new();

    impacts.push(format!(
        "Reduced estimated cost by {:.2}",
        cost_improvement.to_string().green()
    ));

    let cost_type = identify_cost_change_type(plan_before, plan_after);
    if !cost_type.is_empty() && cost_type != "Cost model refinement based on updated statistics" {
        impacts.push(cost_type);
    }

    if has_scan_upgrade(plan_before, plan_after) {
        impacts.push("Eliminated full table scan, using index instead".to_string());
    }

    if let Some(scan_change) = detect_scan_optimization(plan_before, plan_after) {
        impacts.push(scan_change);
    }

    if let Some(strategy_change) = detect_strategy_change(plan_before, plan_after) {
        impacts.push(strategy_change);
    }

    if has_operator_elimination(plan_before, plan_after) {
        let diff = count_operators(plan_before) - count_operators(plan_after);
        if diff > 0 {
            impacts.push(format!("Removed {diff} redundant operator(s)"));
        }
    }

    if has_parallelization(plan_after) {
        impacts.push("Enabled parallel execution".to_string());
    }

    impacts.join("; ")
}

fn identify_cost_change_type(
    plan_before: &ra_core::algebra::RelExpr,
    plan_after: &ra_core::algebra::RelExpr,
) -> String {
    use ra_core::algebra::RelExpr;

    match (plan_before, plan_after) {
        (RelExpr::Scan { .. }, RelExpr::IndexOnlyScan { .. })
        | (RelExpr::Scan { .. }, RelExpr::BitmapIndexScan { .. }) => {
            "I/O cost reduced (index access instead of sequential scan)".to_string()
        }
        (RelExpr::Join { .. }, RelExpr::ParallelHashJoin { .. }) => {
            "CPU cost optimized (parallel execution)".to_string()
        }
        (RelExpr::Sort { .. }, RelExpr::IncrementalSort { .. }) => {
            "CPU and Memory cost reduced (incremental sort)".to_string()
        }
        _ => {
            if has_filter_near_scan(plan_after) && !has_filter_near_scan(plan_before) {
                "CPU cost reduced (filter pushdown reduces processing)".to_string()
            } else {
                "Cost model refinement based on updated statistics".to_string()
            }
        }
    }
}

fn has_filter_near_scan(plan: &ra_core::algebra::RelExpr) -> bool {
    has_filter_near_scan_depth(plan, 0)
}

fn has_filter_near_scan_depth(plan: &ra_core::algebra::RelExpr, depth: usize) -> bool {
    use ra_core::algebra::RelExpr;

    if depth > 2 {
        return false;
    }

    match plan {
        RelExpr::Filter { input, .. } => {
            matches!(**input, RelExpr::Scan { .. })
                || plan
                    .children()
                    .iter()
                    .any(|c| has_filter_near_scan_depth(c, depth + 1))
        }
        _ => plan
            .children()
            .iter()
            .any(|c| has_filter_near_scan_depth(c, depth + 1)),
    }
}

fn detect_strategy_change(
    plan_before: &ra_core::algebra::RelExpr,
    plan_after: &ra_core::algebra::RelExpr,
) -> Option<String> {
    use ra_core::algebra::RelExpr;

    match (plan_before, plan_after) {
        (RelExpr::Join { .. }, RelExpr::ParallelHashJoin { .. }) => {
            Some("Hash join → Parallel hash join".to_string())
        }
        (RelExpr::ParallelHashJoin { .. }, RelExpr::Join { .. }) => {
            Some("Parallel hash join → Hash join".to_string())
        }
        (RelExpr::Sort { .. }, RelExpr::IncrementalSort { .. }) => {
            Some("Full sort → Incremental sort".to_string())
        }
        (RelExpr::IncrementalSort { .. }, RelExpr::Sort { .. }) => {
            Some("Incremental sort → Full sort".to_string())
        }
        _ => None,
    }
}

fn detect_scan_optimization(
    plan_before: &ra_core::algebra::RelExpr,
    plan_after: &ra_core::algebra::RelExpr,
) -> Option<String> {
    let before_scan_type = find_scan_type(plan_before);
    let after_scan_type = find_scan_type(plan_after);

    if before_scan_type == after_scan_type {
        None
    } else {
        match (before_scan_type.as_deref(), after_scan_type.as_deref()) {
            (Some("Scan"), Some("IndexOnlyScan")) => {
                Some("Index-only scan enabled (covering index)".to_string())
            }
            (Some("Scan"), Some("BitmapIndexScan")) => {
                Some("Bitmap index scan enabled".to_string())
            }
            (Some("BitmapIndexScan"), Some("IndexOnlyScan")) => {
                Some("Upgraded to index-only scan".to_string())
            }
            _ => Some(format!(
                "Scan method: {} → {}",
                before_scan_type.unwrap_or_else(|| "Unknown".to_string()),
                after_scan_type.unwrap_or_else(|| "Unknown".to_string())
            )),
        }
    }
}

fn find_scan_type(plan: &ra_core::algebra::RelExpr) -> Option<String> {
    use ra_core::algebra::RelExpr;

    match plan {
        RelExpr::Scan { .. } => Some("Scan".to_string()),
        RelExpr::IndexOnlyScan { .. } => Some("IndexOnlyScan".to_string()),
        RelExpr::BitmapIndexScan { .. } => Some("BitmapIndexScan".to_string()),
        RelExpr::BitmapHeapScan { .. } => Some("BitmapHeapScan".to_string()),
        _ => {
            for child in plan.children() {
                if let Some(scan_type) = find_scan_type(child) {
                    return Some(scan_type);
                }
            }
            None
        }
    }
}

/// Render the diff between `before` and `plan` as standard unified-diff
/// format: `-` prefix for removed lines, `+` for added, ` ` for context.
/// The sign character appears at column 0 so the tree indentation inside
/// each line stays perfectly aligned across all three kinds of output.
fn print_plan_with_changes_inline(
    plan: &ra_core::algebra::RelExpr,
    before: &ra_core::algebra::RelExpr,
) {
    let before_tree = format_plan_tree(before);
    let after_tree = format_plan_tree(plan);

    let before_lines: Vec<&str> = before_tree.lines().collect();
    let after_lines: Vec<&str> = after_tree.lines().collect();

    // Build an LCS table over the raw tree lines so the diff is line-exact.
    let lcs = lcs_lines(&before_lines, &after_lines);

    let mut bi = 0usize; // index into before_lines
    let mut ai = 0usize; // index into after_lines
    let mut li = 0usize; // index into lcs

    while bi < before_lines.len() || ai < after_lines.len() {
        let in_lcs_before =
            li < lcs.len() && bi < before_lines.len() && before_lines[bi] == lcs[li];
        let in_lcs_after =
            li < lcs.len() && ai < after_lines.len() && after_lines[ai] == lcs[li];

        if in_lcs_before && in_lcs_after {
            // Unchanged context line.
            eprintln!(" {}", before_lines[bi].dimmed());
            bi += 1;
            ai += 1;
            li += 1;
        } else if !in_lcs_before && bi < before_lines.len() {
            // Line only in before → removed.
            eprintln!("{}{}", "-".red().bold(), before_lines[bi].red());
            bi += 1;
        } else if !in_lcs_after && ai < after_lines.len() {
            // Line only in after → added.
            eprintln!("{}{}", "+".green().bold(), after_lines[ai].green());
            ai += 1;
        } else {
            break; // safety
        }
    }
}

/// Compute the Longest Common Subsequence of two line slices.
/// Returns the matched lines in order (not their indices).
fn lcs_lines<'a>(before: &[&'a str], after: &[&'a str]) -> Vec<&'a str> {
    let rows = before.len();
    let cols = after.len();
    let mut dp = vec![vec![0u32; cols + 1]; rows + 1];
    for row in 1..=rows {
        for col in 1..=cols {
            if before[row - 1] == after[col - 1] {
                dp[row][col] = dp[row - 1][col - 1] + 1;
            } else {
                dp[row][col] = dp[row - 1][col].max(dp[row][col - 1]);
            }
        }
    }
    // Back-track to recover the LCS.
    let mut result = Vec::new();
    let (mut row, mut col) = (rows, cols);
    while row > 0 && col > 0 {
        if before[row - 1] == after[col - 1] {
            result.push(before[row - 1]);
            row -= 1;
            col -= 1;
        } else if dp[row - 1][col] >= dp[row][col - 1] {
            row -= 1;
        } else {
            col -= 1;
        }
    }
    result.reverse();
    result
}

fn has_scan_upgrade(before: &ra_core::algebra::RelExpr, after: &ra_core::algebra::RelExpr) -> bool {
    has_table_scan(before) && has_index_scan(after)
}

fn has_table_scan(expr: &ra_core::algebra::RelExpr) -> bool {
    match expr {
        ra_core::algebra::RelExpr::Scan { .. } => true,
        _ => expr.children().iter().any(|&child| has_table_scan(child)),
    }
}

fn has_index_scan(expr: &ra_core::algebra::RelExpr) -> bool {
    match expr {
        ra_core::algebra::RelExpr::IndexScan { .. }
        | ra_core::algebra::RelExpr::IndexOnlyScan { .. }
        | ra_core::algebra::RelExpr::BitmapIndexScan { .. } => true,
        _ => expr.children().iter().any(|&child| has_index_scan(child)),
    }
}

fn count_operators(expr: &ra_core::algebra::RelExpr) -> usize {
    1 + expr
        .children()
        .iter()
        .map(|c| count_operators(c))
        .sum::<usize>()
}

fn has_operator_elimination(
    before: &ra_core::algebra::RelExpr,
    after: &ra_core::algebra::RelExpr,
) -> bool {
    count_operators(before) > count_operators(after)
}

fn has_parallelization(expr: &ra_core::algebra::RelExpr) -> bool {
    match expr {
        ra_core::algebra::RelExpr::ParallelScan { .. }
        | ra_core::algebra::RelExpr::ParallelHashJoin { .. }
        | ra_core::algebra::RelExpr::ParallelAggregate { .. }
        | ra_core::algebra::RelExpr::Gather { .. } => true,
        _ => expr
            .children()
            .iter()
            .any(|&child| has_parallelization(child)),
    }
}

fn print_applied_rules(tracking: &ra_engine::RuleTrackingResult) {
    eprintln!("{}", "Rules Applied:".bold());
    if tracking.applied.is_empty() {
        eprintln!("  {}", "No rules modified the e-graph".dimmed());
        return;
    }

    for (i, rule) in tracking.applied.iter().enumerate() {
        let cost_info = if let Some(improvement) = rule.cost_improvement {
            format!(" (cost improvement: {improvement:.2})")
        } else {
            String::new()
        };

        eprintln!(
            "  {}. {} - fired {} time{}{}",
            i + 1,
            rule.name.green(),
            rule.fired_count,
            if rule.fired_count == 1 { "" } else { "s" },
            cost_info.dimmed()
        );
    }
}

fn print_evaluated_rules(tracking: &ra_engine::RuleTrackingResult) {
    eprintln!("{}", "Rules Evaluated but Not Applied:".bold());
    if tracking.evaluated.is_empty() {
        eprintln!("  {}", "All evaluated rules were applied".dimmed());
        return;
    }

    let max_show = 10;
    for (i, rule) in tracking.evaluated.iter().take(max_show).enumerate() {
        eprintln!(
            "  {}. {} - tried {} time{} ({})",
            i + 1,
            rule.name.yellow(),
            rule.tried_count,
            if rule.tried_count == 1 { "" } else { "s" },
            rule.rejection_reason.dimmed()
        );
    }

    if tracking.evaluated.len() > max_show {
        eprintln!(
            "  {} ({} more rules not shown)",
            "...".dimmed(),
            tracking.evaluated.len() - max_show
        );
    }
}

fn print_available_rules(tracking: &ra_engine::RuleTrackingResult) {
    eprintln!(
        "{}: {} total",
        "Available Rules".bold(),
        tracking.available.len()
    );
    eprintln!("  Use --rules-applied to see which rules modified the plan");
}
