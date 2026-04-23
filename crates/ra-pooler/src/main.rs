//! Ra connection pooler binary.
//!
//! Sits between PostgreSQL clients (standard libpq/JDBC) and
//! PostgreSQL backends. Clients talk PG wire protocol to the
//! pooler; the pooler talks QUIC to backends running the
//! ra-pg-quic extension.

mod backend;
mod config;
mod error;
mod frontend;
mod quic;

use std::sync::Arc;

use tracing::info;

/// Entry point: parse config, connect to backends, start the PG
/// wire frontend listener.
///
/// # Errors
///
/// Exits with a non-zero status on configuration, connection, or
/// I/O errors.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path = parse_args()?;
    let config = config::load_config(&config_path)?;

    info!(
        listen_addr = %config.listen_addr,
        backends = config.backends.len(),
        pool_size = config.pool_size,
        tls_cert = ?config.tls_cert,
        tls_key = ?config.tls_key,
        "ra-pooler starting"
    );

    for (i, backend) in config.backends.iter().enumerate() {
        info!(
            index = i,
            addr = %backend.addr,
            weight = backend.weight,
            "configured backend"
        );
    }

    let pool = Arc::new(backend::BackendPool::connect(&config).await?);

    info!(connected = pool.backend_count(), "backend pool ready");

    frontend::run_frontend(config.listen_addr, pool).await?;

    Ok(())
}

/// Parse command-line arguments. Expects exactly one argument:
/// the path to the TOML configuration file.
///
/// Usage: `ra-pooler <config.toml>`
fn parse_args() -> anyhow::Result<String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        anyhow::bail!(
            "usage: {} <config.toml>",
            args.first().map_or("ra-pooler", String::as_str)
        );
    }
    Ok(args[1].clone())
}
