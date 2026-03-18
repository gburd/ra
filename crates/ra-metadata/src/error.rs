//! Error types for the metadata integration crate.

use thiserror::Error;

/// Errors from database metadata operations.
#[derive(Debug, Error)]
pub enum MetadataError {
    /// Failed to parse a connection string.
    #[error(
        "invalid connection string '{input}': {reason}"
    )]
    InvalidConnectionString {
        /// The original connection string.
        input: String,
        /// Why it was rejected.
        reason: String,
    },

    /// Failed to connect to the database.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// A query to the system catalog failed.
    #[error("catalog query failed: {0}")]
    CatalogQueryFailed(String),

    /// Failed to parse EXPLAIN output.
    #[error("EXPLAIN parse error: {0}")]
    ExplainParseFailed(String),

    /// The requested table was not found.
    #[error("table not found: {0}")]
    TableNotFound(String),

    /// A feature is not supported by this backend.
    #[error("{backend} does not support: {feature}")]
    Unsupported {
        /// Database backend name.
        backend: String,
        /// The unsupported feature.
        feature: String,
    },

    /// Generic I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
