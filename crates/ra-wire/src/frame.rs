//! 16-byte frame header encoding and decoding.
//!
//! ```text
//!  0       1       2       3
//!  +-------+-------+-------+-------+
//!  |Ver|Flg| MsgType (u16) |  Rsvd |
//!  +-------+-------+-------+-------+
//!  |     Payload Length (u32)       |
//!  +-------+-------+-------+-------+
//!  |        Request ID (u64)       |
//!  |                               |
//!  +-------+-------+-------+-------+
//! ```
//!
//! - Version: 4 bits (current: 1)
//! - Flags: 4 bits (`COMPRESSED`, `LAST_FRAME`, `ERROR`, reserved)
//! - `MsgType`: big-endian u16 message type code
//! - Reserved: 1 byte (must be 0)
//! - Payload Length: big-endian u32
//! - Request ID: big-endian u64

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::WireError;
use crate::messages::MessageType;

/// Current protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Frame header size in bytes.
pub const HEADER_SIZE: usize = 16;

/// Maximum payload size: 64 MiB.
pub const MAX_PAYLOAD_SIZE: u32 = 64 * 1024 * 1024;

/// Minimum payload size for zstd compression.
pub const COMPRESSION_THRESHOLD: usize = 4096;

// ── Flag bits (lower 4 bits of byte 0) ──────────────────────

/// Payload is zstd-compressed.
pub const FLAG_COMPRESSED: u8 = 0b0001;
/// Last frame in a streaming sequence.
pub const FLAG_LAST_FRAME: u8 = 0b0010;
/// Frame carries an error response.
pub const FLAG_ERROR: u8 = 0b0100;

/// Parsed frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub version: u8,
    pub flags: u8,
    pub message_type: u16,
    pub payload_length: u32,
    pub request_id: u64,
}

impl FrameHeader {
    /// Create a new frame header.
    #[must_use]
    pub fn new(message_type: MessageType, payload_length: u32, request_id: u64) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            flags: 0,
            message_type: message_type.code(),
            payload_length,
            request_id,
        }
    }

    /// Set the compressed flag.
    #[must_use]
    pub fn with_compressed(mut self) -> Self {
        self.flags |= FLAG_COMPRESSED;
        self
    }

    /// Set the last-frame flag.
    #[must_use]
    pub fn with_last_frame(mut self) -> Self {
        self.flags |= FLAG_LAST_FRAME;
        self
    }

    /// Set the error flag.
    #[must_use]
    pub fn with_error(mut self) -> Self {
        self.flags |= FLAG_ERROR;
        self
    }

    #[must_use]
    pub fn is_compressed(self) -> bool {
        self.flags & FLAG_COMPRESSED != 0
    }

    #[must_use]
    pub fn is_last_frame(self) -> bool {
        self.flags & FLAG_LAST_FRAME != 0
    }

    #[must_use]
    pub fn is_error(self) -> bool {
        self.flags & FLAG_ERROR != 0
    }

    /// Encode the header into 16 bytes.
    pub fn encode(&self, dst: &mut BytesMut) {
        let ver_flags = (self.version << 4) | (self.flags & 0x0F);
        dst.put_u8(ver_flags);
        dst.put_u16(self.message_type);
        dst.put_u8(0); // reserved
        dst.put_u32(self.payload_length);
        dst.put_u64(self.request_id);
    }

    /// Decode a header from a byte buffer. Returns `None` if
    /// the buffer has fewer than `HEADER_SIZE` bytes.
    ///
    /// # Errors
    ///
    /// Returns `WireError` if version is unsupported or
    /// payload exceeds `MAX_PAYLOAD_SIZE`.
    pub fn decode(src: &mut Bytes) -> Result<Option<Self>, WireError> {
        if src.len() < HEADER_SIZE {
            return Ok(None);
        }
        let ver_flags = src.get_u8();
        let version = ver_flags >> 4;
        let flags = ver_flags & 0x0F;
        let message_type = src.get_u16();
        let _reserved = src.get_u8();
        let payload_length = src.get_u32();
        let request_id = src.get_u64();

        if version != PROTOCOL_VERSION {
            return Err(WireError::UnsupportedVersion {
                version,
                expected: PROTOCOL_VERSION,
            });
        }
        if payload_length > MAX_PAYLOAD_SIZE {
            return Err(WireError::FrameTooLarge {
                size: payload_length,
                max: MAX_PAYLOAD_SIZE,
            });
        }

        Ok(Some(Self {
            version,
            flags,
            message_type,
            payload_length,
            request_id,
        }))
    }
}

/// A complete frame: header + payload bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Bytes,
}

impl Frame {
    /// Create a new frame.
    #[must_use]
    pub fn new(header: FrameHeader, payload: Bytes) -> Self {
        Self { header, payload }
    }

    /// Encode the full frame (header + payload) into a buffer.
    pub fn encode(&self, dst: &mut BytesMut) {
        self.header.encode(dst);
        dst.extend_from_slice(&self.payload);
    }

    /// Attempt to decode a complete frame from a buffer.
    /// Returns `Ok(None)` if more data is needed.
    ///
    /// # Errors
    ///
    /// Returns `WireError` on malformed headers.
    pub fn decode(src: &mut Bytes) -> Result<Option<Self>, WireError> {
        if src.len() < HEADER_SIZE {
            return Ok(None);
        }

        // Peek at payload length without consuming.
        let payload_length = u32::from_be_bytes([src[4], src[5], src[6], src[7]]) as usize;
        let total = HEADER_SIZE + payload_length;

        if src.len() < total {
            return Err(WireError::IncompleteFrame {
                needed: total,
                have: src.len(),
            });
        }

        let mut header_bytes = src.split_to(HEADER_SIZE);
        let Some(header) = FrameHeader::decode(&mut header_bytes)? else {
            return Err(WireError::FrameIntegrity(
                "header decode returned None despite \
                 sufficient bytes"
                    .into(),
            ));
        };
        let payload = src.split_to(payload_length);

        Ok(Some(Self { header, payload }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trip() {
        let header = FrameHeader::new(MessageType::Handshake, 42, 12345)
            .with_compressed()
            .with_last_frame();

        let mut buf = BytesMut::with_capacity(HEADER_SIZE);
        header.encode(&mut buf);
        assert_eq!(buf.len(), HEADER_SIZE);

        let mut bytes = buf.freeze();
        let decoded = FrameHeader::decode(&mut bytes)
            .expect("decode ok")
            .expect("complete header");

        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert!(decoded.is_compressed());
        assert!(decoded.is_last_frame());
        assert!(!decoded.is_error());
        assert_eq!(decoded.message_type, MessageType::Handshake.code());
        assert_eq!(decoded.payload_length, 42);
        assert_eq!(decoded.request_id, 12345);
    }

    #[test]
    fn frame_round_trip() {
        let payload = Bytes::from_static(b"hello world");
        let header = FrameHeader::new(MessageType::ExecuteRawSql, payload.len() as u32, 99);
        let frame = Frame::new(header, payload.clone());

        let mut buf = BytesMut::new();
        frame.encode(&mut buf);

        let mut bytes = buf.freeze();
        let decoded = Frame::decode(&mut bytes)
            .expect("decode ok")
            .expect("complete frame");

        assert_eq!(decoded.header.request_id, 99);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn incomplete_header_returns_none() {
        let mut bytes = Bytes::from_static(&[0u8; 8]);
        let result = FrameHeader::decode(&mut bytes).expect("no error");
        assert!(result.is_none());
    }

    #[test]
    fn bad_version_rejected() {
        let mut buf = BytesMut::with_capacity(HEADER_SIZE);
        // version 15, no flags
        buf.put_u8(0xF0);
        buf.put_u16(0x0001);
        buf.put_u8(0);
        buf.put_u32(0);
        buf.put_u64(0);

        let mut bytes = buf.freeze();
        let err = FrameHeader::decode(&mut bytes).unwrap_err();
        assert!(matches!(err, WireError::UnsupportedVersion { .. }));
    }

    #[test]
    fn oversized_payload_rejected() {
        let mut buf = BytesMut::with_capacity(HEADER_SIZE);
        buf.put_u8(PROTOCOL_VERSION << 4);
        buf.put_u16(0x0001);
        buf.put_u8(0);
        buf.put_u32(MAX_PAYLOAD_SIZE + 1);
        buf.put_u64(0);

        let mut bytes = buf.freeze();
        let err = FrameHeader::decode(&mut bytes).unwrap_err();
        assert!(matches!(err, WireError::FrameTooLarge { .. }));
    }
}
