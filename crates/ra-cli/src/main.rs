//! Command-line interface for the relational algebra rule system.
#![allow(clippy::print_stderr)]

mod cache_commands;
mod config_commands;
mod diff_validator;
mod display;
mod federated_commands;
mod migrate_commands;
pub(crate) mod plan_diff;
mod regression_commands;
pub(crate) mod side_by_side;
mod stats_commands;
mod test_executor;
mod visualize;

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use colored::Colorize;

use ra_engine::Optimizer;
use ra_parser::{
    parse_metadata, parse_rule_file, sql_to_relexpr, validate_metadata_all, ParseError, RuleFile,
};

use display::format_plan_tree;
use test_executor::{run_tests, FileResult, TestOutcome, TestResult};

// ── CLI definition ──────────────────────────────────────────

#[derive(Parser)]
#[command(name = "ra-cli")]
#[command(
    about = "Ra -- the relational algebra query optimizer toolkit",
    long_about = "Ra is a toolkit for analyzing, optimizing, and testing SQL queries \
        using relational algebra rewrite rules.\n\n\
        Common workflows:\n  \
        ra-cli explain 'SELECT ...'     Parse SQL into a relational algebra plan\n  \
        ra-cli optimize 'SELECT ...'    Optimize a SQL query with rewrite rules\n  \
        ra-cli validate rules/          Validate .rra rule files\n  \
        ra-cli test rules/              Run embedded test cases in rule files\n  \
        ra-cli list                     List available optimization rules\n\n\
        Use --help on any subcommand for detailed usage.",
    version,
    propagate_version = true,
    after_help = "See 'ra-cli <command> --help' for details on a specific command."
)]
struct Cli {
    /// Increase output verbosity (show per-file results, debug info).
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
    /// Validate .rra rule files for correct syntax and metadata.
    #[command(long_about = "Validate one or more .rra rule files.\n\n\
            Checks YAML frontmatter syntax, required fields (id, name, category, version), \
            and category format. Exits with code 1 if any file fails.\n\n\
            Examples:\n  \
            ra-cli validate rules/filter-pushdown.rra\n  \
            ra-cli validate rules/              # scan entire directory\n  \
            ra-cli --verbose validate rules/    # show per-file PASS/FAIL")]
    Validate {
        /// Path to a .rra file or directory to scan recursively.
        path: String,
    },
    /// Run embedded test cases defined in .rra rule files.
    #[command(
        long_about = "Execute the test cases embedded in .rra rule files and report results.\n\n\
            Each test case specifies an input SQL, expected plan, or expected optimization. \
            Results include pass/fail counts and timing.\n\n\
            Examples:\n  \
            ra-cli test rules/\n  \
            ra-cli test rules/ --filter pushdown\n  \
            ra-cli test rules/join-commutativity.rra --verbose"
    )]
    Test {
        /// Path to a .rra file or directory to scan recursively.
        path: String,
        /// Run only tests whose name contains this substring.
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// List available optimization rules.
    #[command(
        long_about = "Display a table of all valid .rra rules in a directory.\n\n\
            Shows rule ID, name, and category. Use --category or --tag to filter.\n\n\
            Examples:\n  \
            ra-cli list\n  \
            ra-cli list --dir rules/ --category logical/join"
    )]
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
    /// Show detailed metadata for a specific rule by ID.
    #[command(
        long_about = "Look up a rule by its ID and display all metadata sections.\n\n\
            Shows name, category, description, relational algebra, implementation notes, \
            and test cases. Searches the rules directory for a matching ID.\n\n\
            Examples:\n  \
            ra-cli show filter-pushdown-basic\n  \
            ra-cli show join-commutativity --dir rules/"
    )]
    Show {
        /// Rule ID to look up (e.g. "filter-pushdown-basic").
        rule_id: String,
        /// Path to the rules directory (defaults to ./rules).
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Parse a SQL query into a relational algebra plan tree.
    #[command(
        long_about = "Parse SQL into relational algebra and display the unoptimized plan tree.\n\n\
            Useful for understanding how Ra represents a query before optimization.\n\n\
            Examples:\n  \
            ra-cli explain 'SELECT * FROM orders WHERE amount > 100'\n  \
            echo 'SELECT 1' | ra-cli explain --stdin\n  \
            ra-cli explain 'SELECT ...' --hardware-profile server"
    )]
    Explain {
        /// SQL query to explain (ignored when --stdin is set).
        #[arg(default_value = "")]
        query: String,
        /// Hardware profile for cost estimation (edge, mobile, laptop, desktop, server, gpu-server, auto).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
        /// Read SQL from stdin instead of the positional argument.
        #[arg(long)]
        stdin: bool,
    },
    /// Optimize a SQL query using relational algebra rewrite rules.
    #[command(
        long_about = "Parse SQL, apply optimization rules, and show the resulting plan.\n\n\
            Supports resource budgets, diff output, database-specific EXPLAIN formats, \
            and optimizer tracing.\n\n\
            Examples:\n  \
            ra-cli optimize 'SELECT * FROM users WHERE active = true'\n  \
            ra-cli optimize 'SELECT ...' --diff side-by-side\n  \
            ra-cli optimize 'SELECT ...' --explain-format postgres\n  \
            echo 'SELECT ...' | ra-cli optimize --stdin --trace"
    )]
    Optimize {
        /// SQL query to optimize (ignored when --stdin is set).
        #[arg(default_value = "")]
        query: String,
        /// Hardware profile for cost estimation (edge, mobile, laptop, desktop, server, gpu-server, auto).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
        /// Read SQL from stdin instead of the positional argument.
        #[arg(long)]
        stdin: bool,
        /// Diff output format: colored, plain, side-by-side, compact. Defaults to colored if no format specified.
        #[arg(long, value_name = "FORMAT", default_missing_value = "colored", num_args = 0..=1)]
        diff: Option<String>,
        /// Disable color output.
        #[arg(long)]
        no_color: bool,
        /// Resource budget profile: interactive, standard, batch,
        /// memory-constrained, unlimited. Default: unbounded
        /// (unless --rules-* flags are used, then defaults to standard).
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
        /// Output EXPLAIN in a database-specific format: postgres, mysql, sqlite, ascii (default).
        #[arg(long)]
        explain_format: Option<String>,
        /// Show optimizer trace information (iteration details, search/apply times).
        #[arg(long)]
        trace: bool,
        /// Show optimization statistics (planning time, iterations, nodes explored, etc.).
        #[arg(long)]
        stats: bool,
        /// Show only rules that modified the e-graph (applied rules).
        #[arg(long)]
        rules_applied: bool,
        /// Show rules that were tried but rejected, with reasons.
        #[arg(long)]
        rules_evaluated: bool,
        /// Show all rules available in the system.
        #[arg(long)]
        rules_available: bool,
        /// Show all three rule categories (applied, evaluated, available).
        #[arg(long)]
        rules_all: bool,
        /// Deprecated: use --rules-applied, --rules-evaluated, --rules-available, or --rules-all.
        #[arg(long, hide = true)]
        rules: bool,
    },
    /// Gather database metadata and write to a JSON file.
    GatherMetadata {
        /// Database connection URL for live gathering
        /// (postgresql://, mysql://, sqlite://, or .db file path).
        #[arg(long)]
        db: Option<String>,
        /// Path to a schema JSON file to load (offline mode).
        #[arg(long)]
        schema: Option<String>,
        /// Output file path for gathered metadata.
        #[arg(short, long, default_value = "schema.json")]
        output: String,
    },
    /// Compare RA optimizer plan against a database EXPLAIN plan.
    Compare {
        /// SQL query to compare.
        #[arg(long)]
        sql: String,
        /// Database connection URL for live EXPLAIN
        /// (postgresql://, mysql://, sqlite://).
        #[arg(long)]
        db: Option<String>,
        /// Path to a database EXPLAIN plan in JSON format (offline).
        #[arg(long)]
        explain_json: Option<String>,
        /// Path to a schema JSON file (used with --db for stats).
        #[arg(long)]
        schema: Option<String>,
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
        /// Record an asciinema .cast file of the session.
        #[arg(long)]
        record: Option<String>,
    },
    /// Statistics timeline commands (play, feedback, visualize).
    #[command(subcommand)]
    StatsTimeline(StatsTimelineCommands),
    /// Format a SQL query with configurable style.
    Format {
        /// SQL query to format (omit to read from stdin).
        query: Option<String>,
        /// Read SQL from stdin.
        #[arg(long)]
        stdin: bool,
        /// Keyword capitalization: keywords, all, none.
        #[arg(long, default_value = "keywords")]
        capitalize: String,
        /// Indentation: spaces2, spaces4, tab.
        #[arg(long, default_value = "spaces2")]
        indent: String,
    },
    /// Translate SQL between database dialects.
    Translate {
        /// SQL query to translate.
        query: String,
        /// Source dialect: postgresql, mysql, sqlite, duckdb, mssql, oracle.
        #[arg(long)]
        from: String,
        /// Target dialect: postgresql, mysql, sqlite, duckdb, mssql, oracle.
        #[arg(long)]
        to: String,
    },
    /// Analyze triggers on a table and estimate DML costs.
    AnalyzeTriggers {
        /// Table name to analyze.
        table: String,
        /// Database connection URL (postgresql://, mysql://, sqlite://).
        #[arg(long)]
        database_url: Option<String>,
        /// Path to a schema JSON file (offline mode).
        #[arg(long)]
        schema: Option<String>,
    },
    /// Federated query analysis commands.
    #[command(subcommand)]
    Federated(FederatedCommands),
    /// Manage configuration settings.
    #[command(subcommand)]
    Config(ConfigCommands),
    /// Plan cache management.
    #[command(subcommand)]
    Cache(CacheCommands),
    /// Migrate rule pre-conditions from prose to formal YAML.
    #[command(subcommand)]
    Migrate(MigrateCommands),
    /// Monitor a PostgreSQL database with schema analysis and tuning advice.
    Monitor {
        /// PostgreSQL connection string (e.g. "host=localhost dbname=prod").
        #[arg(long)]
        postgres: Option<String>,
        /// Launch interactive TUI dashboard.
        #[arg(long)]
        tui: bool,
        /// Run with demo data (no database required).
        #[arg(long)]
        demo: bool,
        /// Output format for non-TUI mode: text, json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Query regression detection commands.
    #[command(subcommand)]
    Regression(RegressionCommands),
    /// Generate shell completion scripts.
    #[command(long_about = "Generate tab-completion scripts for your shell.\n\n\
            Source the output in your shell profile to enable completions.\n\n\
            Examples:\n  \
            ra-cli completions bash  > ~/.local/share/bash-completion/completions/ra-cli\n  \
            ra-cli completions zsh   > ~/.zfunc/_ra-cli\n  \
            ra-cli completions fish  > ~/.config/fish/completions/ra-cli.fish\n  \
            ra-cli completions elvish")]
    Completions {
        /// Target shell: bash, zsh, fish, elvish, powershell.
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum RegressionCommands {
    /// Establish a baseline for a query.
    Baseline {
        /// Path to SQL query file.
        query_file: PathBuf,
        /// Query identifier (defaults to filename).
        #[arg(long)]
        query_id: Option<String>,
        /// Storage backend: sqlite or toml.
        #[arg(long, default_value = "sqlite")]
        storage: String,
        /// Path to storage file.
        #[arg(long, default_value = "regression.db")]
        storage_path: PathBuf,
        /// Hardware profile for cost estimation.
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
    },
    /// Check for regressions in a query.
    Check {
        /// Path to SQL query file.
        query_file: PathBuf,
        /// Query identifier (defaults to filename).
        #[arg(long)]
        query_id: Option<String>,
        /// Storage backend: sqlite or toml.
        #[arg(long, default_value = "sqlite")]
        storage: String,
        /// Path to storage file.
        #[arg(long, default_value = "regression.db")]
        storage_path: PathBuf,
        /// Hardware profile for cost estimation.
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
        /// Warning threshold (default: 1.25 = 25% increase).
        #[arg(long)]
        warn_threshold: Option<f64>,
        /// Error threshold (default: 2.0 = 2x increase).
        #[arg(long)]
        error_threshold: Option<f64>,
    },
    /// Show regression report for all queries.
    Report {
        /// Storage backend: sqlite or toml.
        #[arg(long, default_value = "sqlite")]
        storage: String,
        /// Path to storage file.
        #[arg(long, default_value = "regression.db")]
        storage_path: PathBuf,
        /// Output format: text, json.
        #[arg(long, default_value = "text")]
        format: String,
        /// Show only regressions (not improvements).
        #[arg(long)]
        only_regressions: bool,
    },
}

#[derive(Subcommand)]
enum MigrateCommands {
    /// Migrate pre-conditions in rule files.
    Preconditions {
        /// Path to input rule file or directory.
        #[arg(short = 'i', long)]
        input: String,
        /// Path to output directory for migrated files.
        #[arg(short = 'o', long)]
        output: String,
        /// Perform validation after migration.
        #[arg(long)]
        validate: bool,
        /// Dry run mode (show what would be migrated without writing).
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate migrated pre-conditions against baseline.
    Validate {
        /// Path to baseline rules directory.
        #[arg(short = 'b', long)]
        baseline: String,
        /// Path to migrated rules directory.
        #[arg(short = 'm', long)]
        migrated: String,
        /// Path to facts TOML file for testing.
        #[arg(short = 'f', long)]
        facts: Option<String>,
    },
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

/// What level of rule tracking information to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleDisplayMode {
    /// Show no rule information.
    None,
    /// Show only rules that modified the e-graph.
    Applied,
    /// Show rules that were tried but rejected.
    Evaluated,
    /// Show all rules available in the system.
    Available,
    /// Show all three categories.
    All,
}

impl RuleDisplayMode {
    /// Determine display mode from CLI flags.
    fn from_flags(
        applied: bool,
        evaluated: bool,
        available: bool,
        all: bool,
        deprecated_rules: bool,
    ) -> Self {
        if all {
            Self::All
        } else if applied {
            Self::Applied
        } else if evaluated {
            Self::Evaluated
        } else if available {
            Self::Available
        } else if deprecated_rules {
            // Backward compatibility: treat old --rules as --rules-applied
            Self::Applied
        } else {
            Self::None
        }
    }

    fn should_track(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// List all configuration settings.
    List,
    /// Get a specific configuration value.
    Get {
        /// Dotted key path (e.g. "editor.mode").
        key: String,
    },
    /// Set a configuration value.
    Set {
        /// Dotted key path (e.g. "editor.mode").
        key: String,
        /// New value for the setting.
        value: String,
    },
    /// Open configuration file in $EDITOR.
    Edit,
    /// Reset configuration to defaults.
    Reset,
    /// Show the configuration file path.
    Path,
}

#[derive(Subcommand)]
enum CacheCommands {
    /// Show all cached plans.
    List,
    /// Show cache hit rate, size, and utilization.
    Stats,
    /// Clear cached plans (optionally scoped to a table).
    Clear {
        /// Clear only entries referencing this table.
        #[arg(long)]
        table: Option<String>,
    },
    /// Reoptimize stale cached plans.
    Reoptimize {
        /// Drift threshold percentage (default 20).
        #[arg(long, default_value = "20")]
        threshold_pct: f64,
    },
    /// Show statistics drift for cached plans.
    Drift,
}

// ── Main ────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    let is_test_cmd = matches!(cli.command, Commands::Test { .. });

    // Check if optimize command without trace flag or with explain_format
    let suppress_logs = matches!(
        &cli.command,
        Commands::Optimize { trace, explain_format, .. }
        if !trace || explain_format.is_some()
    );

    let filter = if cli.quiet || suppress_logs {
        "error".to_owned()
    } else if cli.verbose && !is_test_cmd {
        "debug".to_owned()
    } else if is_test_cmd {
        "ra_cli=info,warn".to_owned()
    } else {
        "info".to_owned()
    };
    tracing_subscriber::fmt()
        .with_env_filter(&filter)
        .with_target(false)
        .without_time()
        .init();

    match cli.command {
        Commands::Validate { path } => cmd_validate(&path, cli.verbose, cli.quiet),
        Commands::Test { path, filter } => {
            cmd_test(&path, filter.as_deref(), cli.verbose, cli.quiet)
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
        Commands::Explain {
            query,
            hardware_profile,
            stdin: use_stdin,
        } => {
            let resolved = resolve_query(&query, use_stdin)?;
            cmd_explain(&resolved, &hardware_profile, cli.verbose, cli.quiet)
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
        } => {
            let resolved = resolve_query(&query, use_stdin)?;

            // Determine which rule tracking mode to use
            let show_rules = RuleDisplayMode::from_flags(
                rules_applied,
                rules_evaluated,
                rules_available,
                rules_all,
                rules,
            );

            let budget = build_resource_budget(
                resource_budget.as_deref(),
                max_time.as_deref(),
                max_memory.as_deref(),
                max_iterations,
                overflow_strategy.as_deref(),
                show_rules.should_track(),
            )?;

            cmd_optimize(
                &resolved,
                &hardware_profile,
                diff.as_deref(),
                no_color,
                budget.as_ref(),
                explain_format.as_deref(),
                stats,
                show_rules,
                cli.verbose,
                cli.quiet,
            )
        }
        Commands::GatherMetadata { db, schema, output } => cmd_gather_metadata(
            db.as_deref(),
            schema.as_deref(),
            &output,
            cli.verbose,
            cli.quiet,
        ),
        Commands::Compare {
            sql,
            db,
            explain_json,
            schema,
            hardware_profile,
        } => cmd_compare(
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
        } => cmd_tui(timeline.as_deref(), demo, headless, record.as_deref()),
        Commands::StatsTimeline(sub) => match sub {
            StatsTimelineCommands::Play {
                timeline,
                format,
                speed,
            } => {
                let fmt = stats_commands::OutputFormat::from_str_arg(&format)?;
                stats_commands::cmd_stats_play(&timeline, fmt, speed, cli.verbose)
            }
            StatsTimelineCommands::Feedback {
                timeline,
                format,
                batch_size,
            } => {
                let fmt = stats_commands::OutputFormat::from_str_arg(&format)?;
                stats_commands::cmd_stats_feedback(&timeline, fmt, batch_size, cli.verbose)
            }
            StatsTimelineCommands::Visualize { timeline, format } => {
                let fmt = stats_commands::OutputFormat::from_str_arg(&format)?;
                stats_commands::cmd_stats_visualize(&timeline, fmt, cli.verbose)
            }
        },
        Commands::Format {
            query,
            stdin,
            capitalize,
            indent,
        } => cmd_format(query.as_deref(), stdin, &capitalize, &indent, cli.quiet),
        Commands::Translate { query, from, to } => cmd_translate(&query, &from, &to, cli.quiet),
        Commands::AnalyzeTriggers {
            table,
            database_url,
            schema,
        } => cmd_analyze_triggers(
            &table,
            database_url.as_deref(),
            schema.as_deref(),
            cli.verbose,
            cli.quiet,
        ),
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
        Commands::Config(sub) => match sub {
            ConfigCommands::List => config_commands::cmd_config_list(cli.quiet),
            ConfigCommands::Get { key } => config_commands::cmd_config_get(&key),
            ConfigCommands::Set { key, value } => {
                config_commands::cmd_config_set(&key, &value, cli.quiet)
            }
            ConfigCommands::Edit => config_commands::cmd_config_edit(),
            ConfigCommands::Reset => config_commands::cmd_config_reset(cli.quiet),
            ConfigCommands::Path => config_commands::cmd_config_path(),
        },
        Commands::Cache(sub) => match sub {
            CacheCommands::List => cache_commands::cmd_cache_list(cli.verbose, cli.quiet),
            CacheCommands::Stats => cache_commands::cmd_cache_stats(cli.quiet),
            CacheCommands::Clear { table } => {
                cache_commands::cmd_cache_clear(table.as_deref(), cli.quiet)
            }
            CacheCommands::Reoptimize { threshold_pct } => {
                cache_commands::cmd_cache_reoptimize(threshold_pct, cli.quiet)
            }
            CacheCommands::Drift => cache_commands::cmd_cache_drift(cli.verbose, cli.quiet),
        },
        Commands::Monitor {
            postgres: _,
            tui,
            demo,
            format,
        } => cmd_monitor(tui, demo, &format, cli.quiet),
        Commands::Regression(sub) => match sub {
            RegressionCommands::Baseline {
                query_file,
                query_id,
                storage,
                storage_path,
                hardware_profile,
            } => regression_commands::cmd_regression_baseline(
                &query_file,
                query_id.as_deref(),
                &storage,
                &storage_path,
                &hardware_profile,
                cli.verbose,
                cli.quiet,
            ),
            RegressionCommands::Check {
                query_file,
                query_id,
                storage,
                storage_path,
                hardware_profile,
                warn_threshold,
                error_threshold,
            } => regression_commands::cmd_regression_check(
                &query_file,
                query_id.as_deref(),
                &storage,
                &storage_path,
                &hardware_profile,
                warn_threshold,
                error_threshold,
                cli.verbose,
                cli.quiet,
            ),
            RegressionCommands::Report {
                storage,
                storage_path,
                format,
                only_regressions,
            } => regression_commands::cmd_regression_report(
                &storage,
                &storage_path,
                &format,
                only_regressions,
                cli.verbose,
                cli.quiet,
            ),
        },
        Commands::Migrate(sub) => match sub {
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
                match migrate_commands::validate_preconditions(
                    baseline_path,
                    migrated_path,
                    facts_path,
                ) {
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
        },
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "ra-cli", &mut std::io::stdout());
            Ok(())
        }
    }
}

// ── validate ────────────────────────────────────────────────

fn cmd_validate(path: &str, verbose: bool, quiet: bool) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!("Validating {} file(s)", files.len()));
    }

    let mut pass = 0u32;
    let mut fail = 0u32;

    for file in &files {
        let source =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;

        match parse_rule_file(&source) {
            Ok(rule) => {
                let extra_errors = validate_metadata_all(&rule.metadata);
                if extra_errors.is_empty() {
                    pass += 1;
                    if verbose {
                        print_status("PASS", &file.display().to_string(), true);
                    }
                } else {
                    fail += 1;
                    print_status("FAIL", &file.display().to_string(), false);
                    for err in &extra_errors {
                        print_detail(&format!("  {err}"));
                    }
                }
            }
            Err(e) => {
                fail += 1;
                print_status("FAIL", &file.display().to_string(), false);
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

fn cmd_test(path: &str, filter: Option<&str>, verbose: bool, quiet: bool) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!("Running tests from {} file(s)...", files.len()));
    }

    let (results, summary) = run_tests(&files, filter, verbose)?;

    if !quiet {
        print_file_results(&summary.file_results, verbose);

        if verbose {
            print_individual_results(&results);
        }

        eprintln!();
        print_test_summary(&summary);

        if !summary.slowest.is_empty() && verbose {
            eprintln!();
            eprintln!("{}", "Slowest tests:".bold());
            for (name, dur) in &summary.slowest {
                eprintln!("  {:>6.0}ms  {}", dur.as_secs_f64() * 1000.0, name.dimmed(),);
            }
        }
    }

    if summary.failed > 0 {
        bail!("{} test(s) failed", summary.failed);
    }

    Ok(())
}

fn print_file_results(file_results: &[FileResult], verbose: bool) {
    for fr in file_results {
        if fr.passed == fr.total {
            if verbose {
                eprintln!(
                    "  {} {} ({}/{} passed)",
                    "[PASS]".green().bold(),
                    fr.display_path,
                    fr.passed,
                    fr.total,
                );
            }
        } else {
            eprintln!(
                "  {} {} ({}/{} passed)",
                "[FAIL]".red().bold(),
                fr.display_path,
                fr.passed,
                fr.total,
            );
            for (name, reason) in &fr.failures {
                eprintln!("        - {} {}", name, format!("({reason})").yellow(),);
            }
        }
    }
}

fn print_individual_results(results: &[TestResult]) {
    eprintln!();
    eprintln!("{}", "Individual results:".bold());
    for result in results {
        match &result.outcome {
            TestOutcome::Pass => {
                eprintln!(
                    "  {} {} ({}ms)",
                    "[PASS]".green().bold(),
                    result.name,
                    result.duration.as_millis(),
                );
            }
            TestOutcome::Fail { reason } => {
                eprintln!("  {} {}", "[FAIL]".red().bold(), result.name,);
                eprintln!("        {}", reason.yellow());
            }
            TestOutcome::Skip { reason } => {
                eprintln!(
                    "  {} {} ({})",
                    "[SKIP]".dimmed().bold(),
                    result.name,
                    reason.dimmed(),
                );
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
}

fn print_test_summary(summary: &test_executor::TestSummary) {
    let pass_rate = if summary.total > 0 {
        #[allow(clippy::cast_precision_loss)]
        let rate = summary.passed as f64 / summary.total as f64 * 100.0;
        format!("{rate:.1}%")
    } else {
        "N/A".to_owned()
    };

    let status_line = format!(
        "Summary: {}/{} passed ({pass_rate})",
        summary.passed, summary.total,
    );

    if summary.failed == 0 && summary.errored == 0 {
        eprintln!("{}", status_line.green().bold());
    } else {
        eprintln!("{}", status_line.bold());
    }

    if summary.failed > 0 {
        eprintln!("  {}: {} tests", "Failed".red().bold(), summary.failed,);
    }
    if summary.skipped > 0 {
        eprintln!("  {}: {} tests", "Skipped".dimmed(), summary.skipped,);
    }
    if summary.errored > 0 {
        eprintln!("  {}: {} tests", "Errors".red(), summary.errored,);
    }
    eprintln!(
        "  {}: {:.1}s",
        "Duration".dimmed(),
        summary.duration.as_secs_f64(),
    );
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

    let mut entries: Vec<(String, String, String, PathBuf)> = Vec::new();

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
            if !meta.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                continue;
            }
        }

        entries.push((meta.id, meta.name, meta.category, file.clone()));
    }

    entries.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));

    if !quiet {
        let mut header = format!("{} rule(s) found", entries.len());
        if let Some(cat) = category_filter {
            header.push_str(&format!(" in category '{cat}'"));
        }
        if let Some(tag) = tag_filter {
            header.push_str(&format!(" with tag '{tag}'"));
        }
        print_header(&header);
    }

    let id_w = entries.iter().map(|e| e.0.len()).max().unwrap_or(2).max(2);
    let name_w = entries.iter().map(|e| e.1.len()).max().unwrap_or(4).max(4);
    let cat_w = entries.iter().map(|e| e.2.len()).max().unwrap_or(8).max(8);

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

fn cmd_stats(dir: &str, verbose: bool, quiet: bool) -> Result<()> {
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

    let mut by_category: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut by_id: std::collections::HashMap<String, Vec<PathBuf>> =
        std::collections::HashMap::new();
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
                let errs = validate_metadata_all(&rule.metadata);
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
    let duplicates: Vec<_> = by_id.iter().filter(|(_, v)| v.len() > 1).collect();

    if !quiet {
        print_header(&format!("Rule Collection Statistics ({total} files)"));

        eprintln!("  {}: {}", "Total .rra files".bold(), total);
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
        eprintln!("  {}: {}", "Unique rule IDs".bold(), by_id.len());
        eprintln!("  {}: {}", "Duplicate IDs".bold(), duplicates.len());
        eprintln!("  {}: {}", "Categories".bold(), by_category.len());

        eprintln!();
        eprintln!("{}", "Rules by Category:".bold());
        for (cat, rules) in &by_category {
            eprintln!("  {:>4}  {}", rules.len().to_string().cyan(), cat,);
        }

        if !duplicates.is_empty() {
            eprintln!();
            eprintln!("{}", "Duplicate Rule IDs:".bold());
            for (id, paths) in &duplicates {
                eprintln!("  {} ({}x):", id.yellow(), paths.len());
                if verbose {
                    for p in *paths {
                        eprintln!("    - {}", p.display());
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

    let Some((rule, path)) = find_rule_by_id(rule_id, &files) else {
        bail!(
            "rule '{rule_id}' not found in {dir}\n\
             hint: run 'ra-cli list' to see available rules"
        );
    };

    eprintln!("{}", format!("Rule: {}", rule.metadata.id).bold());
    eprintln!("  {}: {}", "Name".bold(), rule.metadata.name);
    eprintln!("  {}: {}", "Category".bold(), rule.metadata.category);
    eprintln!("  {}: {}", "Version".bold(), rule.metadata.version);
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
        eprintln!("  {}: {}", "Tags".bold(), rule.metadata.tags.join(", "));
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
            format!("Test Cases: {} block(s)", rule.test_cases.len()).bold()
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
    let plan = sql_to_relexpr(query).with_context(|| format!("failed to parse SQL: {query}"))?;

    let hardware = load_hardware_profile(hardware_profile_name)?;

    if !quiet {
        print_header("Query Plan Explanation");
        eprintln!("  {}: {query}", "SQL".bold());

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

    Ok(())
}

// ── optimize ────────────────────────────────────────────────

fn cmd_optimize(
    query: &str,
    hardware_profile_name: &str,
    diff_format: Option<&str>,
    no_color: bool,
    budget: Option<&ra_engine::ResourceBudget>,
    explain_format: Option<&str>,
    show_stats: bool,
    show_rules: RuleDisplayMode,
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

    let plan = sql_to_relexpr(query).with_context(|| format!("failed to parse SQL: {query}"))?;
    let hardware = load_hardware_profile(hardware_profile_name)?;

    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(hardware.clone());

    if let Some(b) = budget {
        optimizer.set_resource_budget(b.clone());
    }

    if budget.is_some() {
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
    }
}

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
        // Check if the budget is unlimited - if so, use simpler title
        let budget = optimizer.config().resource_budget.as_ref();
        let title = if budget.map_or(false, |b| b.is_unlimited()) {
            "Query Optimization"
        } else {
            "Query Optimization (Resource-Bounded)"
        };
        print_optimization_header(
            title,
            query,
            hardware,
            verbose,
        );
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

fn print_optimization_header(
    title: &str,
    query: &str,
    hardware: &ra_hardware::HardwareProfile,
    verbose: bool,
) {
    print_header(title);

    // Format SQL query nicely
    let formatted_query = match ra_parser::formatter::SqlFormatter::default_style().format(query) {
        Ok(formatted) => {
            // Indent each line of the formatted query
            formatted
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Err(_) => {
            // If formatting fails, fall back to the original query
            format!("    {query}")
        }
    };

    eprintln!("  {}:", "SQL".bold());
    eprintln!("{formatted_query}");

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
        let diff_output = plan_diff::render_diff(original, optimized, fmt);
        eprintln!("{diff_output}");
    } else if original == optimized {
        // Plans are identical - show only once
        eprintln!("{}", "Original Plan Unchanged After Optimization:".bold());
        eprintln!("{}", format_plan_tree(original));
    } else {
        // Plans differ - show both
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
        "side-by-side" | "sbs" => Ok(plan_diff::DiffFormat::SideBySide),
        "compact" | "summary" => Ok(plan_diff::DiffFormat::Compact),
        _ => bail!(
            "unknown diff format: '{s}'. \
             Valid options: colored, plain, side-by-side, compact"
        ),
    }
}

/// EXPLAIN output format options.
enum ExplainOutputFormat {
    /// Use ASCII tree format (ra-cli's default).
    Ascii,
    /// Use database-specific text formatter from explain.rs.
    DatabaseText {
        database: DatabaseTextFormat,
        cost_params: ra_metadata::DatabaseCostParams,
    },
}

/// Database-specific text format options.
enum DatabaseTextFormat {
    Postgres,
    Mysql,
    Sqlite,
}

/// Parse an EXPLAIN format string.
fn parse_explain_format(s: &str) -> Result<ExplainOutputFormat> {
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
fn print_explain_output(plan: &ra_core::algebra::RelExpr, format_str: &str) -> Result<()> {
    let format = parse_explain_format(format_str)?;

    let output = match format {
        ExplainOutputFormat::Ascii => format_plan_tree(plan),
        ExplainOutputFormat::DatabaseText {
            database,
            cost_params,
        } => {
            let explain_plan = ra_metadata::from_relexpr(plan, &cost_params);
            match database {
                DatabaseTextFormat::Postgres => {
                    ra_metadata::format_postgres_explain(&explain_plan.root)
                }
                DatabaseTextFormat::Mysql => ra_metadata::format_mysql_explain(&explain_plan.root),
                DatabaseTextFormat::Sqlite => {
                    ra_metadata::format_sqlite_explain(&explain_plan.root)
                }
            }
        }
    };

    eprintln!("{output}");
    Ok(())
}

// ── gather-metadata ────────────────────────────────────────

fn cmd_gather_metadata(
    db_url: Option<&str>,
    schema_path: Option<&str>,
    output_path: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let schema = if let Some(url) = db_url {
        if !quiet {
            let kind = ra_metadata::detect_kind(url)
                .map_or_else(|_| "unknown".to_owned(), |k| k.to_string());
            eprintln!("Connecting to {} database...", kind.cyan());
        }
        let mut connector =
            ra_metadata::connect(url).with_context(|| format!("connecting to database: {url}"))?;
        connector
            .gather_schema()
            .with_context(|| format!("gathering schema from: {url}"))?
    } else if let Some(path) = schema_path {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("reading schema file: {path}"))?;
        serde_json::from_str(&source)
            .with_context(|| format!("parsing schema JSON from: {path}"))?
    } else {
        bail!("either --db <url> or --schema <path> is required");
    };

    if !quiet {
        print_header("Database Metadata");
        eprintln!("  {}: {}", "Database".bold(), schema.kind);
        eprintln!("  {}: {}", "Schema".bold(), schema.schema_name);
        eprintln!("  {}: {}", "Tables".bold(), schema.table_count());
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

    let json = serde_json::to_string_pretty(&schema).context("serializing schema to JSON")?;
    std::fs::write(output_path, json)
        .with_context(|| format!("writing output to: {output_path}"))?;

    if !quiet {
        eprintln!();
        eprintln!(
            "{}",
            format!("Wrote metadata to {output_path}").green().bold()
        );
    }

    Ok(())
}

// ── compare ────────────────────────────────────────────────

fn cmd_compare(
    sql: &str,
    db_url: Option<&str>,
    explain_json_path: Option<&str>,
    _schema_path: Option<&str>,
    hardware_profile_name: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let ra_plan = sql_to_relexpr(sql).with_context(|| format!("failed to parse SQL: {sql}"))?;

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

// ── tui ─────────────────────────────────────────────────────

fn cmd_tui(
    timeline_path: Option<&str>,
    demo: bool,
    headless: bool,
    record_path: Option<&str>,
) -> Result<()> {
    let timeline = if demo {
        ra_tui::Timeline::demo()
    } else if let Some(path) = timeline_path {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("reading timeline file: {path}"))?;

        if path.ends_with(".json") {
            serde_json::from_str(&source)
                .with_context(|| format!("parsing timeline JSON from: {path}"))?
        } else if path.ends_with(".toml") {
            ra_tui::Timeline::from_toml(&source)
                .map_err(|e| anyhow::anyhow!("{e}"))
                .with_context(|| format!("converting TOML timeline: {path}"))?
        } else {
            serde_json::from_str(&source)
                .with_context(|| format!("parsing timeline from: {path}"))?
        }
    } else {
        bail!(
            "specify --demo for demo data or \
             --timeline <path> to load a timeline file"
        );
    };

    let mut app = ra_tui::App::new(timeline).context("initializing TUI")?;

    if let Some(output) = record_path {
        let path = std::path::Path::new(output);
        let frame_count = ra_tui::record_session(&mut app, path, 120, 40, 1.0)
            .context("recording TUI session")?;
        eprintln!("Recorded {frame_count} frames to {output}");
        return Ok(());
    }

    if headless {
        let final_cost = app.run_headless().context("running headless TUI")?;
        eprintln!("Headless run complete. Final cost: {final_cost:.0}");
        return Ok(());
    }

    app.run().context("running TUI")?;

    Ok(())
}

// ── format ───────────────────────────────────────────────────

fn cmd_format(
    query: Option<&str>,
    stdin: bool,
    capitalize: &str,
    indent: &str,
    quiet: bool,
) -> Result<()> {
    let sql = if stdin || query.is_none() {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading SQL from stdin")?;
        buf
    } else {
        query.unwrap_or_default().to_owned()
    };

    let cap_mode = match capitalize.to_lowercase().as_str() {
        "keywords" | "kw" => ra_parser::CapitalizeMode::Keywords,
        "all" => ra_parser::CapitalizeMode::All,
        "none" => ra_parser::CapitalizeMode::None,
        other => bail!(
            "unknown capitalize mode: '{other}'. \
             Valid: keywords, all, none"
        ),
    };

    let indent_style = match indent.to_lowercase().as_str() {
        "spaces2" | "2" => ra_parser::IndentStyle::Spaces(2),
        "spaces4" | "4" => ra_parser::IndentStyle::Spaces(4),
        "tab" | "tabs" => ra_parser::IndentStyle::Tab,
        other => bail!(
            "unknown indent style: '{other}'. \
             Valid: spaces2, spaces4, tab"
        ),
    };

    let config = ra_parser::FormatConfig {
        capitalize: cap_mode,
        indent: indent_style,
        ..ra_parser::FormatConfig::default()
    };

    let formatter = ra_parser::SqlFormatter::new(config);
    let formatted = formatter
        .format(&sql)
        .with_context(|| format!("formatting SQL: {sql}"))?;

    if !quiet {
        eprintln!("{formatted}");
    }

    Ok(())
}

// ── translate ────────────────────────────────────────────────

fn cmd_translate(query: &str, from: &str, to: &str, quiet: bool) -> Result<()> {
    let source_dialect = parse_dialect(from)?;
    let target_dialect = parse_dialect(to)?;

    let translator = ra_dialect::DialectTranslator::new(source_dialect, target_dialect);

    let result = translator
        .translate(query)
        .with_context(|| format!("translating SQL from {from} to {to}: {query}"))?;

    if !quiet {
        print_header(&format!(
            "SQL Translation: {} -> {}",
            source_dialect, target_dialect
        ));
        eprintln!("  {}: {query}", "Input".bold());
        eprintln!();
        eprintln!("{}", "Translated:".bold());
        eprintln!("  {}", result.sql);

        if !result.warnings.is_empty() {
            eprintln!();
            eprintln!("{}", "Warnings:".bold());
            for w in &result.warnings {
                eprintln!("  {} {}", format!("[{}]", w.severity).yellow(), w.message);
                if let Some(ref hint) = w.hint {
                    eprintln!("    {}: {hint}", "hint".dimmed());
                }
            }
        }
    }

    Ok(())
}

// ── analyze-triggers ────────────────────────────────────────

fn cmd_analyze_triggers(
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

    print_dml_cost("INSERT", &analysis.insert_cost, verbose);
    print_dml_cost("UPDATE", &analysis.update_cost, verbose);
    print_dml_cost("DELETE", &analysis.delete_cost, verbose);

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

fn load_schema_for_analysis(
    database_url: Option<&str>,
    schema_path: Option<&str>,
) -> Result<ra_metadata::SchemaInfo> {
    if let Some(url) = database_url {
        let mut connector =
            ra_metadata::connect(url).with_context(|| format!("connecting to database: {url}"))?;
        let schema = connector
            .gather_schema()
            .with_context(|| "gathering schema metadata from database")?;
        return Ok(schema);
    }

    if let Some(path) = schema_path {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading schema file: {path}"))?;
        let schema: ra_metadata::SchemaInfo = serde_json::from_str(&contents)
            .with_context(|| format!("parsing schema JSON: {path}"))?;
        return Ok(schema);
    }

    bail!(
        "must provide either --database-url or --schema \
         for trigger analysis"
    );
}

fn print_dml_cost(
    event: &str,
    cost: &Option<ra_engine::trigger_optimizer::DmlCostEstimate>,
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

/// Parse a dialect name string into a `Dialect` enum.
fn parse_dialect(name: &str) -> Result<ra_dialect::Dialect> {
    match name.to_lowercase().as_str() {
        "postgresql" | "postgres" | "pg" => Ok(ra_dialect::Dialect::PostgreSql),
        "mysql" => Ok(ra_dialect::Dialect::MySql),
        "sqlite" => Ok(ra_dialect::Dialect::Sqlite),
        "duckdb" => Ok(ra_dialect::Dialect::DuckDb),
        "mssql" | "mssqlserver" | "sqlserver" => Ok(ra_dialect::Dialect::MsSql),
        "oracle" => Ok(ra_dialect::Dialect::Oracle),
        other => bail!(
            "unknown dialect: '{other}'. Valid: postgresql, \
             mysql, sqlite, duckdb, mssql, oracle"
        ),
    }
}

// ── Helpers ─────────────────────────────────────────────────

/// Resolve the SQL query from either the positional argument or stdin.
fn resolve_query(positional: &str, use_stdin: bool) -> Result<String> {
    if use_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading SQL from stdin")?;
        let trimmed = buf.trim().to_owned();
        if trimmed.is_empty() {
            bail!(
                "no SQL received on stdin\n\
                 hint: pipe a query, e.g. \
                 echo \"SELECT 1\" | ra-cli explain --stdin"
            );
        }
        Ok(trimmed)
    } else {
        if positional.is_empty() {
            bail!(
                "no SQL query provided\n\
                 hint: pass a query argument or use --stdin"
            );
        }
        Ok(positional.to_owned())
    }
}

/// Load a hardware profile by name.
fn load_hardware_profile(name: &str) -> Result<ra_hardware::HardwareProfile> {
    let profile = match name.to_lowercase().as_str() {
        "auto" => ra_hardware::detect_hardware(),
        "cpu-only" => ra_hardware::HardwareProfile::cpu_only(),
        "gpu-server" => ra_hardware::HardwareProfile::gpu_server(),
        "fpga" => ra_hardware::HardwareProfile::fpga_appliance(),
        _ => bail!(
            "unknown hardware profile: {name}. Valid options: auto, cpu-only, gpu-server, fpga"
        ),
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
fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rra") {
            out.push(path);
        }
    }
    Ok(())
}

/// Search for a rule by ID across a set of files.
fn find_rule_by_id(rule_id: &str, files: &[PathBuf]) -> Option<(RuleFile, PathBuf)> {
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
    rule_tracking_requested: bool,
) -> Result<Option<ra_engine::ResourceBudget>> {
    let has_custom = max_time.is_some()
        || max_memory.is_some()
        || max_iterations.is_some()
        || overflow_strategy.is_some();

    // Default behavior: unbounded unless rule tracking is requested
    if profile.is_none() && !has_custom {
        if rule_tracking_requested {
            // Rule tracking requires a budget to be set, default to standard
            return Ok(Some(ra_engine::ResourceBudget::standard()));
        }
        // No profile, no custom settings, no rule tracking = unbounded
        return Ok(None);
    }

    // If profile is explicitly set, use it; otherwise default to standard
    // when custom settings are provided or rule tracking is requested
    let mut budget = match profile {
        Some("interactive") => ra_engine::ResourceBudget::interactive(),
        Some("standard") => ra_engine::ResourceBudget::standard(),
        Some("batch") => ra_engine::ResourceBudget::batch(),
        Some("memory-constrained") => ra_engine::ResourceBudget::memory_constrained(),
        Some("unlimited") => ra_engine::ResourceBudget::unlimited(),
        Some(other) => bail!(
            "unknown resource budget profile: '{other}'. \
             Valid: interactive, standard, batch, \
             memory-constrained, unlimited"
        ),
        None if rule_tracking_requested => {
            // Rule tracking with custom settings still needs a base budget
            ra_engine::ResourceBudget::standard()
        }
        None => {
            // Custom settings without rule tracking = start unbounded
            ra_engine::ResourceBudget::unlimited()
        }
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
        let n: u64 = ms.trim().parse().context("invalid millisecond value")?;
        return Ok(std::time::Duration::from_millis(n));
    }
    if let Some(secs) = s.strip_suffix('s') {
        let n: u64 = secs.trim().parse().context("invalid seconds value")?;
        return Ok(std::time::Duration::from_secs(n));
    }
    // Default to seconds
    let n: u64 = s
        .parse()
        .context("invalid duration; use e.g. '100ms' or '1s'")?;
    Ok(std::time::Duration::from_secs(n))
}

/// Parse a human-readable byte size (e.g. "10MB", "500MB", "2GB").
fn parse_byte_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let upper = s.to_uppercase();
    if let Some(gb) = upper.strip_suffix("GB") {
        let n: u64 = gb.trim().parse().context("invalid GB value")?;
        return Ok(n.saturating_mul(1024 * 1024 * 1024));
    }
    if let Some(mb) = upper.strip_suffix("MB") {
        let n: u64 = mb.trim().parse().context("invalid MB value")?;
        return Ok(n.saturating_mul(1024 * 1024));
    }
    if let Some(kb) = upper.strip_suffix("KB") {
        let n: u64 = kb.trim().parse().context("invalid KB value")?;
        return Ok(n.saturating_mul(1024));
    }
    s.parse::<u64>()
        .context("invalid byte size; use e.g. '10MB', '2GB', or raw bytes")
}

/// Parse an overflow strategy string.
fn parse_overflow(s: &str) -> Result<ra_engine::OverflowStrategy> {
    match s.to_lowercase().as_str() {
        "best-so-far" | "best" => Ok(ra_engine::OverflowStrategy::ReturnBestSoFar),
        "original" => Ok(ra_engine::OverflowStrategy::ReturnOriginal),
        "fail" => Ok(ra_engine::OverflowStrategy::Fail),
        _ => bail!(
            "unknown overflow strategy: '{s}'. \
             Valid: best-so-far, original, fail"
        ),
    }
}

/// Display resource usage from a bounded optimization result.
fn print_resource_usage(result: &ra_engine::OptimizationResult, verbose: bool) {
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
    eprintln!("  {}: {}", "Iterations".bold(), usage.iterations_used,);
    eprintln!(
        "  {}: {}",
        "Peak e-graph nodes".bold(),
        usage.peak_egraph_nodes,
    );

    if verbose {
        #[allow(clippy::cast_precision_loss)]
        let mem_mb = usage.peak_memory_estimate as f64 / (1024.0 * 1024.0);
        eprintln!("  {}: {mem_mb:.2} MB", "Peak memory (est.)".bold(),);
        eprintln!("  {}: {:.2}", "Plan cost".bold(), result.cost,);
    }
}

fn print_optimization_stats(usage: &ra_engine::ResourceUsageReport) {
    eprintln!("{}", "Optimization Statistics:".bold());
    eprintln!(
        "  {}: {:.1}ms",
        "Planning time".bold(),
        usage.elapsed_time.as_secs_f64() * 1000.0,
    );
    eprintln!("  {}: {}", "Iterations used".bold(), usage.iterations_used,);
    eprintln!(
        "  {}: {}",
        "Peak e-graph nodes".bold(),
        usage.peak_egraph_nodes,
    );
    #[allow(clippy::cast_precision_loss)]
    let mem_mb = usage.peak_memory_estimate as f64 / (1024.0 * 1024.0);
    eprintln!("  {}: {mem_mb:.2} MB", "Peak memory".bold(),);
    if let Some(ref exceeded) = usage.budget_exceeded {
        eprintln!("  {}: {exceeded}", "Budget exceeded".bold().yellow(),);
    }
}

fn print_unbounded_stats(elapsed: std::time::Duration) {
    eprintln!("{}", "Optimization Statistics:".bold());
    eprintln!(
        "  {}: {:.1}ms",
        "Planning time".bold(),
        elapsed.as_secs_f64() * 1000.0,
    );
}

fn print_rule_tracking(result: &ra_engine::OptimizationResult, mode: RuleDisplayMode) {
    use colored::Colorize;

    let Some(tracking) = &result.rule_tracking else {
        eprintln!("{}", "Rule tracking not available".yellow());
        eprintln!("This should not happen - tracking was requested but not populated");
        return;
    };

    match mode {
        RuleDisplayMode::None => {}
        RuleDisplayMode::Applied => {
            print_applied_rules(tracking);
        }
        RuleDisplayMode::Evaluated => {
            print_evaluated_rules(tracking);
        }
        RuleDisplayMode::Available => {
            print_available_rules(tracking);
        }
        RuleDisplayMode::All => {
            print_applied_rules(tracking);
            eprintln!();
            print_evaluated_rules(tracking);
            eprintln!();
            print_available_rules(tracking);
        }
    }
}

fn print_intermediate_steps(
    tracking: &ra_engine::RuleTrackingResult,
    original_plan: &ra_core::algebra::RelExpr,
) {
    use colored::Colorize;

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

    for step in steps {
        eprintln!(
            "{}",
            format!("Step {}: Applied {}", step.step_number, step.rule_name)
                .bold()
                .green()
        );

        // Enhanced "Why" section with rule-specific reasoning
        let why_text = enhance_reasoning(&step.rule_name, &step.reason, &step.plan_before, &step.plan_after);
        eprintln!("  {}: {}", "Why".bold(), why_text);

        // Enhanced "Impact" section
        if let Some(improvement) = step.cost_improvement {
            eprintln!("  {}: {}", "Impact".bold().cyan(), format_impact(improvement, &step.plan_before, &step.plan_after));
        }

        eprintln!();
        eprintln!("  {}:", "Changes".bold());

        // Display plan tree with changes highlighted
        print_plan_with_changes(&step.plan_after, &step.plan_before);

        eprintln!();
    }

    eprintln!("{}", "Final Optimized Plan:".bold());
    if let Some(last_step) = steps.last() {
        eprintln!("{}", format_plan_tree(&last_step.plan_after));
    }
}

/// Enhance the reasoning explanation based on rule name and context.
fn enhance_reasoning(
    rule_name: &str,
    original_reason: &str,
    plan_before: &ra_core::algebra::RelExpr,
    plan_after: &ra_core::algebra::RelExpr,
) -> String {
    use colored::Colorize;

    // If original reason already has good detail, use it
    if !original_reason.contains("Pattern matched") && original_reason.len() > 30 {
        return original_reason.to_string();
    }

    // Generate rule-specific explanations
    let explanation = match rule_name {
        name if name.contains("filter-pushdown") || name.contains("push-filter") => {
            "Filter condition can be evaluated earlier to reduce data processed by downstream operators"
        }
        name if name.contains("join-order") || name.contains("reorder") => {
            "Join order optimized to process smaller result sets first, reducing intermediate data"
        }
        name if name.contains("index") => {
            "Index scan available for predicate, eliminating full table scan"
        }
        name if name.contains("semi-join") || name.contains("semijoin") => {
            "Converted to semi-join since only existence check is needed, not full join results"
        }
        name if name.contains("projection") || name.contains("project-pushdown") => {
            "Project columns earlier to reduce data width and memory usage"
        }
        name if name.contains("aggregate") && name.contains("push") => {
            "Aggregate pushed down to reduce data volume before subsequent operations"
        }
        name if name.contains("eliminate") => {
            "Removed redundant operator that doesn't affect query results"
        }
        name if name.contains("merge") => {
            "Combined adjacent operators to reduce overhead"
        }
        name if name.contains("parallel") => {
            "Parallelized operation to utilize multiple CPU cores"
        }
        name if name.contains("bitmap") => {
            "Using bitmap index to efficiently combine multiple index scans"
        }
        _ => {
            // Analyze plan structure for generic explanation
            if has_filter_pushdown(plan_before, plan_after) {
                "Moved filter closer to data source to reduce processing"
            } else if has_join_reorder(plan_before, plan_after) {
                "Reordered operations for better performance"
            } else {
                "Applied rewrite rule to improve query execution"
            }
        }
    };

    format!("{} [{}]", explanation, rule_name.dimmed())
}

/// Format the impact of an optimization with context.
fn format_impact(
    cost_improvement: f64,
    plan_before: &ra_core::algebra::RelExpr,
    plan_after: &ra_core::algebra::RelExpr,
) -> String {
    use colored::Colorize;

    let mut impacts = Vec::new();

    // Show cost reduction percentage if significant
    impacts.push(format!(
        "Reduced estimated cost by {:.2}",
        cost_improvement.to_string().green()
    ));

    // Detect specific optimizations
    if has_scan_upgrade(plan_before, plan_after) {
        impacts.push("Eliminated full table scan, using index instead".to_string());
    }

    if has_operator_elimination(plan_before, plan_after) {
        let diff = count_operators(plan_before) - count_operators(plan_after);
        if diff > 0 {
            impacts.push(format!("Removed {} redundant operator(s)", diff));
        }
    }

    if has_parallelization(plan_after) {
        impacts.push("Enabled parallel execution".to_string());
    }

    impacts.join("; ")
}

/// Extract just the operator part from a plan tree line (removing tree characters).
fn extract_operator(line: &str) -> String {
    line.trim_start_matches(|c: char| c.is_whitespace() || "└├─│".contains(c))
        .trim()
        .to_string()
}

/// Print plan as a tree with highlighted changes.
fn print_plan_with_changes(
    plan: &ra_core::algebra::RelExpr,
    before: &ra_core::algebra::RelExpr,
) {
    use colored::Colorize;

    // Get tree representations
    let before_tree = format_plan_tree(before);
    let after_tree = format_plan_tree(plan);

    // Split into lines preserving tree structure
    let before_lines: Vec<&str> = before_tree.lines().collect();
    let after_lines: Vec<&str> = after_tree.lines().collect();

    // Extract just the operator parts (without tree characters) for comparison
    let before_ops: Vec<String> = before_lines.iter().map(|l| extract_operator(l)).collect();
    let after_ops: Vec<String> = after_lines.iter().map(|l| extract_operator(l)).collect();

    // Print the after tree with changes highlighted
    for (i, line) in after_lines.iter().enumerate() {
        let op = &after_ops[i];

        // Check if this operator existed in the before plan
        if before_ops.contains(op) {
            // Unchanged - show dimmed
            eprintln!("    {}", line.dimmed());
        } else {
            // New or changed - show bold and green
            eprintln!("    {}", line.green().bold());
        }
    }

    // Show removed operators (that were in before but not in after)
    let mut shown_removed = false;
    for (i, op) in before_ops.iter().enumerate() {
        if !after_ops.contains(op) && !op.is_empty() {
            if !shown_removed {
                eprintln!("    {}", "Removed:".red().bold());
                shown_removed = true;
            }
            eprintln!("      {} {}", "−".red().bold(), before_lines[i].trim().red().strikethrough());
        }
    }
}

/// Detect if a filter was pushed down in the plan.
fn has_filter_pushdown(before: &ra_core::algebra::RelExpr, after: &ra_core::algebra::RelExpr) -> bool {
    // Simple heuristic: if before has filter higher in tree and after has it lower
    let before_depth = filter_depth(before, 0);
    let after_depth = filter_depth(after, 0);
    after_depth > before_depth
}

/// Get the depth of the first filter in the plan tree.
fn filter_depth(expr: &ra_core::algebra::RelExpr, depth: usize) -> usize {
    match expr {
        ra_core::algebra::RelExpr::Filter { .. } => depth,
        _ => {
            expr.children()
                .iter()
                .map(|child| filter_depth(child, depth + 1))
                .min()
                .unwrap_or(usize::MAX)
        }
    }
}

/// Detect if joins were reordered.
fn has_join_reorder(before: &ra_core::algebra::RelExpr, after: &ra_core::algebra::RelExpr) -> bool {
    let before_joins = count_joins(before);
    let after_joins = count_joins(after);
    // Same number of joins but structure changed
    before_joins == after_joins && before_joins > 0 && before != after
}

/// Count join operators in a plan.
fn count_joins(expr: &ra_core::algebra::RelExpr) -> usize {
    match expr {
        ra_core::algebra::RelExpr::Join { .. } => {
            1 + expr.children().iter().map(|c| count_joins(c)).sum::<usize>()
        }
        ra_core::algebra::RelExpr::ParallelHashJoin { .. } => {
            1 + expr.children().iter().map(|c| count_joins(c)).sum::<usize>()
        }
        _ => expr.children().iter().map(|c| count_joins(c)).sum(),
    }
}

/// Detect if scan was upgraded (e.g., table scan to index scan).
fn has_scan_upgrade(before: &ra_core::algebra::RelExpr, after: &ra_core::algebra::RelExpr) -> bool {
    has_table_scan(before) && has_index_scan(after)
}

/// Check if plan has a table scan.
fn has_table_scan(expr: &ra_core::algebra::RelExpr) -> bool {
    match expr {
        ra_core::algebra::RelExpr::Scan { .. } => true,
        _ => expr.children().iter().any(|&child| has_table_scan(child)),
    }
}

/// Check if plan has an index scan.
fn has_index_scan(expr: &ra_core::algebra::RelExpr) -> bool {
    match expr {
        ra_core::algebra::RelExpr::IndexScan { .. }
        | ra_core::algebra::RelExpr::IndexOnlyScan { .. }
        | ra_core::algebra::RelExpr::BitmapIndexScan { .. } => true,
        _ => expr.children().iter().any(|&child| has_index_scan(child)),
    }
}

/// Count total operators in a plan.
fn count_operators(expr: &ra_core::algebra::RelExpr) -> usize {
    1 + expr.children().iter().map(|c| count_operators(c)).sum::<usize>()
}

/// Detect if an operator was eliminated.
fn has_operator_elimination(before: &ra_core::algebra::RelExpr, after: &ra_core::algebra::RelExpr) -> bool {
    count_operators(before) > count_operators(after)
}

/// Check if plan uses parallelization.
fn has_parallelization(expr: &ra_core::algebra::RelExpr) -> bool {
    match expr {
        ra_core::algebra::RelExpr::ParallelScan { .. }
        | ra_core::algebra::RelExpr::ParallelHashJoin { .. }
        | ra_core::algebra::RelExpr::ParallelAggregate { .. }
        | ra_core::algebra::RelExpr::Gather { .. } => true,
        _ => expr.children().iter().any(|&child| has_parallelization(child)),
    }
}

fn print_applied_rules(tracking: &ra_engine::RuleTrackingResult) {
    use colored::Colorize;

    eprintln!("{}", "Rules Applied:".bold());
    if tracking.applied.is_empty() {
        eprintln!("  {}", "No rules modified the e-graph".dimmed());
        return;
    }

    for (i, rule) in tracking.applied.iter().enumerate() {
        let cost_info = if let Some(improvement) = rule.cost_improvement {
            format!(" (cost improvement: {:.2})", improvement)
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
    use colored::Colorize;

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
    use colored::Colorize;

    eprintln!(
        "{}: {} total",
        "Available Rules".bold(),
        tracking.available.len()
    );
    eprintln!("  Use --rules-applied to see which rules modified the plan");
}

// ── Output formatting ───────────────────────────────────────

fn print_header(msg: &str) {
    eprintln!();
    eprintln!("{}", msg.bold());
    eprintln!();
}

fn print_status(label: &str, detail: &str, ok: bool) {
    if ok {
        eprintln!("  {} {detail}", format!("[{label}]").green().bold(),);
    } else {
        eprintln!("  {} {detail}", format!("[{label}]").red().bold(),);
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
            print_detail(&format!("{}:{line}: {source}", path.display()));
        }
        ParseError::Validation(v) => {
            print_detail(&format!("{}: {v}", path.display()));
        }
        ParseError::Other(msg) => {
            print_detail(&format!("{}: {msg}", path.display()));
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

// ── monitor ────────────────────────────────────────────────

fn cmd_monitor(tui: bool, demo: bool, format: &str, quiet: bool) -> Result<()> {
    use ra_pg_monitor::{
        Advisor, BloatDetector, ConfigChecker, MonitorApp, QueryMonitor, SchemaAnalyzer,
        StalenessChecker,
    };

    let mut query_monitor = QueryMonitor::new(100.0);
    let mut schema_analyzer = SchemaAnalyzer::new();
    let mut config_checker = ConfigChecker::new();
    let mut bloat_detector = BloatDetector::new();
    let mut staleness_checker = StalenessChecker::new();

    if demo {
        load_demo_data(
            &mut query_monitor,
            &mut schema_analyzer,
            &mut config_checker,
            &mut bloat_detector,
            &mut staleness_checker,
        );
    }

    let advisor = Advisor::new(
        query_monitor,
        schema_analyzer,
        config_checker,
        bloat_detector,
        staleness_checker,
    );

    if tui {
        let mut app = MonitorApp::new(advisor);
        app.run().context("TUI monitor failed")?;
        return Ok(());
    }

    let recs = advisor.all_recommendations();
    if recs.is_empty() {
        if !quiet {
            eprintln!("{}", "No issues found.".green().bold());
        }
        return Ok(());
    }

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&recs)
                .context("failed to serialize recommendations")?;
            eprintln!("{json}");
        }
        _ => {
            for rec in &recs {
                let severity_colored = match rec.severity {
                    ra_pg_monitor::Severity::Info => "INFO".cyan(),
                    ra_pg_monitor::Severity::Warning => "WARN".yellow(),
                    ra_pg_monitor::Severity::Error => "ERROR".red(),
                    ra_pg_monitor::Severity::Critical => "CRIT".red().bold(),
                };
                eprintln!(
                    "[{}] {} {}: {}",
                    severity_colored,
                    rec.category,
                    rec.target.bold(),
                    rec.message,
                );
                eprintln!("      {} {}", "Fix:".dimmed(), rec.suggestion,);
            }
            eprintln!();
            eprintln!("{}: {} recommendation(s)", "Total".bold(), recs.len(),);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn load_demo_data(
    query_monitor: &mut ra_pg_monitor::QueryMonitor,
    schema_analyzer: &mut ra_pg_monitor::SchemaAnalyzer,
    config_checker: &mut ra_pg_monitor::ConfigChecker,
    bloat_detector: &mut ra_pg_monitor::BloatDetector,
    staleness_checker: &mut ra_pg_monitor::StalenessChecker,
) {
    use ra_pg_monitor::bloat_detector::TableBloatInput;
    use ra_pg_monitor::config_checker::PgConfig;
    use ra_pg_monitor::query_monitor::{PlanNode, PlanNodeType, QueryRecord, QuerySeverity};
    use ra_pg_monitor::schema_analyzer::{
        ColumnTypeInfo, ForeignKeyInfo, IndexUsage, TableSchemaInfo,
    };
    use ra_pg_monitor::stats_staleness::TableStatsInput;

    // Demo queries
    query_monitor.record(QueryRecord {
        query: "SELECT * FROM orders WHERE status = 'pending'".to_string(),
        duration_ms: 2300.0,
        total_cost: 45000.0,
        root_plan: PlanNodeType::SeqScan,
        plan_nodes: vec![PlanNode {
            node_type: PlanNodeType::SeqScan,
            relation: Some("orders".to_string()),
            estimated_rows: 1_000_000.0,
            actual_rows: Some(50_000.0),
            startup_cost: 0.0,
            total_cost: 45000.0,
        }],
        rows_returned: 50_000,
        shared_hit: 1000,
        shared_read: 9000,
        severity: QuerySeverity::Normal,
        suggestion: String::new(),
        is_regression: false,
    });

    query_monitor.record(QueryRecord {
        query: "SELECT u.name, COUNT(o.id) FROM users u \
                JOIN orders o ON u.id = o.user_id \
                GROUP BY u.name"
            .to_string(),
        duration_ms: 850.0,
        total_cost: 12000.0,
        root_plan: PlanNodeType::HashJoin,
        plan_nodes: vec![
            PlanNode {
                node_type: PlanNodeType::HashJoin,
                relation: None,
                estimated_rows: 50_000.0,
                actual_rows: Some(45_000.0),
                startup_cost: 100.0,
                total_cost: 12000.0,
            },
            PlanNode {
                node_type: PlanNodeType::SeqScan,
                relation: Some("users".to_string()),
                estimated_rows: 100_000.0,
                actual_rows: None,
                startup_cost: 0.0,
                total_cost: 2000.0,
            },
        ],
        rows_returned: 45_000,
        shared_hit: 8000,
        shared_read: 2000,
        severity: QuerySeverity::Normal,
        suggestion: String::new(),
        is_regression: false,
    });

    // Demo schema issues
    let orders_table = TableSchemaInfo {
        name: "orders".to_string(),
        columns: vec![
            ColumnTypeInfo {
                name: "id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "user_id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "status".to_string(),
                pg_type: "text".to_string(),
                avg_width: 10,
            },
            ColumnTypeInfo {
                name: "metadata".to_string(),
                pg_type: "jsonb".to_string(),
                avg_width: 500,
            },
        ],
        indexes: vec![
            IndexUsage {
                name: "orders_pkey".to_string(),
                table: "orders".to_string(),
                columns: vec!["id".to_string()],
                index_type: "btree".to_string(),
                scans: 50_000,
                size_bytes: 2_097_152,
                is_unique: true,
                is_primary: true,
            },
            IndexUsage {
                name: "idx_orders_old".to_string(),
                table: "orders".to_string(),
                columns: vec!["status".to_string()],
                index_type: "btree".to_string(),
                scans: 0,
                size_bytes: 1_048_576,
                is_unique: false,
                is_primary: false,
            },
            IndexUsage {
                name: "idx_orders_metadata".to_string(),
                table: "orders".to_string(),
                columns: vec!["metadata".to_string()],
                index_type: "btree".to_string(),
                scans: 100,
                size_bytes: 4_194_304,
                is_unique: false,
                is_primary: false,
            },
        ],
        primary_key: vec!["id".to_string()],
        foreign_keys: vec![ForeignKeyInfo {
            name: "fk_orders_user".to_string(),
            columns: vec!["user_id".to_string()],
            referenced_table: "users".to_string(),
            referenced_columns: vec!["id".to_string()],
        }],
        seq_scan_count: 5000,
        filtered_columns: vec!["status".to_string(), "user_id".to_string()],
        dead_tuples: 200_000,
        live_tuples: 1_000_000,
    };
    schema_analyzer.add_table(orders_table);

    let logs_table = TableSchemaInfo {
        name: "audit_logs".to_string(),
        columns: vec![
            ColumnTypeInfo {
                name: "id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "event".to_string(),
                pg_type: "text".to_string(),
                avg_width: 200,
            },
        ],
        indexes: vec![],
        primary_key: vec![],
        foreign_keys: vec![],
        seq_scan_count: 200,
        filtered_columns: vec![],
        dead_tuples: 0,
        live_tuples: 500_000,
    };
    schema_analyzer.add_table(logs_table);

    schema_analyzer.analyze();

    // Demo configuration (typical defaults, not tuned)
    config_checker.load_config(PgConfig {
        shared_buffers: 128 * 1024 * 1024,
        effective_cache_size: 4 * 1024 * 1024 * 1024,
        work_mem: 4 * 1024 * 1024,
        maintenance_work_mem: 64 * 1024 * 1024,
        random_page_cost: 4.0,
        effective_io_concurrency: 1,
        default_statistics_target: 100,
        max_parallel_workers_per_gather: 2,
        parallel_tuple_cost: 0.01,
        system_ram: 32 * 1024 * 1024 * 1024,
        cpu_cores: 16,
        is_ssd: true,
    });
    config_checker.analyze();

    // Demo bloat
    bloat_detector.analyze_table(&TableBloatInput {
        table: "orders".to_string(),
        live_tuples: 1_000_000,
        dead_tuples: 200_000,
        last_autovacuum: None,
        index_bloat: vec![("idx_orders_old".to_string(), 500_000, 300_000)],
    });

    // Demo staleness
    staleness_checker.analyze_table(&TableStatsInput {
        table: "orders".to_string(),
        live_tuples: 1_000_000,
        modifications_since_analyze: 350_000,
        last_analyze: Some(1_700_000_000),
        last_autoanalyze: None,
        analyze_threshold: 50,
        analyze_scale_factor: 0.1,
    });
    staleness_checker.analyze_table(&TableStatsInput {
        table: "users".to_string(),
        live_tuples: 100_000,
        modifications_since_analyze: 5_000,
        last_analyze: Some(1_710_000_000),
        last_autoanalyze: Some(1_710_000_000),
        analyze_threshold: 50,
        analyze_scale_factor: 0.1,
    });
}
