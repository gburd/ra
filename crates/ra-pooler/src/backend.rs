//! Backend connection pool: maintains QUIC connections to
//! PostgreSQL backends running the ra-pg-quic extension.

use std::sync::atomic::{AtomicUsize, Ordering};

use ra_wire::capabilities::Capabilities;
use ra_wire::messages::Message;
use tracing::{debug, info, warn};

use crate::config::PoolerConfig;
use crate::error::PoolerError;
use crate::quic;

/// A single backend connection with its negotiated capabilities.
pub struct BackendChannel {
    pub connection: quinn::Connection,
    pub capabilities: Capabilities,
}

/// A single backend entry in the pool (one per configured backend).
struct BackendEntry {
    channel: BackendChannel,
    /// Used for weighted round-robin in future phases.
    _weight: u32,
}

/// Round-robin pool of QUIC connections to backends.
pub struct BackendPool {
    backends: Vec<BackendEntry>,
    next: AtomicUsize,
}

impl BackendPool {
    /// Connect to all configured backends and perform handshakes.
    ///
    /// Backends that fail to connect are logged and skipped. At
    /// least one backend must succeed for the pool to be usable.
    ///
    /// # Errors
    ///
    /// Returns `PoolerError::NoBackends` if no backend connections
    /// could be established.
    pub async fn connect(config: &PoolerConfig) -> Result<Self, PoolerError> {
        let client_config = quic::build_client_config()?;
        let endpoint = quic::create_endpoint(client_config)?;

        let mut backends = Vec::with_capacity(config.backends.len());

        for backend_cfg in &config.backends {
            match quic::connect_backend(&endpoint, backend_cfg.addr).await {
                Ok((connection, capabilities)) => {
                    info!(
                        addr = %backend_cfg.addr,
                        weight = backend_cfg.weight,
                        capabilities = ?capabilities,
                        "backend connected"
                    );
                    backends.push(BackendEntry {
                        channel: BackendChannel {
                            connection,
                            capabilities,
                        },
                        _weight: backend_cfg.weight,
                    });
                }
                Err(e) => {
                    warn!(
                        addr = %backend_cfg.addr,
                        error = %e,
                        "failed to connect to backend, skipping"
                    );
                }
            }
        }

        if backends.is_empty() {
            return Err(PoolerError::NoBackends);
        }

        info!(count = backends.len(), "backend pool initialized");

        Ok(Self {
            backends,
            next: AtomicUsize::new(0),
        })
    }

    /// Acquire a reference to the next backend channel using
    /// weighted round-robin selection.
    ///
    /// # Errors
    ///
    /// Returns `PoolerError::PoolExhausted` if the pool is empty.
    pub fn acquire(&self) -> Result<&BackendChannel, PoolerError> {
        if self.backends.is_empty() {
            return Err(PoolerError::PoolExhausted);
        }
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.backends.len();
        Ok(&self.backends[idx].channel)
    }

    /// Execute a raw SQL query on the next available backend.
    ///
    /// # Errors
    ///
    /// Returns `PoolerError` on pool, stream, or wire errors.
    pub async fn execute_raw_sql(&self, sql: &str) -> Result<Vec<Message>, PoolerError> {
        let channel = self.acquire()?;
        debug!(
            capabilities = ?channel.capabilities,
            "routing query to backend"
        );
        quic::execute_raw_sql(&channel.connection, sql).await
    }

    /// Return the number of connected backends in the pool.
    #[must_use]
    pub fn backend_count(&self) -> usize {
        self.backends.len()
    }
}
