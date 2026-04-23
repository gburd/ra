//! Protocol error types.

use serde::{Deserialize, Serialize};

/// Errors that can occur during wire protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("frame too large: {size} bytes (max {max})")]
    FrameTooLarge { size: u32, max: u32 },

    #[error("unsupported protocol version: {version} (expected {expected})")]
    UnsupportedVersion { version: u8, expected: u8 },

    #[error("unknown message type: {0:#06x}")]
    UnknownMessageType(u16),

    #[error("bincode serialization: {0}")]
    Bincode(#[from] bincode::Error),

    #[error("zstd compression: {0}")]
    ZstdCompress(std::io::Error),

    #[error("zstd decompression: {0}")]
    ZstdDecompress(std::io::Error),

    #[error("incomplete frame: need {needed} bytes, have {have}")]
    IncompleteFrame { needed: usize, have: usize },

    #[error("frame integrity: {0}")]
    FrameIntegrity(String),

    #[error("protocol error ({code}): {message}")]
    Protocol { code: u32, message: String },
}

/// SQL-state style error codes for protocol-level errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolErrorCode {
    /// Generic protocol violation.
    ProtocolViolation = 0x0001,
    /// Handshake failed (version mismatch, auth failure).
    HandshakeFailed = 0x0002,
    /// Message type not supported by this endpoint.
    UnsupportedMessage = 0x0003,
    /// Payload deserialization failed.
    MalformedPayload = 0x0004,
    /// Internal server error on the backend.
    InternalError = 0x0005,
    /// Request timeout.
    Timeout = 0x0006,
    /// Stale plan — table OIDs changed since optimization.
    StalePlan = 0x0100,
    /// Extension not installed on backend.
    ExtensionMissing = 0x0101,
}

impl ProtocolErrorCode {
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self as u32
    }

    /// Try to convert a `u32` to a known error code.
    #[must_use]
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x0001 => Some(Self::ProtocolViolation),
            0x0002 => Some(Self::HandshakeFailed),
            0x0003 => Some(Self::UnsupportedMessage),
            0x0004 => Some(Self::MalformedPayload),
            0x0005 => Some(Self::InternalError),
            0x0006 => Some(Self::Timeout),
            0x0100 => Some(Self::StalePlan),
            0x0101 => Some(Self::ExtensionMissing),
            _ => None,
        }
    }
}
