//! The `proxy` subcommand.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::proxy;

pub fn cmd_proxy(
    backend: &str,
    listen: &str,
    takeover: bool,
    log_format: &str,
    min_improvement: f64,
) -> Result<()> {
    let listen_addr: SocketAddr = listen
        .parse()
        .with_context(|| format!("invalid listen address: {listen}"))?;

    let log_fmt = log_format
        .parse::<proxy::LogFormat>()
        .with_context(|| format!("invalid log format: {log_format}"))?;

    let config = proxy::ProxyConfig {
        listen_addr,
        backend: backend.to_string(),
        enable_plan_takeover: takeover,
        log_format: log_fmt,
        min_improvement_percent: min_improvement,
    };

    eprintln!("{}", "Ra Database Proxy".bold().green());
    eprintln!();
    eprintln!(
        "  {}: {}",
        "Backend".bold(),
        proxy::mask_connection_string(backend)
    );
    eprintln!("  {}: {}", "Listening".bold(), listen);
    eprintln!("  {}: {:.1}%", "Min Improvement".bold(), min_improvement);

    if takeover {
        eprintln!(
            "  {}: {}",
            "Plan Takeover".bold(),
            "enabled (requires pg_plan_advice)".yellow()
        );
    }

    eprintln!();
    eprintln!(
        "{}",
        "Note: Full wire protocol implementation is a work in progress.".dimmed()
    );
    eprintln!(
        "{}",
        "      This command currently provides basic passthrough functionality.".dimmed()
    );
    eprintln!();

    let runtime = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;

    runtime.block_on(proxy::run_proxy(config))
}
