#![expect(clippy::exit, reason = "CLI binary uses process::exit for error codes")]
//! Command dispatch/routing for ra-cli.

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate;
use colored::Colorize;

use crate::cli::{
    CacheCommands, Cli, Commands, ConfigCommands, FederatedCommands, MigrateCommands,
    PgSnapshotCommands, RegressionCommands, RuleDisplayMode, StatsTimelineCommands,
};
use crate::commands;
use crate::helpers::resolve_query;
use crate::{
    cache_commands, config_commands, federated_commands, migrate_commands, pg_snapshot_commands,
    stats_commands, timeline_commands,
};

/// Route the parsed CLI command to the appropriate handler.
pub fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Validate { path } => {
            commands::validate::cmd_validate(&path, cli.verbose, cli.quiet)
        }
        Commands::Test { path, filter } => {
            commands::test_cmd::cmd_test(&path, filter.as_deref(), cli.verbose, cli.quiet)
        }
        Commands::List { dir, category, tag } => {
            let dir = dir.as_deref().unwrap_or("rules");
            commands::list::cmd_list(dir, category.as_deref(), tag.as_deref(), cli.quiet)
        }
        Commands::Stats { dir } => {
            let dir = dir.as_deref().unwrap_or("rules");
            commands::stats::cmd_stats(dir, cli.verbose, cli.quiet)
        }
        Commands::Show { rule_id, dir } => {
            let dir = dir.as_deref().unwrap_or("rules");
            commands::show::cmd_show(&rule_id, dir)
        }
        Commands::Explain {
            query,
            hardware_profile,
            stdin: use_stdin,
            timeline,
            snapshot,
            provenance,
        } => {
            let resolved = resolve_query(&query, use_stdin)?;
            commands::explain::cmd_explain(
                &resolved,
                &hardware_profile,
                timeline.as_deref(),
                snapshot,
                cli.verbose,
                cli.quiet,
                provenance,
            )
        }
        Commands::Optimize {
            query,
            hardware_profile,
            stdin: use_stdin,
            diff,
            no_color,
            resource_budget,
            max_time,
            max_memory,
            max_iterations,
            overflow_strategy,
            explain_format,
            trace: _,
            stats,
            rules_applied,
            rules_evaluated,
            rules_available,
            rules_all,
            rules,
            rule_advisor,
            rule_advisor_learn,
            rule_advisor_db,
            timeline,
            snapshot,
            schema_json,
            schema_sql,
            db,
        } => {
            let resolved = resolve_query(&query, use_stdin)?;

            let show_rules = RuleDisplayMode::from_flags(
                rules_applied,
                rules_evaluated,
                rules_available,
                rules_all,
                rules,
            );

            commands::optimize::cmd_optimize(
                &resolved,
                &hardware_profile,
                diff.as_deref(),
                no_color,
                resource_budget.as_deref(),
                max_time.as_deref(),
                max_memory.as_deref(),
                max_iterations,
                overflow_strategy.as_deref(),
                explain_format.as_deref(),
                stats,
                show_rules,
                timeline.as_deref(),
                snapshot,
                cli.verbose,
                cli.quiet,
                schema_json.as_deref(),
                schema_sql.as_deref(),
                db.as_deref(),
                rule_advisor,
                rule_advisor_learn,
                rule_advisor_db.as_deref(),
            )
        }
        Commands::GatherMetadata { db, schema, output } => {
            commands::gather_metadata::cmd_gather_metadata(
                db.as_deref(),
                schema.as_deref(),
                &output,
                cli.verbose,
                cli.quiet,
            )
        }
        Commands::Compare {
            sql,
            db,
            explain_json,
            schema,
            hardware_profile,
        } => commands::compare::cmd_compare(
            &sql,
            db.as_deref(),
            explain_json.as_deref(),
            schema.as_deref(),
            &hardware_profile,
            cli.verbose,
            cli.quiet,
        ),
        Commands::Tui {
            timeline,
            demo,
            headless,
            record,
        } => commands::tui::cmd_tui(timeline.as_deref(), demo, headless, record.as_deref()),
        Commands::PgSnapshot(sub) => dispatch_pg_snapshot(sub),
        Commands::StatsTimeline(sub) => dispatch_stats_timeline(sub, cli.verbose),
        Commands::Format {
            query,
            stdin,
            capitalize,
            indent,
        } => commands::format::cmd_format(query.as_deref(), stdin, &capitalize, &indent, cli.quiet),
        Commands::Proxy {
            backend,
            listen,
            takeover,
            log_format,
            min_improvement,
        } => commands::proxy_cmd::cmd_proxy(
            &backend,
            &listen,
            takeover,
            &log_format,
            min_improvement,
        ),
        Commands::Translate { query, from, to } => {
            commands::translate::cmd_translate(&query, &from, &to, cli.quiet)
        }
        Commands::AnalyzeTriggers {
            table,
            database_url,
            schema,
        } => commands::analyze::cmd_analyze_triggers(
            &table,
            database_url.as_deref(),
            schema.as_deref(),
            cli.verbose,
            cli.quiet,
        ),
        Commands::Federated(sub) => dispatch_federated(sub, cli.verbose, cli.quiet),
        Commands::Config(sub) => dispatch_config(sub, cli.quiet),
        Commands::Cache(sub) => dispatch_cache(sub, cli.verbose, cli.quiet),
        Commands::Monitor {
            postgres: _,
            tui,
            demo,
            format,
        } => commands::monitor::cmd_monitor(tui, demo, &format, cli.quiet),
        Commands::Regression(sub) => dispatch_regression(sub, cli.verbose, cli.quiet),
        Commands::Migrate(sub) => dispatch_migrate(sub),
        Commands::Timeline(cmd) => timeline_commands::cmd_timeline(&cmd, cli.quiet),
        Commands::Ml(cmd) => tokio::runtime::Runtime::new()
            .context("failed to create tokio runtime")?
            .block_on(crate::ml_commands::handle_ml_command(cmd)),
        Commands::Benchmark {
            all: _,
            database: _,
            workload: _,
            output: _,
            format: _,
        } => {
            anyhow::bail!(
                "Benchmark command is temporarily disabled due to incomplete implementation"
            )
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "ra-cli", &mut std::io::stdout());
            Ok(())
        }
    }
}

fn dispatch_pg_snapshot(sub: PgSnapshotCommands) -> Result<()> {
    match sub {
        PgSnapshotCommands::Capture {
            database,
            tables,
            output,
            label,
        } => pg_snapshot_commands::capture_pg_snapshot(&database, &tables, &output, label.as_deref()),
        PgSnapshotCommands::GenerateScript {
            tables,
            output_dir,
            interval,
            script,
        } => {
            let sql_script =
                pg_snapshot_commands::generate_capture_script(&tables, &output_dir, interval)?;
            std::fs::write(&script, sql_script)?;
            eprintln!(
                "{}",
                format!("SQL script written to {}", script.display()).green()
            );
            Ok(())
        }
        PgSnapshotCommands::MergeTimeline {
            snapshot_dir,
            output,
            name,
            description,
        } => pg_snapshot_commands::merge_snapshots_to_timeline(
            &snapshot_dir,
            &output,
            &name,
            &description,
        ),
    }
}

fn dispatch_stats_timeline(sub: StatsTimelineCommands, verbose: bool) -> Result<()> {
    match sub {
        StatsTimelineCommands::Play {
            timeline,
            format,
            speed,
        } => {
            let fmt = stats_commands::OutputFormat::from_str_arg(&format)?;
            stats_commands::cmd_stats_play(&timeline, fmt, speed, verbose)
        }
        StatsTimelineCommands::Feedback {
            timeline,
            format,
            batch_size,
        } => {
            let fmt = stats_commands::OutputFormat::from_str_arg(&format)?;
            stats_commands::cmd_stats_feedback(&timeline, fmt, batch_size, verbose)
        }
        StatsTimelineCommands::Visualize { timeline, format } => {
            let fmt = stats_commands::OutputFormat::from_str_arg(&format)?;
            stats_commands::cmd_stats_visualize(&timeline, fmt, verbose)
        }
    }
}

fn dispatch_federated(sub: FederatedCommands, verbose: bool, quiet: bool) -> Result<()> {
    match sub {
        FederatedCommands::Analyze {
            query,
            remote_db,
            remote_table,
            latency,
            bandwidth,
            remote_rows,
            avg_row_size,
        } => federated_commands::cmd_federated_analyze(
            &query,
            &remote_db,
            &remote_table,
            latency,
            bandwidth,
            remote_rows,
            avg_row_size,
            verbose,
            quiet,
        ),
    }
}

fn dispatch_config(sub: ConfigCommands, quiet: bool) -> Result<()> {
    match sub {
        ConfigCommands::List => config_commands::cmd_config_list(quiet),
        ConfigCommands::Get { key } => config_commands::cmd_config_get(&key),
        ConfigCommands::Set { key, value } => config_commands::cmd_config_set(&key, &value, quiet),
        ConfigCommands::Edit => config_commands::cmd_config_edit(),
        ConfigCommands::Reset => config_commands::cmd_config_reset(quiet),
        ConfigCommands::Path => config_commands::cmd_config_path(),
    }
}

fn dispatch_cache(sub: CacheCommands, verbose: bool, quiet: bool) -> Result<()> {
    match sub {
        CacheCommands::List => cache_commands::cmd_cache_list(verbose, quiet),
        CacheCommands::Stats => cache_commands::cmd_cache_stats(quiet),
        CacheCommands::Clear { table } => cache_commands::cmd_cache_clear(table.as_deref(), quiet),
        CacheCommands::Reoptimize { threshold_pct } => {
            cache_commands::cmd_cache_reoptimize(threshold_pct, quiet)
        }
        CacheCommands::Drift => cache_commands::cmd_cache_drift(verbose, quiet),
    }
}

fn dispatch_regression(sub: RegressionCommands, verbose: bool, quiet: bool) -> Result<()> {
    match sub {
        RegressionCommands::Baseline {
            query_file,
            query_id,
            storage,
            storage_path,
            hardware_profile,
        } => crate::regression_commands::cmd_regression_baseline(
            &query_file,
            query_id.as_deref(),
            &storage,
            &storage_path,
            &hardware_profile,
            verbose,
            quiet,
        ),
        RegressionCommands::Check {
            query_file,
            query_id,
            storage,
            storage_path,
            hardware_profile,
            warn_threshold,
            error_threshold,
        } => crate::regression_commands::cmd_regression_check(
            &query_file,
            query_id.as_deref(),
            &storage,
            &storage_path,
            &hardware_profile,
            warn_threshold,
            error_threshold,
            verbose,
            quiet,
        ),
        RegressionCommands::Report {
            storage,
            storage_path,
            format,
            only_regressions,
        } => crate::regression_commands::cmd_regression_report(
            &storage,
            &storage_path,
            &format,
            only_regressions,
            verbose,
            quiet,
        ),
    }
}

fn dispatch_migrate(sub: MigrateCommands) -> Result<()> {
    match sub {
        MigrateCommands::Preconditions {
            input,
            output,
            validate,
            dry_run,
        } => {
            let input_path = std::path::Path::new(&input);
            let output_path = std::path::Path::new(&output);
            match migrate_commands::migrate_preconditions(
                input_path,
                output_path,
                dry_run,
                validate,
            ) {
                Ok(report) => {
                    report.print_summary();
                    Ok(())
                }
                Err(e) => {
                    eprintln!("{} {}", "Migration failed:".red().bold(), e);
                    std::process::exit(1);
                }
            }
        }
        MigrateCommands::Validate {
            baseline,
            migrated,
            facts,
        } => {
            let baseline_path = std::path::Path::new(&baseline);
            let migrated_path = std::path::Path::new(&migrated);
            let facts_path = facts.as_ref().map(|s| std::path::Path::new(s.as_str()));
            match migrate_commands::validate_preconditions(baseline_path, migrated_path, facts_path)
            {
                Ok(report) => {
                    report.print_summary();
                    Ok(())
                }
                Err(e) => {
                    eprintln!("{} {}", "Validation failed:".red().bold(), e);
                    std::process::exit(1);
                }
            }
        }
    }
}
