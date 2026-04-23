#![expect(clippy::doc_markdown)]
//! Ra wire protocol: QUIC-based communication between Ra pooler
//! and PostgreSQL backends running the ra-pg-quic extension.
//!
//! Provides frame encoding/decoding, message type definitions,
//! capability negotiation, and bincode+zstd serialization.

pub mod capabilities;
pub mod codec;
pub mod error;
pub mod frame;
pub mod messages;
pub mod types;
