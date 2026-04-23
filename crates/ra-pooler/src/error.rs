//! Pooler-specific error types.

use std::net::SocketAddr;

/// Errors that can occur during pooler operation.
#[derive(Debug, thiserror::Error)]
pub enum PoolerError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("failed to bind listener on {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },

    #[error("QUIC connection to backend {addr} failed: {source}")]
    QuicConnect {
        addr: SocketAddr,
        source: anyhow::Error,
    },

    #[error("QUIC stream error: {0}")]
    QuicStream(String),

    #[error("wire protocol error: {0}")]
    Wire(#[from] ra_wire::error::WireError),

    #[error("backend handshake failed: {0}")]
    Handshake(String),

    #[error("PG wire protocol error: {0}")]
    PgWire(String),

    #[error("no backends available")]
    NoBackends,

    #[error("backend pool exhausted")]
    PoolExhausted,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
