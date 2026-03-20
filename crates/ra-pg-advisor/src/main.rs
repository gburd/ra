//! `ra-pg-advisor` CLI: `PostgreSQL` plan advisor powered by RA.
//!
//! Modes:
//! - Daemon: continuously monitor and advise
//! - One-shot: analyze a single query

use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, Level};

use ra_pg_advisor::advice_gen::AdviceGenerator;
use ra_pg_advisor::daemon::{AdvisorDaemon, DaemonConfig};
use ra_pg_advisor::pg_api::{AdviceBackend, PgAdviceApi};

#[derive(Parser)]
#[command(
    name = "ra-pg-advisor",
    about = "PostgreSQL plan advisor powered by RA optimizer",
    version
)]
struct Cli {
    /// `PostgreSQL` connection string.
    #[arg(
        long,
        env = "RA_PG_ADVISOR_POSTGRES",
        global = true
    )]
    postgres: Option<String>,

    /// Log level.
    #[arg(long, default_value = "info", global = true)]
    log_level: Level,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the advisor daemon.
    Daemon {
        /// Polling interval in seconds.
        #[arg(long, default_value = "5")]
        interval: u64,

        /// Slow query threshold in milliseconds.
        #[arg(long, default_value = "100")]
        slow_threshold: f64,

        /// Preferred advice backend.
        #[arg(long, default_value = "guc")]
        backend: String,

        /// Minimum confidence to apply advice (0.0-1.0).
        #[arg(long, default_value = "0.7")]
        min_confidence: f64,
    },

    /// Analyze a single query and show advice.
    Analyze {
        /// SQL query to analyze.
        #[arg(long)]
        query: String,

        /// Output format: advice, hint-plan, sql.
        #[arg(long, default_value = "advice")]
        format: String,
    },

    /// Show available backends for this `PostgreSQL` instance.
    Backends,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(cli.log_level)
        .init();

    match cli.command {
        Command::Daemon {
            interval,
            slow_threshold,
            backend,
            min_confidence,
        } => {
            run_daemon(
                cli.postgres.as_deref(),
                interval,
                slow_threshold,
                &backend,
                min_confidence,
            )
        }
        Command::Analyze { query, format } => {
            run_analyze(&query, &format)
        }
        Command::Backends => {
            run_backends();
            Ok(())
        }
    }
}

fn run_daemon(
    postgres: Option<&str>,
    interval: u64,
    slow_threshold: f64,
    backend_str: &str,
    min_confidence: f64,
) -> Result<()> {
    let conn = postgres
        .context("--postgres is required for daemon mode")?;
    let backend = parse_backend(backend_str)?;

    let config = DaemonConfig {
        connection_string: conn.to_string(),
        poll_interval: Duration::from_secs(interval),
        slow_threshold_ms: slow_threshold,
        backend,
        min_confidence,
        ..DaemonConfig::default()
    };

    info!(
        backend = %config.backend,
        interval_secs = interval,
        threshold_ms = slow_threshold,
        "starting advisor daemon"
    );

    let _daemon = AdvisorDaemon::new(config);

    info!(
        "daemon initialized \
         (polling not yet implemented -- use library API)"
    );
    Ok(())
}

fn run_analyze(query: &str, format: &str) -> Result<()> {
    info!(query, "analyzing query");

    let plan = ra_core::RelExpr::Scan {
        table: "placeholder".to_string(),
        alias: None,
    };

    let generator = AdviceGenerator::new(0.8);
    let advice = generator.generate("cli", &plan);

    match format {
        "advice" => {
            let combined =
                AdviceGenerator::combine_advice(&advice);
            if combined.is_empty() {
                info!("no advice generated for this query");
            } else {
                info!(
                    advice = combined.as_str(),
                    "generated advice"
                );
            }
        }
        "hint-plan" => {
            let hint =
                AdviceGenerator::to_pg_hint_plan(&advice);
            if hint.is_empty() {
                info!("no pg_hint_plan hints generated");
            } else {
                info!(
                    hint = hint.as_str(),
                    "pg_hint_plan format"
                );
            }
        }
        "sql" => {
            let api = PgAdviceApi::new(
                AdviceBackend::PgPlanAdviceGuc,
            );
            let sql = api.format_apply_sql(
                &advice,
                Some(query),
            );
            info!(sql = sql.as_str(), "apply SQL");
        }
        other => {
            bail!(
                "unknown format: {other} \
                 (expected: advice, hint-plan, sql)"
            );
        }
    }

    Ok(())
}

fn run_backends() {
    let checks = PgAdviceApi::detection_order();
    for (backend, sql) in &checks {
        info!(
            backend = %backend,
            check_sql = sql.as_str(),
            "backend detection query"
        );
    }
}

fn parse_backend(s: &str) -> Result<AdviceBackend> {
    match s {
        "guc" | "pg_plan_advice" => {
            Ok(AdviceBackend::PgPlanAdviceGuc)
        }
        "stash" | "pg_stash_advice" => {
            Ok(AdviceBackend::PgStashAdvice)
        }
        "hint" | "pg_hint_plan" => {
            Ok(AdviceBackend::PgHintPlan)
        }
        other => {
            bail!(
                "unknown backend: {other} \
                 (expected: guc, stash, hint)"
            )
        }
    }
}
