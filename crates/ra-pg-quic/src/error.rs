//! Error types for the ra-pg-quic background worker.

/// Errors from the QUIC listener background worker.
#[derive(Debug, thiserror::Error)]
pub enum QuicWorkerError {
    #[error("TLS certificate generation failed: {0}")]
    CertGeneration(#[from] rcgen::Error),

    #[error("TLS configuration failed: {0}")]
    Tls(#[from] rustls::Error),

    #[error("QUIC endpoint bind failed: {0}")]
    EndpointBind(std::io::Error),

    #[error("QUIC connection error: {0}")]
    Connection(#[from] quinn::ConnectionError),

    #[error("wire protocol error: {0}")]
    Wire(#[from] ra_wire::error::WireError),

    #[error("bincode serialization error: {0}")]
    Bincode(#[from] bincode::Error),

    #[error("SPI execution failed: {0}")]
    Spi(String),

    #[error("stream read error: {0}")]
    StreamRead(#[from] quinn::ReadExactError),

    #[error("stream write error: {0}")]
    StreamWrite(#[from] quinn::WriteError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("handshake failed: {0}")]
    Handshake(String),

    #[error("background worker shutting down")]
    Shutdown,
}
