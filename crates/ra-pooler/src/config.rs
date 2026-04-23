//! Configuration types for the Ra connection pooler.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::PoolerError;

/// Top-level pooler configuration, parsed from a TOML file.
#[derive(Debug, Clone, Deserialize)]
pub struct PoolerConfig {
    /// Address to listen on for PG wire protocol connections.
    #[serde(default = "default_listen_addr")]
    pub listen_addr: SocketAddr,

    /// Backend QUIC endpoints to connect to.
    pub backends: Vec<BackendConfig>,

    /// Maximum number of QUIC connections per backend.
    #[serde(default = "default_pool_size")]
    pub pool_size: usize,

    /// Path to TLS certificate (PEM) for QUIC client auth.
    #[serde(default)]
    pub tls_cert: Option<PathBuf>,

    /// Path to TLS private key (PEM) for QUIC client auth.
    #[serde(default)]
    pub tls_key: Option<PathBuf>,
}

/// Configuration for a single backend PostgreSQL instance.
#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfig {
    /// QUIC address of the backend (ra-pg-quic extension).
    pub addr: SocketAddr,

    /// Weight for weighted round-robin routing (higher = more traffic).
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_listen_addr() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 5433))
}

fn default_pool_size() -> usize {
    10
}

fn default_weight() -> u32 {
    1
}

/// Load configuration from a TOML file at the given path.
///
/// # Errors
///
/// Returns `PoolerError::Config` if the file cannot be read or
/// parsed, or if the configuration is invalid.
pub fn load_config(path: &str) -> Result<PoolerConfig, PoolerError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| PoolerError::Config(format!("failed to read config file '{path}': {e}")))?;
    let config: PoolerConfig = toml::from_str(&contents)
        .map_err(|e| PoolerError::Config(format!("failed to parse config file '{path}': {e}")))?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &PoolerConfig) -> Result<(), PoolerError> {
    if config.backends.is_empty() {
        return Err(PoolerError::Config(
            "at least one backend must be configured".into(),
        ));
    }
    if config.pool_size == 0 {
        return Err(PoolerError::Config(
            "pool_size must be greater than 0".into(),
        ));
    }
    Ok(())
}
