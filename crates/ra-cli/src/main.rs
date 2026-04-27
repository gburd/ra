//! Command-line interface for the relational algebra rule system.
#![allow(clippy::print_stderr)]

mod cache_commands;
mod cli;
mod commands;
mod config_commands;
mod diff_validator;
mod display;
mod federated_commands;
mod helpers;
mod migrate_commands;
mod ml_commands;
mod output;
mod pg_snapshot_commands;
pub(crate) mod plan_diff;
mod proxy;
mod regression_commands;
pub(crate) mod router;
mod rule_explanations;
pub(crate) mod side_by_side;
mod stats_commands;
mod test_executor;
mod timeline_commands;
mod visualize;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Commands};

// ── Main ────────────────────────────────────────────────────

fn main() {
    if let Err(e) = run_main() {
        let debug_level = std::env::var("DEBUG_RA")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);

        eprintln!("Error: {e}");

        if debug_level > 1 {
            eprintln!("\nStack backtrace:");
            eprintln!("{:?}", e.backtrace());
        }

        std::process::exit(1);
    }
}

fn run_main() -> Result<()> {
    let cli = Cli::parse();

    let is_test_cmd = matches!(cli.command, Commands::Test { .. });

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

    router::dispatch(cli)
}
