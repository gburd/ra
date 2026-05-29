//! The `explain` subcommand.

use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use ra_parser::sql_to_relexpr;

use crate::display::format_plan_tree;
use crate::helpers::{hardware_profile_from_def, load_hardware_profile};
use crate::output::errors::format_sql_error;
use crate::output::print_header;

pub fn cmd_explain(
    query: &str,
    hardware_profile_name: &str,
    timeline_path: Option<&Path>,
    snapshot_index: usize,
    verbose: bool,
    quiet: bool,
    show_provenance: bool,
    plan_advice: Option<&str>,
) -> Result<()> {
    use ra_engine::TimelineConfig;

    let plan = sql_to_relexpr(query).map_err(|e| format_sql_error(&e, query))?;

    let (hardware, timeline_context) = if let Some(path) = timeline_path {
        let timeline = TimelineConfig::from_file(path)
            .with_context(|| format!("Failed to load timeline from {}", path.display()))?;

        let snapshot = timeline.snapshots.get(snapshot_index).ok_or_else(|| {
            anyhow::anyhow!(
                "Snapshot index {} not found in timeline (has {} snapshots)",
                snapshot_index,
                timeline.snapshots.len()
            )
        })?;

        let hardware_def = timeline
            .get_hardware_profile(&snapshot.hardware_profile)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Hardware profile '{}' not found in timeline",
                    snapshot.hardware_profile
                )
            })?;

        let hardware = hardware_profile_from_def(hardware_def);

        (hardware, Some((timeline, snapshot_index)))
    } else {
        (load_hardware_profile(hardware_profile_name)?, None)
    };

    if !quiet {
        print_header("Query Plan Explanation");
        eprintln!("  {}:", "SQL".bold());
        crate::output::plans::print_formatted_sql(query);

        if let Some((timeline, idx)) = &timeline_context {
            let snapshot = &timeline.snapshots[*idx];
            eprintln!(
                "  {}: {} (snapshot {})",
                "Timeline".bold(),
                timeline.metadata.name,
                idx
            );
            if let Some(label) = &snapshot.label {
                eprintln!("  {}: {label}", "Snapshot".bold());
            }
        }

        if verbose {
            eprintln!(
                "  {}: {} ({} cores, {} MB L3 cache, {}-bit SIMD)",
                "Hardware".bold(),
                hardware.name,
                hardware.cpu_cores,
                hardware.l3_cache_bytes / (1024 * 1024),
                hardware.simd_width_bits
            );
        }

        eprintln!();
        eprintln!("{}", "Plan:".bold());
        eprintln!("{}", format_plan_tree(&plan));
    }

    if show_provenance {
        // Run the optimizer to produce an OptimizationResult so we
        // have a populated `PlanProvenance`. We use the bounded
        // entry point with default budgets — for `--provenance`
        // the user wants the metadata, not a different plan from
        // the one they saw above.
        use ra_engine::Optimizer;
        let optimizer = if let Some(advice) = plan_advice {
            Optimizer::with_config(ra_engine::OptimizerConfig {
                plan_advice: Some(advice.to_string()),
                ..ra_engine::OptimizerConfig::default()
            })
        } else {
            Optimizer::new()
        };
        let opt_result = optimizer
            .optimize_bounded(&plan)
            .context("optimization failed while collecting provenance")?;
        if let Some(p) = opt_result.provenance.as_ref() {
            eprintln!();
            eprintln!("{}", "Provenance:".bold());
            eprintln!(
                "  {} {:#016x}_{:016x}_{:016x}",
                "fingerprint:".bold(),
                p.fingerprint.join_graph_hash,
                p.fingerprint.predicate_hash,
                p.fingerprint.aggregation_hash
            );
            eprintln!(
                "  {} {}",
                "cost_model_id:".bold(),
                p.cost_model_id.as_deref().unwrap_or("(none)")
            );
            eprintln!(
                "  {} {}",
                "stats_version:".bold(),
                p.stats_version
                    .map_or_else(|| "(unset)".to_string(), |v| v.to_string())
            );
            eprintln!(
                "  {} {:016x}",
                "hardware_profile_hash:".bold(),
                p.hardware_profile_hash
            );
            eprintln!(
                "  {} {:016x}",
                "active_rule_set_hash:".bold(),
                p.active_rule_set_hash
            );
            eprintln!("  {} {}", "optimizer_version:".bold(), p.optimizer_version);
            eprintln!("  {} {:?}", "route:".bold(), p.route);
            eprintln!("  {} {}", "termination_reason:".bold(), p.termination_reason);
        }
    }

    Ok(())
}
