//! Capability flags negotiated during the QUIC handshake.
//!
//! Both pooler and backend advertise their capabilities.
//! The intersection determines the session's feature set.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Protocol capabilities negotiated at connection time.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Capabilities: u64 {
        /// Arrow IPC result format for OLAP workloads.
        const ARROW_IPC = 1 << 0;
        /// Zstd frame compression for payloads > 4 KiB.
        const ZSTD_COMPRESSION = 1 << 1;
        /// Differential (per-resource) invalidation notices.
        const DELTA_INVALIDATION = 1 << 2;
        /// Streaming statistics subscription.
        const STREAMING_STATS = 1 << 3;
        /// Prepared statement support via PrepareStatement.
        const PREPARED_PLANS = 1 << 4;
        /// Server-side cursor support.
        const CURSOR_SUPPORT = 1 << 5;
        /// COPY protocol support.
        const COPY_SUPPORT = 1 << 6;
        /// Runtime stats collection (EXPLAIN ANALYZE).
        const RUNTIME_STATS = 1 << 7;
    }
}

impl Serialize for Capabilities {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.bits().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Capabilities {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bits = u64::deserialize(deserializer)?;
        Self::from_bits(bits)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown capability bits: {bits:#x}")))
    }
}

impl Capabilities {
    /// Return the intersection of two capability sets.
    #[must_use]
    pub fn negotiate(self, remote: Self) -> Self {
        self & remote
    }

    /// Default capabilities for a pooler.
    #[must_use]
    pub fn pooler_defaults() -> Self {
        Self::ZSTD_COMPRESSION
            | Self::DELTA_INVALIDATION
            | Self::STREAMING_STATS
            | Self::PREPARED_PLANS
            | Self::CURSOR_SUPPORT
            | Self::COPY_SUPPORT
            | Self::RUNTIME_STATS
    }

    /// Default capabilities for a backend.
    #[must_use]
    pub fn backend_defaults() -> Self {
        Self::ZSTD_COMPRESSION
            | Self::DELTA_INVALIDATION
            | Self::PREPARED_PLANS
            | Self::CURSOR_SUPPORT
            | Self::COPY_SUPPORT
            | Self::RUNTIME_STATS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_intersection() {
        let pooler = Capabilities::ARROW_IPC | Capabilities::ZSTD_COMPRESSION;
        let backend = Capabilities::ZSTD_COMPRESSION | Capabilities::STREAMING_STATS;
        let negotiated = pooler.negotiate(backend);
        assert_eq!(negotiated, Capabilities::ZSTD_COMPRESSION);
    }

    #[test]
    fn serde_round_trip() {
        let caps = Capabilities::pooler_defaults();
        let encoded = bincode::serialize(&caps).expect("serialize caps");
        let decoded: Capabilities = bincode::deserialize(&encoded).expect("deserialize caps");
        assert_eq!(caps, decoded);
    }

    #[test]
    fn unknown_bits_rejected() {
        let bad_bits: u64 = 1 << 63;
        let encoded = bincode::serialize(&bad_bits).expect("serialize raw");
        let result: Result<Capabilities, _> = bincode::deserialize(&encoded);
        assert!(result.is_err());
    }
}
