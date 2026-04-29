//! The `analyze-triggers` subcommand.

use anyhow::Result;
use colored::Colorize;

use crate::helpers::load_schema_for_analysis;
use crate::output::print_header;

pub fn cmd_analyze_triggers(
    table: &str,
    database_url: Option<&str>,
    schema_path: Option<&str>,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let schema = load_schema_for_analysis(database_url, schema_path)?;

    let estimated_rows = schema
        .get_table(table)
        .and_then(|t| t.estimated_rows)
        .unwrap_or(1000.0);

    let analysis =
        ra_engine::trigger_optimizer::analyze_table_triggers(table, &schema, estimated_rows);

    if !quiet {
        print_header(&format!("Trigger Analysis: {table}"));
    }

    print_dml_cost("INSERT", analysis.insert_cost.as_ref(), verbose);
    print_dml_cost("UPDATE", analysis.update_cost.as_ref(), verbose);
    print_dml_cost("DELETE", analysis.delete_cost.as_ref(), verbose);

    if !analysis.cascade_warnings.is_empty() {
        eprintln!();
        eprintln!("{}", "Cascade Warnings:".bold());
        for warning in &analysis.cascade_warnings {
            let severity_str = match warning.severity {
                ra_engine::trigger_optimizer::CascadeSeverity::Info => {
                    format!("[{}]", warning.severity).dimmed().to_string()
                }
                ra_engine::trigger_optimizer::CascadeSeverity::Warning => {
                    format!("[{}]", warning.severity).yellow().to_string()
                }
                ra_engine::trigger_optimizer::CascadeSeverity::Error => {
                    format!("[{}]", warning.severity).red().to_string()
                }
            };
            eprintln!("  {severity_str} {}", warning.message);
            if verbose && !warning.trigger_chain.is_empty() {
                eprintln!("    chain: {}", warning.trigger_chain.join(" -> "));
            }
        }
    } else if !quiet {
        eprintln!();
        eprintln!("  {}", "No cascade warnings detected.".dimmed());
    }

    Ok(())
}

fn print_dml_cost(
    event: &str,
    cost: Option<&ra_engine::trigger_optimizer::DmlCostEstimate>,
    verbose: bool,
) {
    let Some(cost) = cost else {
        return;
    };

    eprintln!();
    eprintln!("  {} {}:", event.bold(), "cost".dimmed());
    eprintln!(
        "    triggers: {} ({} firing)",
        cost.trigger_count,
        if cost.trigger_count > 0 {
            "active"
        } else {
            "none"
        }
    );
    eprintln!("    base cost:    {:.2}", cost.base_cost);
    eprintln!("    trigger cost: {:.2}", cost.trigger_cost);
    eprintln!("    total cost:   {:.2}", cost.total_cost);

    if verbose {
        for item in &cost.trigger_breakdown {
            eprintln!(
                "      {} ({} {}) cost: {:.2}",
                item.trigger_name, item.timing, item.scope, item.estimated_cost,
            );
        }
    }
}
