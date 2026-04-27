//! The `compare` subcommand.

use anyhow::{bail, Context, Result};
use colored::Colorize;

use ra_parser::sql_to_relexpr;

use crate::diff_validator;
use crate::display::format_plan_tree;
use crate::helpers::load_hardware_profile;
use crate::output::errors::format_sql_error;
use crate::output::print_header;

pub fn cmd_compare(
    sql: &str,
    db_url: Option<&str>,
    explain_json_path: Option<&str>,
    _schema_path: Option<&str>,
    hardware_profile_name: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let ra_plan = sql_to_relexpr(sql).map_err(|e| format_sql_error(&e, sql))?;

    let db_explain = if let Some(url) = db_url {
        if !quiet {
            let kind = ra_metadata::detect_kind(url)
                .map_or_else(|_| "unknown".to_owned(), |k| k.to_string());
            eprintln!("Running EXPLAIN on {} database...", kind.cyan());
        }
        let mut connector =
            ra_metadata::connect(url).with_context(|| format!("connecting to database: {url}"))?;
        connector
            .explain_query(sql)
            .with_context(|| format!("running EXPLAIN on: {url}"))?
    } else if let Some(path) = explain_json_path {
        let explain_source = std::fs::read_to_string(path)
            .with_context(|| format!("reading EXPLAIN JSON: {path}"))?;
        serde_json::from_str(&explain_source)
            .with_context(|| format!("parsing EXPLAIN JSON from: {path}"))?
    } else {
        bail!(
            "either --db <url> or --explain-json <path> \
             is required"
        );
    };

    let hardware = load_hardware_profile(hardware_profile_name)?;

    let comparison = ra_metadata::diff_validator::compare_plans(&ra_plan, &db_explain);

    if !quiet {
        print_header("Plan Comparison");
        eprintln!("  {}: {sql}", "SQL".bold());

        if verbose {
            eprintln!(
                "  {}: {} ({} cores, {} MB L3 cache, \
                 {}-bit SIMD)",
                "Hardware".bold(),
                hardware.name,
                hardware.cpu_cores,
                hardware.l3_cache_bytes / (1024 * 1024),
                hardware.simd_width_bits
            );
        }

        eprintln!();

        eprintln!("{}", "RA Optimizer Plan:".bold());
        eprintln!("{}", format_plan_tree(&ra_plan));
        eprintln!();

        eprintln!("{}", "Database EXPLAIN Plan:".bold());
        eprintln!("{}", diff_validator::format_explain_tree(&db_explain));
        eprintln!();

        eprintln!(
            "{} {:.0}% ({} agreements, {} disagreements)",
            "Confidence:".bold(),
            comparison.confidence * 100.0,
            comparison.agreements.len(),
            comparison.disagreements.len(),
        );
        eprintln!();

        if !comparison.agreements.is_empty() {
            eprintln!("{}", "Agreements:".bold());
            for a in &comparison.agreements {
                eprintln!(
                    "  {} {}: {}",
                    "[OK]".green().bold(),
                    a.aspect,
                    a.description,
                );
            }
            eprintln!();
        }

        if !comparison.disagreements.is_empty() {
            eprintln!("{}", "Disagreements:".bold());
            for d in &comparison.disagreements {
                eprintln!("  {} {}:", "[DIFF]".yellow().bold(), d.aspect,);
                eprintln!("    {}: {}", "RA optimizer".bold(), d.ra_choice,);
                eprintln!("    {}:     {}", "Database".bold(), d.db_choice,);
                eprintln!("    {}: {}", "Severity".dimmed(), d.severity,);
            }
        }
    }

    Ok(())
}
