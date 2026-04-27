//! The `optimize` subcommand.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use colored::Colorize;

use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;

use crate::cli::RuleDisplayMode;
use crate::helpers::{build_resource_budget, hardware_profile_from_def, load_hardware_profile};
use crate::output::errors::format_sql_error;
use crate::output::plans::{
    print_explain_output, print_intermediate_steps, print_optimization_header,
    print_optimization_stats, print_plan_output, print_resource_usage, print_rule_tracking,
    print_unbounded_stats,
};
use crate::output::print_header;
use crate::plan_diff;

/// Convert `SchemaInfo` from ra-metadata to `HashMap` of `Statistics` for the optimizer.
fn schema_info_to_table_stats(
    schema: &ra_metadata::SchemaInfo,
) -> HashMap<String, ra_core::statistics::Statistics> {
    use ra_core::statistics::{ColumnStats, IndexStats, Statistics};

    let mut result = HashMap::new();

    for (table_name, table_info) in &schema.tables {
        let row_count = table_info.estimated_rows.unwrap_or(1000.0);

        let mut columns = HashMap::new();
        for col in &table_info.columns {
            let distinct_count = row_count * 0.1;
            let null_fraction = if col.nullable { 0.05 } else { 0.0 };

            let col_stats = ColumnStats {
                distinct_count,
                null_fraction,
                min_value: None,
                max_value: None,
                avg_length: None,
                histogram: None,
                correlation: None,
                most_common_values: None,
                most_common_freqs: None,
            };
            columns.insert(col.name.clone(), col_stats);
        }

        let mut indexes = HashMap::new();
        for idx in &table_info.indexes {
            let is_primary = table_info
                .primary_key_columns()
                .iter()
                .all(|&pk_col| idx.columns.contains(&pk_col.to_string()));

            let idx_stats = IndexStats {
                columns: idx.columns.clone(),
                is_unique: idx.unique,
                is_primary,
                index_type: match idx.index_type.to_lowercase().as_str() {
                    "btree" => ra_core::facts::IndexType::BTree,
                    "hash" => ra_core::facts::IndexType::Hash,
                    "gin" => ra_core::facts::IndexType::Gin,
                    "gist" => ra_core::facts::IndexType::Gist,
                    "brin" => ra_core::facts::IndexType::Brin,
                    "rum" => ra_core::facts::IndexType::Rum,
                    "hnsw" => ra_core::facts::IndexType::HNSW,
                    "ivfflat" => ra_core::facts::IndexType::IVFFlat,
                    _ => ra_core::facts::IndexType::BTree,
                },
                tuple_count: row_count,
                index_size: 0,
            };
            indexes.insert(idx.name.clone(), idx_stats);
        }

        let estimated_avg_row_size = table_info.columns.len() as u64 * 30;
        let estimated_total_size = (row_count as u64) * estimated_avg_row_size;

        let stats = Statistics {
            row_count,
            avg_row_size: estimated_avg_row_size,
            total_size: estimated_total_size,
            columns,
            indexes,
        };

        result.insert(table_name.clone(), stats);
    }

    result
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_optimize(
    query: &str,
    hardware_profile_name: &str,
    diff_format: Option<&str>,
    no_color: bool,
    resource_budget: Option<&str>,
    max_time: Option<&str>,
    max_memory: Option<&str>,
    max_iterations: Option<usize>,
    overflow_strategy: Option<&str>,
    explain_format: Option<&str>,
    show_stats: bool,
    show_rules: RuleDisplayMode,
    timeline_path: Option<&Path>,
    snapshot_index: usize,
    verbose: bool,
    quiet: bool,
    schema_json: Option<&Path>,
    schema_sql: Option<&Path>,
    db: Option<&str>,
    use_rule_advisor: bool,
    use_rule_advisor_learn: bool,
    rule_advisor_db: Option<&str>,
) -> Result<()> {
    use ra_engine::{SnapshotFactsProvider, TimelineConfig};

    let color_mode = if no_color {
        plan_diff::ColorMode::Never
    } else if std::env::var("FORCE_COLOR").is_ok() {
        plan_diff::ColorMode::Always
    } else {
        plan_diff::ColorMode::Auto
    };
    plan_diff::apply_color_mode(color_mode);

    let budget = build_resource_budget(
        resource_budget,
        max_time,
        max_memory,
        max_iterations,
        overflow_strategy,
        show_rules.should_track(),
    )?;

    // Load schema from one of three sources (priority: db > schema_json > schema_sql)
    let table_stats_opt = if let Some(db_url) = db {
        if !quiet {
            let kind = ra_metadata::detect_kind(db_url)
                .map_or_else(|_| "unknown".to_owned(), |k| k.to_string());
            eprintln!("Loading schema from {} database...", kind.cyan());
        }
        let mut connector = ra_metadata::connect(db_url)
            .with_context(|| format!("connecting to database: {db_url}"))?;
        let schema = connector
            .gather_schema()
            .with_context(|| format!("gathering schema from: {db_url}"))?;
        Some(schema_info_to_table_stats(&schema))
    } else if let Some(json_path) = schema_json {
        if !quiet {
            eprintln!(
                "Loading schema from JSON: {}",
                json_path.display().to_string().cyan()
            );
        }
        let json_content = std::fs::read_to_string(json_path)
            .with_context(|| format!("reading schema JSON: {}", json_path.display()))?;
        let schema: ra_metadata::SchemaInfo = serde_json::from_str(&json_content)
            .with_context(|| format!("parsing schema JSON: {}", json_path.display()))?;
        Some(schema_info_to_table_stats(&schema))
    } else if let Some(_sql_path) = schema_sql {
        bail!("--schema-sql is not yet implemented. Use --schema-json or --db instead.");
    } else {
        None
    };

    let plan = sql_to_relexpr(query).map_err(|e| format_sql_error(&e, query))?;

    let (hardware, timeline_opt) = if let Some(path) = timeline_path {
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

        (hardware, Some(timeline))
    } else {
        (load_hardware_profile(hardware_profile_name)?, None)
    };

    let mut optimizer = if use_rule_advisor || use_rule_advisor_learn {
        let advisor_config = ra_engine::RuleAdvisorConfig {
            database_name: rule_advisor_db.unwrap_or("").to_string(),
            enable_learning: use_rule_advisor_learn,
            ..ra_engine::RuleAdvisorConfig::default()
        };
        let config = ra_engine::OptimizerConfig {
            use_rule_advisor: true,
            rule_advisor_config: advisor_config,
            ..ra_engine::OptimizerConfig::default()
        };
        Optimizer::with_config(config)
    } else {
        Optimizer::new()
    };
    optimizer.set_hardware_profile(hardware.clone());

    if let Some(b) = &budget {
        optimizer.set_resource_budget(b.clone());
    }

    if let Some(table_stats) = table_stats_opt {
        if !quiet && verbose {
            eprintln!(
                "Loaded statistics for {} tables",
                table_stats.len().to_string().cyan()
            );
        }
        for (table_name, stats) in table_stats {
            optimizer.add_table_stats(table_name, stats);
        }
    }

    if let Some(timeline) = &timeline_opt {
        if !show_rules.should_track() && !verbose {
            let snapshot = &timeline.snapshots[snapshot_index];
            let hardware_def = timeline
                .get_hardware_profile(&snapshot.hardware_profile)
                .unwrap();
            let facts = SnapshotFactsProvider::new(snapshot, hardware_def);

            let optimized = optimizer
                .optimize_with_facts(&plan, &facts)
                .with_context(|| {
                    format!(
                        "failed to optimize query with timeline snapshot {snapshot_index}: {query}"
                    )
                })?;

            if let Some(fmt) = explain_format {
                return print_explain_output(&optimized, fmt);
            }

            if !quiet {
                print_header("Query Optimization (Timeline Snapshot)");
                eprintln!("  {}: {query}", "SQL".bold());
                eprintln!(
                    "  {}: {} (snapshot {})",
                    "Timeline".bold(),
                    timeline.metadata.name,
                    snapshot_index
                );
                if let Some(label) = &snapshot.label {
                    eprintln!("  {}: {label}", "Snapshot".bold());
                }
                eprintln!();

                print_plan_output(&plan, &optimized, diff_format)?;
            }

            return Ok(());
        }

        if !quiet {
            eprintln!("{}", "Note: Timeline facts not used with --verbose or --rules-* flags (limitation of optimize_with_facts).".yellow());
            eprintln!(
                "{}",
                "      Using standard optimization with verbose tracking instead.".yellow()
            );
            eprintln!();
        }
    }

    let result = if budget.is_some() {
        optimize_bounded(
            &optimizer,
            &plan,
            &hardware,
            diff_format,
            explain_format,
            show_stats,
            show_rules,
            verbose,
            quiet,
            query,
            budget.as_ref(),
        )
    } else {
        optimize_unbounded(
            &optimizer,
            &plan,
            &hardware,
            diff_format,
            explain_format,
            show_stats,
            show_rules,
            verbose,
            quiet,
            query,
        )
    };

    if (use_rule_advisor || use_rule_advisor_learn) && !quiet && (verbose || show_stats) {
        if let Some(stats) = optimizer.advisor_stats() {
            eprintln!();
            eprintln!("{}", "Rule Advisor Statistics:".bold());
            eprintln!("  Total rules:      {}", stats.total_rules,);
            eprintln!(
                "  After Stage 1:    {} (context elimination)",
                stats.after_stage1,
            );
            eprintln!(
                "  After Stage 2:    {} (query-shape elimination)",
                stats.after_stage2,
            );
            eprintln!(
                "  After Stage 3:    {} (learned ranking)",
                stats.after_stage3,
            );
            if !stats.stage1_eliminated.is_empty() {
                eprintln!("  Stage 1 excluded: {}", stats.stage1_eliminated.join(", "),);
            }
            if !stats.stage2_eliminated.is_empty() {
                eprintln!("  Stage 2 excluded: {}", stats.stage2_eliminated.join(", "),);
            }
        }
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn optimize_bounded(
    optimizer: &Optimizer,
    plan: &ra_core::algebra::RelExpr,
    hardware: &ra_hardware::HardwareProfile,
    diff_format: Option<&str>,
    explain_format: Option<&str>,
    show_stats: bool,
    show_rules: RuleDisplayMode,
    verbose: bool,
    quiet: bool,
    query: &str,
    budget: Option<&ra_engine::ResourceBudget>,
) -> Result<()> {
    let result = if show_rules.should_track() {
        optimizer
            .optimize_with_tracking_verbose(plan, verbose)
            .with_context(|| format!("failed to optimize query: {query}"))?
    } else {
        optimizer
            .optimize_bounded(plan)
            .with_context(|| format!("failed to optimize query: {query}"))?
    };

    if let Some(fmt) = explain_format {
        return print_explain_output(&result.plan, fmt);
    }

    if !quiet {
        let title = if budget.map_or(false, |b| b.is_unlimited()) {
            "Query Optimization"
        } else {
            "Query Optimization (Resource-Bounded)"
        };
        print_optimization_header(title, query, hardware, verbose);
        print_resource_usage(&result, verbose);

        if show_stats {
            eprintln!();
            print_optimization_stats(&result.resource_usage);
        }

        if show_rules != RuleDisplayMode::None {
            eprintln!();
            if verbose {
                if let Some(tracking) = &result.rule_tracking {
                    print_intermediate_steps(tracking, plan);
                }
            } else {
                print_rule_tracking(&result, show_rules);
            }
        }

        if !verbose {
            eprintln!();
            print_plan_output(plan, &result.plan, diff_format)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn optimize_unbounded(
    optimizer: &Optimizer,
    plan: &ra_core::algebra::RelExpr,
    hardware: &ra_hardware::HardwareProfile,
    diff_format: Option<&str>,
    explain_format: Option<&str>,
    show_stats: bool,
    show_rules: RuleDisplayMode,
    verbose: bool,
    quiet: bool,
    query: &str,
) -> Result<()> {
    use std::time::Instant;

    let start = Instant::now();
    let optimized = optimizer
        .optimize(plan)
        .with_context(|| format!("failed to optimize query: {query}"))?;
    let elapsed = start.elapsed();

    if let Some(fmt) = explain_format {
        return print_explain_output(&optimized, fmt);
    }

    if !quiet {
        print_optimization_header("Query Optimization", query, hardware, verbose);

        if show_stats {
            print_unbounded_stats(elapsed);
            eprintln!();
        }

        if show_rules != RuleDisplayMode::None {
            eprintln!(
                "{}",
                "Rule tracking not available for unbounded optimization".yellow()
            );
            eprintln!("Use resource budgets to enable tracking (e.g., --resource-budget standard)");
            eprintln!();
        }

        print_plan_output(plan, &optimized, diff_format)?;
    }
    Ok(())
}
