//! Command-line interface for the relational algebra rule system.
#![allow(clippy::print_stderr)]

mod diff_validator;
mod display;
mod federated_commands;
pub(crate) mod plan_diff;
pub(crate) mod side_by_side;
mod stats_commands;
mod test_executor;
mod visualize;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use colored::Colorize;

use ra_engine::Optimizer;
use ra_parser::{
    ParseError, RuleFile, parse_metadata, parse_rule_file,
    sql_to_relexpr, validate_metadata_all,
};

use display::format_plan_tree;
use test_executor::{TestOutcome, run_tests};

// ── CLI definition ──────────────────────────────────────────

#[derive(Parser)]
#[command(name = "ra-cli")]
#[command(
    about = "Relational Algebra Rule System CLI",
    long_about = None
)]
struct Cli {
    /// Increase output verbosity.
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress all non-error output.
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate .rra rule files.
    Validate {
        /// Path to a rule file or directory of rule files.
        path: String,
    },
    /// Run rule test cases.
    Test {
        /// Path to a rule file or directory of rule files.
        path: String,
        /// Run only tests matching this substring.
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// List available rules.
    List {
        /// Path to the rules directory (defaults to ./rules).
        #[arg(short, long)]
        dir: Option<String>,
        /// Filter by category prefix (e.g. "logical/join").
        #[arg(short, long)]
        category: Option<String>,
        /// Filter by tag.
        #[arg(short, long)]
        tag: Option<String>,
    },
    /// Show rule collection statistics and duplicate analysis.
    Stats {
        /// Path to the rules directory (defaults to ./rules).
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Show details for a specific rule.
    Show {
        /// Rule ID to look up.
        rule_id: String,
        /// Path to the rules directory (defaults to ./rules).
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Explain a SQL query's relational algebra plan.
    Explain {
        /// SQL query to explain.
        query: String,
        /// Hardware profile for cost estimation (edge, mobile, laptop, desktop, server, gpu-server, auto).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
    },
    /// Optimize a SQL query using rewrite rules.
    Optimize {
        /// SQL query to optimize.
        query: String,
        /// Hardware profile for cost estimation (edge, mobile, laptop, desktop, server, gpu-server, auto).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
        /// Diff output format: colored, plain, side-by-side, compact.
        #[arg(long)]
        diff: Option<String>,
        /// Disable color output.
        #[arg(long)]
        no_color: bool,
        /// Resource budget profile: interactive, standard, batch,
        /// memory-constrained, unlimited.
        #[arg(long)]
        resource_budget: Option<String>,
        /// Maximum wall-clock time for optimization (e.g. "100ms", "1s", "10s").
        #[arg(long)]
        max_time: Option<String>,
        /// Maximum memory for optimization (e.g. "10MB", "500MB", "2GB").
        #[arg(long)]
        max_memory: Option<String>,
        /// Maximum number of optimization iterations.
        #[arg(long)]
        max_iterations: Option<usize>,
        /// Overflow strategy: best-so-far, original, fail.
        #[arg(long)]
        overflow_strategy: Option<String>,
    },
    /// Gather database metadata and write to a JSON file.
    GatherMetadata {
        /// Path to a schema JSON file to load (offline mode).
        #[arg(long)]
        schema: String,
        /// Output file path for gathered metadata.
        #[arg(short, long, default_value = "schema.json")]
        output: String,
    },
    /// Compare RA optimizer plan against a database EXPLAIN plan.
    Compare {
        /// SQL query to compare.
        #[arg(long)]
        sql: String,
        /// Path to a database EXPLAIN plan in JSON format.
        #[arg(long)]
        explain_json: String,
        /// Hardware profile for cost estimation.
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
    },
    /// Launch interactive TUI for real-time plan monitoring.
    Tui {
        /// Path to a timeline JSON file to load.
        #[arg(long)]
        timeline: Option<String>,
        /// Run with built-in demo data.
        #[arg(long)]
        demo: bool,
        /// Run in headless mode (no terminal UI, for testing).
        #[arg(long)]
        headless: bool,
    },
    /// Statistics timeline commands (play, feedback, visualize).
    #[command(subcommand)]
    StatsTimeline(StatsTimelineCommands),
    /// Federated query analysis commands.
    #[command(subcommand)]
    Federated(FederatedCommands),
}

#[derive(Subcommand)]
enum StatsTimelineCommands {
    /// Replay a statistics timeline with streaming output.
    Play {
        /// Path to a timeline TOML file.
        #[arg(long)]
        timeline: String,
        /// Output format (table, json, ascii, html).
        #[arg(long, default_value = "table")]
        format: String,
        /// Playback speed multiplier (0 = instant).
        #[arg(long, default_value = "0")]
        speed: f64,
    },
    /// Simulate batch execution with feedback loop.
    Feedback {
        /// Path to a timeline TOML file.
        #[arg(long)]
        timeline: String,
        /// Output format (table, json, ascii, html).
        #[arg(long, default_value = "table")]
        format: String,
        /// Number of feedback entries per batch.
        #[arg(long, default_value = "5")]
        batch_size: usize,
    },
    /// Generate cost/cardinality evolution charts.
    Visualize {
        /// Path to a timeline TOML file.
        #[arg(long)]
        timeline: String,
        /// Output format (table, json, ascii, html).
        #[arg(long, default_value = "ascii")]
        format: String,
    },
}

#[derive(Subcommand)]
enum FederatedCommands {
    /// Analyze a federated query's execution strategy.
    Analyze {
        /// SQL query to analyze.
        #[arg(long)]
        query: String,
        /// Remote database connection string or type.
        #[arg(long)]
        remote_db: String,
        /// Remote table name.
        #[arg(long)]
        remote_table: String,
        /// Estimated network latency in milliseconds.
        #[arg(long, default_value = "10")]
        latency: u64,
        /// Estimated bandwidth in Mbps.
        #[arg(long, default_value = "100")]
        bandwidth: u64,
        /// Estimated row count of the remote table.
        #[arg(long, default_value = "1000000")]
        remote_rows: f64,
        /// Average row size in bytes.
        #[arg(long, default_value = "200")]
        avg_row_size: u64,
    },
}

// ── Main ────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        "debug"
    } else if cli.quiet {
        "error"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();

    match cli.command {
        Commands::Validate { path } => {
            cmd_validate(&path, cli.verbose, cli.quiet)
        }
        Commands::Test { path, filter } => {
            cmd_test(
                &path,
                filter.as_deref(),
                cli.verbose,
                cli.quiet,
            )
        }
        Commands::List { dir, category, tag } => {
            let dir = dir.as_deref().unwrap_or("rules");
            cmd_list(dir, category.as_deref(), tag.as_deref(), cli.quiet)
        }
        Commands::Stats { dir } => {
            let dir = dir.as_deref().unwrap_or("rules");
            cmd_stats(dir, cli.verbose, cli.quiet)
        }
        Commands::Show { rule_id, dir } => {
            let dir = dir.as_deref().unwrap_or("rules");
            cmd_show(&rule_id, dir)
        }
        Commands::Explain { query, hardware_profile } => {
            cmd_explain(&query, &hardware_profile, cli.verbose, cli.quiet)
        }
        Commands::Optimize {
            query,
            hardware_profile,
            diff,
            no_color,
            resource_budget,
            max_time,
            max_memory,
            max_iterations,
            overflow_strategy,
        } => {
            let budget = build_resource_budget(
                resource_budget.as_deref(),
                max_time.as_deref(),
                max_memory.as_deref(),
                max_iterations,
                overflow_strategy.as_deref(),
            )?;
            cmd_optimize(
                &query,
                &hardware_profile,
                diff.as_deref(),
                no_color,
                budget.as_ref(),
                cli.verbose,
                cli.quiet,
            )
        }
        Commands::GatherMetadata { schema, output } => {
            cmd_gather_metadata(&schema, &output, cli.verbose, cli.quiet)
        }
        Commands::Compare {
            sql,
            explain_json,
            hardware_profile,
        } => {
            cmd_compare(
                &sql,
                &explain_json,
                &hardware_profile,
                cli.verbose,
                cli.quiet,
            )
        }
        Commands::Tui {
            timeline,
            demo,
            headless,
        } => cmd_tui(timeline.as_deref(), demo, headless),
        Commands::StatsTimeline(sub) => match sub {
            StatsTimelineCommands::Play {
                timeline,
                format,
                speed,
            } => {
                let fmt =
                    stats_commands::OutputFormat::from_str_arg(&format)?;
                stats_commands::cmd_stats_play(
                    &timeline,
                    fmt,
                    speed,
                    cli.verbose,
                )
            }
            StatsTimelineCommands::Feedback {
                timeline,
                format,
                batch_size,
            } => {
                let fmt =
                    stats_commands::OutputFormat::from_str_arg(&format)?;
                stats_commands::cmd_stats_feedback(
                    &timeline,
                    fmt,
                    batch_size,
                    cli.verbose,
                )
            }
            StatsTimelineCommands::Visualize {
                timeline,
                format,
            } => {
                let fmt =
                    stats_commands::OutputFormat::from_str_arg(&format)?;
                stats_commands::cmd_stats_visualize(
                    &timeline,
                    fmt,
                    cli.verbose,
                )
            }
        },
        Commands::Federated(sub) => match sub {
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
                cli.verbose,
                cli.quiet,
            ),
        },
    }
}

// ── validate ────────────────────────────────────────────────

fn cmd_validate(
    path: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!(
            "Validating {} file(s)",
            files.len()
        ));
    }

    let mut pass = 0u32;
    let mut fail = 0u32;

    for file in &files {
        let source = std::fs::read_to_string(file)
            .with_context(|| {
                format!("reading {}", file.display())
            })?;

        match parse_rule_file(&source) {
            Ok(rule) => {
                let extra_errors =
                    validate_metadata_all(&rule.metadata);
                if extra_errors.is_empty() {
                    pass += 1;
                    if verbose {
                        print_status(
                            "PASS",
                            &file.display().to_string(),
                            true,
                        );
                    }
                } else {
                    fail += 1;
                    print_status(
                        "FAIL",
                        &file.display().to_string(),
                        false,
                    );
                    for err in &extra_errors {
                        print_detail(&format!("  {err}"));
                    }
                }
            }
            Err(e) => {
                fail += 1;
                print_status(
                    "FAIL",
                    &file.display().to_string(),
                    false,
                );
                print_parse_error(&e, file);
            }
        }
    }

    if !quiet {
        print_summary(pass, fail);
    }

    if fail > 0 {
        bail!("{fail} file(s) failed validation");
    }

    Ok(())
}

// ── test ────────────────────────────────────────────────────

fn cmd_test(
    path: &str,
    filter: Option<&str>,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!(
            "Running tests from {} file(s)...",
            files.len()
        ));
    }

    let (results, summary) =
        run_tests(&files, filter, verbose)?;

    if !quiet {
        for result in &results {
            match &result.outcome {
                TestOutcome::Pass => {
                    if verbose {
                        eprintln!(
                            "  {} {} ({}ms)",
                            "[PASS]".green().bold(),
                            result.name,
                            result.duration.as_millis(),
                        );
                    }
                }
                TestOutcome::Fail { reason } => {
                    eprintln!(
                        "  {} {}",
                        "[FAIL]".red().bold(),
                        result.name,
                    );
                    eprintln!(
                        "        {}",
                        reason.yellow()
                    );
                }
                TestOutcome::Skip { reason } => {
                    if verbose {
                        eprintln!(
                            "  {} {} ({})",
                            "[SKIP]".dimmed().bold(),
                            result.name,
                            reason.dimmed(),
                        );
                    }
                }
                TestOutcome::Error { message } => {
                    eprintln!(
                        "  {} {} ({})",
                        "[ERR]".red().bold(),
                        result.name,
                        message.red(),
                    );
                }
            }
        }

        eprintln!();
        let pass_rate = if summary.total > 0 {
            #[allow(clippy::cast_precision_loss)]
            let rate = summary.passed as f64
                / summary.total as f64
                * 100.0;
            format!("{rate:.1}%")
        } else {
            "N/A".to_owned()
        };

        let status_line = format!(
            "Test Results: {}/{} passed ({pass_rate})",
            summary.passed, summary.total,
        );

        if summary.failed == 0 && summary.errored == 0 {
            eprintln!("{}", status_line.green().bold());
        } else {
            eprintln!("{}", status_line.bold());
        }

        if summary.failed > 0 {
            eprintln!(
                "  {}: {} tests",
                "Failed".red().bold(),
                summary.failed,
            );
        }
        if summary.skipped > 0 {
            eprintln!(
                "  {}: {} tests",
                "Skipped".dimmed(),
                summary.skipped,
            );
        }
        if summary.errored > 0 {
            eprintln!(
                "  {}: {} tests",
                "Errors".red(),
                summary.errored,
            );
        }
        eprintln!(
            "  {}: {:.1}s",
            "Duration".dimmed(),
            summary.duration.as_secs_f64(),
        );
    }

    if summary.failed > 0 {
        bail!(
            "{} test(s) failed",
            summary.failed
        );
    }

    Ok(())
}

// ── list ────────────────────────────────────────────────────

fn cmd_list(
    dir: &str,
    category_filter: Option<&str>,
    tag_filter: Option<&str>,
    quiet: bool,
) -> Result<()> {
    let rules_dir = Path::new(dir);
    if !rules_dir.is_dir() {
        bail!(
            "rules directory not found: {dir}\n\
             hint: pass --dir <path> or run from the repo root"
        );
    }

    let files = collect_rra_files(dir)?;

    if files.is_empty() {
        if !quiet {
            eprintln!("{}", "No .rra files found.".dimmed());
        }
        return Ok(());
    }

    let mut entries: Vec<(String, String, String, PathBuf)> =
        Vec::new();

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let Ok(meta) = parse_metadata(&source) else {
            continue;
        };

        // Apply category filter
        if let Some(cat) = category_filter {
            if !meta.category.starts_with(cat) {
                continue;
            }
        }

        // Apply tag filter
        if let Some(tag) = tag_filter {
            if !meta.tags.iter().any(|t| {
                t.eq_ignore_ascii_case(tag)
            }) {
                continue;
            }
        }

        entries.push((
            meta.id,
            meta.name,
            meta.category,
            file.clone(),
        ));
    }

    entries.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));

    if !quiet {
        let mut header =
            format!("{} rule(s) found", entries.len());
        if let Some(cat) = category_filter {
            header.push_str(&format!(" in category '{cat}'"));
        }
        if let Some(tag) = tag_filter {
            header.push_str(&format!(" with tag '{tag}'"));
        }
        print_header(&header);
    }

    let id_w = entries
        .iter()
        .map(|e| e.0.len())
        .max()
        .unwrap_or(2)
        .max(2);
    let name_w = entries
        .iter()
        .map(|e| e.1.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let cat_w = entries
        .iter()
        .map(|e| e.2.len())
        .max()
        .unwrap_or(8)
        .max(8);

    eprintln!(
        "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
        "ID".bold(),
        "NAME".bold(),
        "CATEGORY".bold(),
    );
    eprintln!(
        "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
        "-".repeat(id_w),
        "-".repeat(name_w),
        "-".repeat(cat_w),
    );

    for (id, name, category, _path) in &entries {
        eprintln!(
            "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
            id.cyan(),
            name,
            category.dimmed(),
        );
    }

    Ok(())
}

// ── stats ──────────────────────────────────────────────────

fn cmd_stats(
    dir: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let rules_dir = Path::new(dir);
    if !rules_dir.is_dir() {
        bail!(
            "rules directory not found: {dir}\n\
             hint: pass --dir <path> or run from the repo root"
        );
    }

    let files = collect_rra_files(dir)?;

    if files.is_empty() {
        if !quiet {
            eprintln!("{}", "No .rra files found.".dimmed());
        }
        return Ok(());
    }

    let mut by_category: std::collections::BTreeMap<
        String,
        Vec<String>,
    > = std::collections::BTreeMap::new();
    let mut by_id: std::collections::HashMap<
        String,
        Vec<PathBuf>,
    > = std::collections::HashMap::new();
    let mut parse_ok = 0u32;
    let mut parse_fail = 0u32;
    let mut valid_ok = 0u32;
    let mut valid_fail = 0u32;

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            parse_fail += 1;
            continue;
        };
        match parse_rule_file(&source) {
            Ok(rule) => {
                parse_ok += 1;
                let errs =
                    validate_metadata_all(&rule.metadata);
                if errs.is_empty() {
                    valid_ok += 1;
                } else {
                    valid_fail += 1;
                }

                let cat_prefix = rule
                    .metadata
                    .category
                    .split('/')
                    .take(2)
                    .collect::<Vec<_>>()
                    .join("/");
                by_category
                    .entry(cat_prefix)
                    .or_default()
                    .push(rule.metadata.id.clone());

                by_id
                    .entry(rule.metadata.id)
                    .or_default()
                    .push(file.clone());
            }
            Err(_) => {
                parse_fail += 1;
            }
        }
    }

    let total = files.len();
    let duplicates: Vec<_> = by_id
        .iter()
        .filter(|(_, v)| v.len() > 1)
        .collect();

    if !quiet {
        print_header(&format!(
            "Rule Collection Statistics ({total} files)"
        ));

        eprintln!(
            "  {}: {}",
            "Total .rra files".bold(),
            total
        );
        eprintln!(
            "  {}: {} ({} failed)",
            "Parsed successfully".bold(),
            parse_ok,
            parse_fail,
        );
        eprintln!(
            "  {}: {} ({} with issues)",
            "Validated".bold(),
            valid_ok,
            valid_fail,
        );
        eprintln!(
            "  {}: {}",
            "Unique rule IDs".bold(),
            by_id.len()
        );
        eprintln!(
            "  {}: {}",
            "Duplicate IDs".bold(),
            duplicates.len()
        );
        eprintln!(
            "  {}: {}",
            "Categories".bold(),
            by_category.len()
        );

        eprintln!();
        eprintln!("{}", "Rules by Category:".bold());
        for (cat, rules) in &by_category {
            eprintln!(
                "  {:>4}  {}",
                rules.len().to_string().cyan(),
                cat,
            );
        }

        if !duplicates.is_empty() {
            eprintln!();
            eprintln!("{}", "Duplicate Rule IDs:".bold());
            for (id, paths) in &duplicates {
                eprintln!(
                    "  {} ({}x):",
                    id.yellow(),
                    paths.len()
                );
                if verbose {
                    for p in *paths {
                        eprintln!(
                            "    - {}",
                            p.display()
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

// ── show ────────────────────────────────────────────────────

fn cmd_show(rule_id: &str, dir: &str) -> Result<()> {
    let files = collect_rra_files(dir)?;

    let Some((rule, path)) = find_rule_by_id(rule_id, &files)
    else {
        bail!(
            "rule '{rule_id}' not found in {dir}\n\
             hint: run 'ra-cli list' to see available rules"
        );
    };

    eprintln!(
        "{}",
        format!("Rule: {}", rule.metadata.id).bold()
    );
    eprintln!("  {}: {}", "Name".bold(), rule.metadata.name);
    eprintln!(
        "  {}: {}",
        "Category".bold(),
        rule.metadata.category
    );
    eprintln!(
        "  {}: {}",
        "Version".bold(),
        rule.metadata.version
    );
    eprintln!("  {}: {}", "File".bold(), path.display());

    if !rule.metadata.databases.is_empty() {
        eprintln!(
            "  {}: {}",
            "Databases".bold(),
            rule.metadata.databases.join(", ")
        );
    }
    if !rule.metadata.authors.is_empty() {
        eprintln!(
            "  {}: {}",
            "Authors".bold(),
            rule.metadata.authors.join(", ")
        );
    }
    if !rule.metadata.tags.is_empty() {
        eprintln!(
            "  {}: {}",
            "Tags".bold(),
            rule.metadata.tags.join(", ")
        );
    }
    if let Some(ref std) = rule.metadata.standard {
        eprintln!("  {}: {std}", "Standard".bold());
    }

    if !rule.description.is_empty() {
        eprintln!();
        eprintln!("{}", "Description:".bold());
        for line in rule.description.lines() {
            eprintln!("  {line}");
        }
    }

    if let Some(ref alg) = rule.algebra_notation {
        eprintln!();
        eprintln!("{}", "Relational Algebra:".bold());
        for line in alg.lines() {
            eprintln!("  {}", line.green());
        }
    }

    if let Some(ref impl_code) = rule.implementation {
        eprintln!();
        eprintln!("{}", "Implementation:".bold());
        for line in impl_code.lines() {
            eprintln!("  {line}");
        }
    }

    if !rule.test_cases.is_empty() {
        eprintln!();
        eprintln!(
            "{}",
            format!(
                "Test Cases: {} block(s)",
                rule.test_cases.len()
            )
            .bold()
        );
    }

    if !rule.references.is_empty() {
        eprintln!();
        eprintln!("{}", "References:".bold());
        for r in &rule.references {
            eprintln!("  - {r}");
        }
    }

    Ok(())
}

// ── explain ─────────────────────────────────────────────────

fn cmd_explain(query: &str, hardware_profile_name: &str, verbose: bool, quiet: bool) -> Result<()> {
    let plan = sql_to_relexpr(query)
        .with_context(|| format!("failed to parse SQL: {query}"))?;

    let hardware = load_hardware_profile(hardware_profile_name)?;

    if !quiet {
        print_header("Query Plan Explanation");
        eprintln!("  {}: {query}", "SQL".bold());

        if verbose {
            eprintln!("  {}: {} ({} cores, {} MB L3 cache, {}-bit SIMD)",
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

    Ok(())
}

// ── optimize ────────────────────────────────────────────────

fn cmd_optimize(
    query: &str,
    hardware_profile_name: &str,
    diff_format: Option<&str>,
    no_color: bool,
    budget: Option<&ra_engine::ResourceBudget>,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let color_mode = if no_color {
        plan_diff::ColorMode::Never
    } else if std::env::var("FORCE_COLOR").is_ok() {
        plan_diff::ColorMode::Always
    } else {
        plan_diff::ColorMode::Auto
    };
    plan_diff::apply_color_mode(color_mode);

    let plan = sql_to_relexpr(query)
        .with_context(|| format!("failed to parse SQL: {query}"))?;
    let hardware = load_hardware_profile(hardware_profile_name)?;

    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(hardware.clone());

    if let Some(b) = budget {
        optimizer.set_resource_budget(b.clone());
    }

    if budget.is_some() {
        optimize_bounded(
            &optimizer, &plan, &hardware, diff_format, verbose,
            quiet, query,
        )
    } else {
        optimize_unbounded(
            &optimizer, &plan, &hardware, diff_format, verbose,
            quiet, query,
        )
    }
}

fn optimize_bounded(
    optimizer: &Optimizer,
    plan: &ra_core::algebra::RelExpr,
    hardware: &ra_hardware::HardwareProfile,
    diff_format: Option<&str>,
    verbose: bool,
    quiet: bool,
    query: &str,
) -> Result<()> {
    let result = optimizer.optimize_bounded(plan).with_context(|| {
        format!("failed to optimize query: {query}")
    })?;

    if !quiet {
        print_optimization_header(
            "Query Optimization (Resource-Bounded)",
            query,
            hardware,
            verbose,
        );
        print_resource_usage(&result, verbose);
        eprintln!();
        print_plan_output(plan, &result.plan, diff_format)?;
    }
    Ok(())
}

fn optimize_unbounded(
    optimizer: &Optimizer,
    plan: &ra_core::algebra::RelExpr,
    hardware: &ra_hardware::HardwareProfile,
    diff_format: Option<&str>,
    verbose: bool,
    quiet: bool,
    query: &str,
) -> Result<()> {
    let optimized = optimizer.optimize(plan).with_context(|| {
        format!("failed to optimize query: {query}")
    })?;

    if !quiet {
        print_optimization_header(
            "Query Optimization",
            query,
            hardware,
            verbose,
        );
        print_plan_output(plan, &optimized, diff_format)?;
    }
    Ok(())
}

fn print_optimization_header(
    title: &str,
    query: &str,
    hardware: &ra_hardware::HardwareProfile,
    verbose: bool,
) {
    print_header(title);
    eprintln!("  {}: {query}", "SQL".bold());
    if verbose {
        eprintln!(
            "  {}: {} ({} cores, {} MB L3, {}-bit SIMD)",
            "Hardware".bold(),
            hardware.name,
            hardware.cpu_cores,
            hardware.l3_cache_bytes / (1024 * 1024),
            hardware.simd_width_bits
        );
    }
    eprintln!();
}

fn print_plan_output(
    original: &ra_core::algebra::RelExpr,
    optimized: &ra_core::algebra::RelExpr,
    diff_format: Option<&str>,
) -> Result<()> {
    if let Some(fmt_str) = diff_format {
        let fmt = parse_diff_format(fmt_str)?;
        let diff_output =
            plan_diff::render_diff(original, optimized, fmt);
        eprintln!("{diff_output}");
    } else {
        eprintln!("{}", "Original Plan:".bold());
        eprintln!("{}", format_plan_tree(original));
        eprintln!();
        eprintln!("{}", "Optimized Plan:".bold());
        eprintln!("{}", format_plan_tree(optimized));
    }
    Ok(())
}

/// Parse a diff format string into a `DiffFormat`.
fn parse_diff_format(s: &str) -> Result<plan_diff::DiffFormat> {
    match s.to_lowercase().as_str() {
        "colored" | "color" => Ok(plan_diff::DiffFormat::Colored),
        "plain" | "text" => Ok(plan_diff::DiffFormat::Plain),
        "side-by-side" | "sbs" => {
            Ok(plan_diff::DiffFormat::SideBySide)
        }
        "compact" | "summary" => {
            Ok(plan_diff::DiffFormat::Compact)
        }
        _ => bail!(
            "unknown diff format: '{s}'. \
             Valid options: colored, plain, side-by-side, compact"
        ),
    }
}

// ── gather-metadata ────────────────────────────────────────

fn cmd_gather_metadata(
    schema_path: &str,
    output_path: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let source = std::fs::read_to_string(schema_path)
        .with_context(|| {
            format!("reading schema file: {schema_path}")
        })?;

    let schema: ra_metadata::SchemaInfo =
        serde_json::from_str(&source).with_context(|| {
            format!(
                "parsing schema JSON from: {schema_path}"
            )
        })?;

    if !quiet {
        print_header("Database Metadata");
        eprintln!(
            "  {}: {}",
            "Database".bold(),
            schema.kind
        );
        eprintln!(
            "  {}: {}",
            "Schema".bold(),
            schema.schema_name
        );
        eprintln!(
            "  {}: {}",
            "Tables".bold(),
            schema.table_count()
        );
    }

    if verbose {
        for (name, table) in &schema.tables {
            eprintln!(
                "    {}: {} columns, {} indexes",
                name.cyan(),
                table.column_count(),
                table.indexes.len(),
            );
        }
    }

    let json = serde_json::to_string_pretty(&schema)
        .context("serializing schema to JSON")?;
    std::fs::write(output_path, json).with_context(|| {
        format!("writing output to: {output_path}")
    })?;

    if !quiet {
        eprintln!();
        eprintln!(
            "{}",
            format!("Wrote metadata to {output_path}")
                .green()
                .bold()
        );
    }

    Ok(())
}

// ── compare ────────────────────────────────────────────────

fn cmd_compare(
    sql: &str,
    explain_json_path: &str,
    hardware_profile_name: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let ra_plan = sql_to_relexpr(sql)
        .with_context(|| format!("failed to parse SQL: {sql}"))?;

    let explain_source =
        std::fs::read_to_string(explain_json_path)
            .with_context(|| {
                format!(
                    "reading EXPLAIN JSON: {explain_json_path}"
                )
            })?;

    let db_explain: ra_metadata::ExplainPlan =
        serde_json::from_str(&explain_source)
            .with_context(|| {
                format!(
                    "parsing EXPLAIN JSON from: \
                     {explain_json_path}"
                )
            })?;

    let hardware =
        load_hardware_profile(hardware_profile_name)?;

    let comparison = ra_metadata::diff_validator::compare_plans(
        &ra_plan,
        &db_explain,
    );

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
        eprintln!(
            "{}",
            diff_validator::format_explain_tree(&db_explain)
        );
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
                eprintln!(
                    "  {} {}:",
                    "[DIFF]".yellow().bold(),
                    d.aspect,
                );
                eprintln!(
                    "    {}: {}",
                    "RA optimizer".bold(),
                    d.ra_choice,
                );
                eprintln!(
                    "    {}:     {}",
                    "Database".bold(),
                    d.db_choice,
                );
                eprintln!(
                    "    {}: {}",
                    "Severity".dimmed(),
                    d.severity,
                );
            }
        }
    }

    Ok(())
}

// ── tui ─────────────────────────────────────────────────────

fn cmd_tui(
    timeline_path: Option<&str>,
    demo: bool,
    headless: bool,
) -> Result<()> {
    let timeline = if demo {
        ra_tui::Timeline::demo()
    } else if let Some(path) = timeline_path {
        let source = std::fs::read_to_string(path)
            .with_context(|| {
                format!("reading timeline file: {path}")
            })?;

        // Try JSON first (native format), fall back to demo if TOML
        if path.ends_with(".json") {
            serde_json::from_str(&source).with_context(|| {
                format!("parsing timeline JSON from: {path}")
            })?
        } else if path.ends_with(".toml") {
            // TOML statistics timelines not yet supported in TUI
            // (different format - statistics evolution vs optimizer snapshots)
            eprintln!("Note: TOML statistics timelines not yet supported in TUI.");
            eprintln!("Using demo timeline instead.");
            eprintln!("Tip: Use 'ra-cli tui --demo' for the demo timeline.");
            ra_tui::Timeline::demo()
        } else {
            // Try JSON parse as fallback
            serde_json::from_str(&source).with_context(|| {
                format!("parsing timeline from: {path}")
            })?
        }
    } else {
        bail!(
            "specify --demo for demo data or \
             --timeline <path> to load a timeline file"
        );
    };

    let mut app =
        ra_tui::App::new(timeline).context("initializing TUI")?;

    if headless {
        let final_cost = app
            .run_headless()
            .context("running headless TUI")?;
        eprintln!(
            "Headless run complete. Final cost: {final_cost:.0}"
        );
        return Ok(());
    }

    app.run().context("running TUI")?;

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────

/// Load a hardware profile by name.
fn load_hardware_profile(name: &str) -> Result<ra_hardware::HardwareProfile> {
    let profile = match name.to_lowercase().as_str() {
        "auto" => ra_hardware::detect_hardware(),
        "cpu-only" => ra_hardware::HardwareProfile::cpu_only(),
        "gpu-server" => ra_hardware::HardwareProfile::gpu_server(),
        "fpga" => ra_hardware::HardwareProfile::fpga_appliance(),
        _ => bail!("unknown hardware profile: {name}. Valid options: auto, cpu-only, gpu-server, fpga"),
    };

    Ok(profile)
}

/// Collect all `.rra` files under a path (file or directory).
fn collect_rra_files(path: &str) -> Result<Vec<PathBuf>> {
    let p = Path::new(path);
    if p.is_file() {
        return Ok(vec![p.to_path_buf()]);
    }
    if !p.is_dir() {
        bail!("path not found: {path}");
    }
    let mut files = Vec::new();
    walk_dir(p, &mut files)?;
    files.sort();
    Ok(files)
}

/// Recursively walk a directory for `.rra` files.
fn walk_dir(
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "rra")
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Search for a rule by ID across a set of files.
fn find_rule_by_id(
    rule_id: &str,
    files: &[PathBuf],
) -> Option<(RuleFile, PathBuf)> {
    for file in files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        if let Ok(rule) = parse_rule_file(&source) {
            if rule.metadata.id == rule_id {
                return Some((rule, file.clone()));
            }
        }
    }
    None
}

// ── Resource budget helpers ──────────────────────────────────

/// Build a [`ResourceBudget`] from CLI flags.
fn build_resource_budget(
    profile: Option<&str>,
    max_time: Option<&str>,
    max_memory: Option<&str>,
    max_iterations: Option<usize>,
    overflow_strategy: Option<&str>,
) -> Result<Option<ra_engine::ResourceBudget>> {
    let has_custom = max_time.is_some()
        || max_memory.is_some()
        || max_iterations.is_some()
        || overflow_strategy.is_some();

    if profile.is_none() && !has_custom {
        return Ok(None);
    }

    let mut budget = match profile {
        Some("interactive") => ra_engine::ResourceBudget::interactive(),
        Some("standard") => ra_engine::ResourceBudget::standard(),
        Some("batch") => ra_engine::ResourceBudget::batch(),
        Some("memory-constrained") => {
            ra_engine::ResourceBudget::memory_constrained()
        }
        Some("unlimited") => ra_engine::ResourceBudget::unlimited(),
        Some(other) => bail!(
            "unknown resource budget profile: '{other}'. \
             Valid: interactive, standard, batch, \
             memory-constrained, unlimited"
        ),
        None => ra_engine::ResourceBudget::standard(),
    };

    if let Some(t) = max_time {
        budget = budget.with_time_limit(parse_duration(t)?);
    }
    if let Some(m) = max_memory {
        budget = budget.with_memory_limit(parse_byte_size(m)?);
    }
    if let Some(n) = max_iterations {
        budget = budget.with_iteration_limit(n);
    }
    if let Some(s) = overflow_strategy {
        budget = budget.with_overflow_strategy(parse_overflow(s)?);
    }

    Ok(Some(budget))
}

/// Parse a human-readable duration string (e.g. "100ms", "1s", "10s").
fn parse_duration(s: &str) -> Result<std::time::Duration> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        let n: u64 = ms
            .trim()
            .parse()
            .context("invalid millisecond value")?;
        return Ok(std::time::Duration::from_millis(n));
    }
    if let Some(secs) = s.strip_suffix('s') {
        let n: u64 =
            secs.trim().parse().context("invalid seconds value")?;
        return Ok(std::time::Duration::from_secs(n));
    }
    // Default to seconds
    let n: u64 = s.parse().context(
        "invalid duration; use e.g. '100ms' or '1s'",
    )?;
    Ok(std::time::Duration::from_secs(n))
}

/// Parse a human-readable byte size (e.g. "10MB", "500MB", "2GB").
fn parse_byte_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let upper = s.to_uppercase();
    if let Some(gb) = upper.strip_suffix("GB") {
        let n: u64 =
            gb.trim().parse().context("invalid GB value")?;
        return Ok(n.saturating_mul(1024 * 1024 * 1024));
    }
    if let Some(mb) = upper.strip_suffix("MB") {
        let n: u64 =
            mb.trim().parse().context("invalid MB value")?;
        return Ok(n.saturating_mul(1024 * 1024));
    }
    if let Some(kb) = upper.strip_suffix("KB") {
        let n: u64 =
            kb.trim().parse().context("invalid KB value")?;
        return Ok(n.saturating_mul(1024));
    }
    s.parse::<u64>().context(
        "invalid byte size; use e.g. '10MB', '2GB', or raw bytes",
    )
}

/// Parse an overflow strategy string.
fn parse_overflow(
    s: &str,
) -> Result<ra_engine::OverflowStrategy> {
    match s.to_lowercase().as_str() {
        "best-so-far" | "best" => {
            Ok(ra_engine::OverflowStrategy::ReturnBestSoFar)
        }
        "original" => {
            Ok(ra_engine::OverflowStrategy::ReturnOriginal)
        }
        "fail" => Ok(ra_engine::OverflowStrategy::Fail),
        _ => bail!(
            "unknown overflow strategy: '{s}'. \
             Valid: best-so-far, original, fail"
        ),
    }
}

/// Display resource usage from a bounded optimization result.
fn print_resource_usage(
    result: &ra_engine::OptimizationResult,
    verbose: bool,
) {
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
    eprintln!(
        "  {}: {}",
        "Iterations".bold(),
        usage.iterations_used,
    );
    eprintln!(
        "  {}: {}",
        "Peak e-graph nodes".bold(),
        usage.peak_egraph_nodes,
    );

    if verbose {
        #[allow(clippy::cast_precision_loss)]
        let mem_mb =
            usage.peak_memory_estimate as f64 / (1024.0 * 1024.0);
        eprintln!(
            "  {}: {mem_mb:.2} MB",
            "Peak memory (est.)".bold(),
        );
        eprintln!(
            "  {}: {:.2}",
            "Plan cost".bold(),
            result.cost,
        );
    }
}

// ── Output formatting ───────────────────────────────────────

fn print_header(msg: &str) {
    eprintln!();
    eprintln!("{}", msg.bold());
    eprintln!();
}

fn print_status(label: &str, detail: &str, ok: bool) {
    if ok {
        eprintln!(
            "  {} {detail}",
            format!("[{label}]").green().bold(),
        );
    } else {
        eprintln!(
            "  {} {detail}",
            format!("[{label}]").red().bold(),
        );
    }
}

fn print_detail(msg: &str) {
    eprintln!("        {}", msg.yellow());
}

fn print_parse_error(err: &ParseError, path: &Path) {
    match err {
        ParseError::MissingFrontmatter => {
            print_detail(&format!(
                "{}: missing YAML frontmatter (---)",
                path.display()
            ));
        }
        ParseError::InvalidYaml { line, source } => {
            print_detail(&format!(
                "{}:{line}: {source}",
                path.display()
            ));
        }
        ParseError::Validation(v) => {
            print_detail(&format!(
                "{}: {v}",
                path.display()
            ));
        }
        ParseError::Other(msg) => {
            print_detail(&format!(
                "{}: {msg}",
                path.display()
            ));
        }
    }
}

fn print_summary(pass: u32, fail: u32) {
    eprintln!();
    let total = pass + fail;
    if fail == 0 {
        eprintln!(
            "{}",
            format!("All {total} file(s) passed validation.")
                .green()
                .bold()
        );
    } else {
        eprintln!(
            "{}: {pass} passed, {fail} failed out of {total}",
            "Summary".bold(),
        );
    }
}
