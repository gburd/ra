//! CLI struct definitions for ra-cli.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

use crate::timeline_commands;

// ── Top-level CLI ──────────────────────────────────────────

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
pub struct Cli {
    /// Increase output verbosity (show per-file results, debug info).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress all non-error output.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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
            ra-cli explain 'SELECT ...' --hardware-profile gpu-server"
    )]
    Explain {
        /// SQL query to explain (ignored when --stdin is set).
        #[arg(default_value = "")]
        query: String,
        /// Hardware profile for cost estimation (auto, cpu-only, gpu-server, fpga).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
        /// Read SQL from stdin instead of the positional argument.
        #[arg(long)]
        stdin: bool,
        /// Timeline TOML file to use for schema/statistics/hardware context.
        #[arg(long)]
        timeline: Option<PathBuf>,
        /// Snapshot index from timeline to use (default: 0 = first snapshot).
        #[arg(long, default_value = "0")]
        snapshot: usize,
        /// Show plan provenance (cost-model snapshot, hardware
        /// hash, active rule-set hash, route, termination reason)
        /// after the plan output. Useful for reproducibility
        /// debugging — see `docs/research/geqo-vs-ra.md` lesson (ii).
        #[arg(long)]
        provenance: bool,
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
        /// Hardware profile for cost estimation (auto, cpu-only, gpu-server, fpga).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
        /// Read SQL from stdin instead of the positional argument.
        #[arg(long)]
        stdin: bool,
        /// Path to JSON file containing schema metadata and statistics.
        #[arg(long)]
        schema_json: Option<PathBuf>,
        /// Path to SQL file containing DDL (CREATE TABLE, CREATE INDEX statements).
        #[arg(long)]
        schema_sql: Option<PathBuf>,
        /// Database connection URL to extract live schema and statistics
        /// (postgresql://, mysql://, sqlite://, duckdb:// or file path).
        #[arg(long)]
        db: Option<String>,
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
        /// Enable the rule advisor pipeline for intelligent rule filtering.
        /// Eliminates irrelevant rules based on database context and query shape.
        #[arg(long)]
        rule_advisor: bool,
        /// Enable rule advisor learning (Stage 3). Persists effectiveness
        /// data to ~/.ra/rule-knowledge.json for future optimization runs.
        #[arg(long)]
        rule_advisor_learn: bool,
        /// Target database for rule advisor context filtering
        /// (e.g. "postgresql", "mysql", "documentdb", "oracle").
        #[arg(long)]
        rule_advisor_db: Option<String>,
        /// Timeline TOML file to use for schema/statistics/hardware context.
        #[arg(long)]
        timeline: Option<PathBuf>,
        /// Snapshot index from timeline to use (default: 0 = first snapshot).
        #[arg(long, default_value = "0")]
        snapshot: usize,
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
    /// PostgreSQL snapshot capture commands.
    #[command(subcommand)]
    PgSnapshot(PgSnapshotCommands),
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
    /// Run as a database proxy to intercept and optimize queries.
    #[command(
        long_about = "Start Ra as a proxy server that intercepts database queries.\n\n\
            The proxy sits between clients and the database, comparing Ra's optimized\n\
            plans with the database's EXPLAIN output and logging improvements.\n\n\
            Examples:\n  \
            ra-cli proxy postgres://localhost:5432/mydb\n  \
            ra-cli proxy postgres://localhost/db --listen 127.0.0.1:5433\n  \
            ra-cli proxy postgres://localhost/db --takeover  # Use pg_plan_advice"
    )]
    Proxy {
        /// Backend database connection string.
        backend: String,
        /// Address to listen on (default: 127.0.0.1:5433).
        #[arg(long, default_value = "127.0.0.1:5433")]
        listen: String,
        /// Enable plan takeover using pg_plan_advice (Postgres 19+).
        #[arg(long)]
        takeover: bool,
        /// Log format: postgres, json, or plain (default: postgres).
        #[arg(long, default_value = "postgres")]
        log_format: String,
        /// Minimum cost improvement % to log (default: 10.0).
        #[arg(long, default_value = "10.0")]
        min_improvement: f64,
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
    /// Optimize query through timeline of evolving fingerprints.
    #[command(
        long_about = "Optimize a query through a timeline of evolving database fingerprints.\n\n\
            Timelines capture schema changes, statistics updates, and hardware changes \
            over time to test plan adaptation and fingerprint invalidation.\n\n\
            Examples:\n  \
            ra-cli timeline --timeline timelines/index-addition.toml\n  \
            ra-cli timeline --timeline timelines/growth-replan.toml --output json\n  \
            ra-cli timeline --timeline timelines/test.toml --test\n  \
            ra-cli timeline --timeline timelines/demo.toml --tui\n  \
            ra-cli timeline --timeline timelines/test.toml --snapshots 0,2,4"
    )]
    Timeline(timeline_commands::TimelineCommand),
    /// ML model management commands (train, load, save, stats, export).
    #[command(subcommand)]
    Ml(crate::ml_commands::MlCommands),
    /// Run comparison benchmarks against native RDBMS implementations.
    #[command(
        long_about = "Run performance comparison benchmarks between Ra and native database implementations.\n\n\
            Compare Ra's optimized query execution against PostgreSQL, MySQL, SQLite, and DuckDB \
            across different workload types including hybrid search, vector search, joins, and analytics.\n\n\
            Examples:\n  \
            ra-cli benchmark --all\n  \
            ra-cli benchmark --database postgresql --workload hybrid-search\n  \
            ra-cli benchmark --database mysql --workload joins --output results.json\n  \
            ra-cli benchmark --all --format html --output comparison.html"
    )]
    Benchmark {
        /// Run benchmarks for all databases and workloads.
        #[arg(long)]
        all: bool,
        /// Database system to benchmark: postgresql, mysql, sqlite, duckdb.
        #[arg(long)]
        database: Option<String>,
        /// Workload type: hybrid-search, vector-search, fts, joins, aggregates, analytics.
        #[arg(long)]
        workload: Option<String>,
        /// Output file path.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Output format: json, markdown, html.
        #[arg(long, default_value = "markdown")]
        format: String,
    },
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

// ── Subcommand enums ───────────────────────────────────────

#[derive(Subcommand)]
pub enum RegressionCommands {
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
pub enum MigrateCommands {
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
pub enum PgSnapshotCommands {
    /// Capture a snapshot from a live PostgreSQL database.
    Capture {
        /// PostgreSQL connection URL (e.g. postgresql://localhost/mydb).
        #[arg(long)]
        database: String,
        /// Tables to capture (format: schema.table).
        #[arg(long, value_delimiter = ',')]
        tables: Vec<String>,
        /// Output file path for the snapshot TOML.
        #[arg(short, long)]
        output: PathBuf,
        /// Optional snapshot label.
        #[arg(short, long)]
        label: Option<String>,
    },
    /// Generate a SQL script for capturing snapshots.
    GenerateScript {
        /// Tables to capture (format: schema.table).
        #[arg(long, value_delimiter = ',')]
        tables: Vec<String>,
        /// Output directory for captured snapshots.
        #[arg(short, long)]
        output_dir: PathBuf,
        /// Time interval between snapshots (seconds).
        #[arg(long)]
        interval: Option<u64>,
        /// Output file path for the SQL script.
        #[arg(short, long, default_value = "capture.sql")]
        script: PathBuf,
    },
    /// Merge multiple snapshots into a timeline configuration.
    MergeTimeline {
        /// Directory containing snapshot TOML files.
        #[arg(long)]
        snapshot_dir: PathBuf,
        /// Output file path for the timeline TOML.
        #[arg(short, long)]
        output: PathBuf,
        /// Timeline name.
        #[arg(short, long)]
        name: String,
        /// Timeline description.
        #[arg(short, long)]
        description: String,
    },
}

#[derive(Subcommand)]
pub enum StatsTimelineCommands {
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
pub enum FederatedCommands {
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

#[derive(Subcommand)]
pub enum ConfigCommands {
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
pub enum CacheCommands {
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

/// What level of rule tracking information to display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleDisplayMode {
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
    pub fn from_flags(
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

    pub fn should_track(self) -> bool {
        !matches!(self, Self::None)
    }
}
