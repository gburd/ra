//! Database proxy for query optimization comparison.
//!
//! The proxy intercepts database queries, compares Ra's optimized plan
//! with the database server's plan, and optionally takes over planning
//! when Ra's plan is demonstrably better.

use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error, info};

/// Configuration for the proxy server.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Address to listen on (e.g., "127.0.0.1:5433").
    pub listen_addr: SocketAddr,
    /// Backend database connection string.
    pub backend: String,
    /// Enable pg_plan_advice integration (Postgres 19+).
    pub enable_plan_takeover: bool,
    /// Log format (postgres, json, or plain).
    /// TODO: Implement in full wire protocol handler (Issue #80)
    #[allow(dead_code)]
    pub log_format: LogFormat,
    /// Minimum improvement percentage to log (e.g., 10.0 for 10%).
    /// TODO: Implement in query comparison logic (Issue #80)
    #[allow(dead_code)]
    pub min_improvement_percent: f64,
}

/// Log output format.
#[derive(Debug, Clone, Copy)]
pub enum LogFormat {
    /// PostgreSQL-style log format.
    Postgres,
    /// JSON structured logging.
    Json,
    /// Plain text format.
    Plain,
}

impl std::str::FromStr for LogFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "postgres" | "pg" => Ok(Self::Postgres),
            "json" => Ok(Self::Json),
            "plain" | "text" => Ok(Self::Plain),
            _ => anyhow::bail!("unknown log format: {s}. Valid: postgres, json, plain"),
        }
    }
}

/// Start the proxy server.
pub async fn run_proxy(config: ProxyConfig) -> Result<()> {
    info!("Starting Ra proxy server on {}", config.listen_addr);
    info!("Backend: {}", mask_connection_string(&config.backend));

    if config.enable_plan_takeover {
        info!("Plan takeover enabled (requires Postgres 19+ with pg_plan_advice)");
    }

    let listener = TcpListener::bind(config.listen_addr)
        .await
        .context("failed to bind proxy listener")?;

    info!("Proxy listening on {}", config.listen_addr);

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                debug!("Accepted connection from {}", addr);
                let config = config.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_connection(socket, addr, config).await {
                        error!("Connection error from {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

/// Handle a single client connection.
async fn handle_connection(
    mut client: TcpStream,
    client_addr: SocketAddr,
    config: ProxyConfig,
) -> Result<()> {
    debug!("Handling connection from {}", client_addr);

    // For now, we'll implement a basic passthrough
    // Full wire protocol implementation would go here

    // Connect to backend
    let backend_addr = parse_backend_address(&config.backend)?;
    let mut backend = TcpStream::connect(&backend_addr)
        .await
        .context("failed to connect to backend database")?;

    info!("Connected to backend: {}", mask_connection_string(&config.backend));

    // Proxy data bidirectionally
    // In a full implementation, we would:
    // 1. Parse the wire protocol
    // 2. Intercept SQL queries
    // 3. Run EXPLAIN on backend
    // 4. Run Ra optimizer
    // 5. Compare plans
    // 6. Log if Ra is better
    // 7. Optionally take over planning

    let (mut client_read, mut client_write) = client.split();
    let (mut backend_read, mut backend_write) = backend.split();

    // Simple bidirectional forwarding for now
    let client_to_backend = async {
        let mut buf = vec![0u8; 8192];
        loop {
            match client_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if backend_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };

    let backend_to_client = async {
        let mut buf = vec![0u8; 8192];
        loop {
            match backend_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if client_write.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };

    // Wait for either direction to close
    tokio::select! {
        _ = client_to_backend => {},
        _ = backend_to_client => {},
    }

    debug!("Connection from {} closed", client_addr);
    Ok(())
}

/// Parse backend connection string to extract address.
fn parse_backend_address(connection_string: &str) -> Result<String> {
    // Simple parser for postgres:// URLs
    // Full implementation would handle all connection string formats

    if connection_string.starts_with("postgres://") || connection_string.starts_with("postgresql://") {
        // Extract host:port from URL
        let url = connection_string
            .strip_prefix("postgres://")
            .or_else(|| connection_string.strip_prefix("postgresql://"))
            .context("invalid connection string")?;

        // Parse: [user[:password]@]host[:port][/database]
        let parts: Vec<&str> = url.split('@').collect();
        let host_part = parts.last().context("no host in connection string")?;

        let host_db: Vec<&str> = host_part.split('/').collect();
        let host = host_db[0];

        // Add default port if not specified
        if host.contains(':') {
            Ok(host.to_string())
        } else {
            Ok(format!("{}:5432", host))
        }
    } else {
        // Assume direct host:port
        Ok(connection_string.to_string())
    }
}

/// Mask sensitive parts of connection string for logging.
pub fn mask_connection_string(s: &str) -> String {
    // Mask password in connection strings
    if let Some(at_pos) = s.find('@') {
        if let Some(scheme_end) = s.find("://") {
            let scheme = &s[..scheme_end + 3];
            let credentials = &s[scheme_end + 3..at_pos];
            let rest = &s[at_pos..];

            // Mask password if present
            if let Some(colon_pos) = credentials.find(':') {
                let user = &credentials[..colon_pos];
                return format!("{}{}:****{}", scheme, user, rest);
            }
        }
    }

    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_postgres_url() {
        let addr = parse_backend_address("postgres://user:pass@localhost/mydb").unwrap();
        assert_eq!(addr, "localhost:5432");

        let addr = parse_backend_address("postgres://localhost:5433/mydb").unwrap();
        assert_eq!(addr, "localhost:5433");
    }

    #[test]
    fn mask_password() {
        let masked = mask_connection_string("postgres://user:secret@localhost/db");
        assert!(masked.contains("****"));
        assert!(!masked.contains("secret"));
        assert!(masked.contains("user"));
    }

    #[test]
    fn parse_log_format() {
        assert!(matches!("postgres".parse::<LogFormat>().unwrap(), LogFormat::Postgres));
        assert!(matches!("json".parse::<LogFormat>().unwrap(), LogFormat::Json));
        assert!(matches!("plain".parse::<LogFormat>().unwrap(), LogFormat::Plain));
    }
}
