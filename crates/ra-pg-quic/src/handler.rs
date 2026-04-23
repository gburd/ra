//! QUIC connection and stream handler.
//!
//! Each accepted QUIC connection spawns a task that reads frames
//! from incoming uni/bi streams, dispatches to SPI execution, and
//! writes response frames back.

use bytes::BytesMut;
use pgrx::prelude::*;
use quinn::{Connection, RecvStream, SendStream};

use ra_wire::capabilities::Capabilities;
use ra_wire::codec::{decode_from_bytes, encode_message};
use ra_wire::frame::HEADER_SIZE;
use ra_wire::messages::{
    HandshakeAckPayload, Message, ProtocolErrorPayload,
};

use crate::error::QuicWorkerError;
use crate::spi_executor;

/// Protocol version supported by this backend.
const PROTOCOL_VERSION: u8 = 1;

/// Server identifier returned in handshake acknowledgement.
const SERVER_ID: &str = "ra-pg-quic/0.2.0";

/// Handle a single QUIC connection.
///
/// Accepts bi-directional streams. Stream 0 (first opened) is
/// the control stream used for handshake. Subsequent streams
/// carry data plane messages.
///
/// # Errors
///
/// Returns `QuicWorkerError` on fatal connection-level errors.
pub async fn handle_connection(
    conn: Connection,
) -> Result<(), QuicWorkerError> {
    tracing::info!(
        remote = %conn.remote_address(),
        "accepted QUIC connection"
    );

    // First stream must be the handshake.
    let (send, recv) = conn.accept_bi().await?;
    if let Err(e) = handle_handshake(send, recv).await {
        tracing::warn!(
            remote = %conn.remote_address(),
            error = %e,
            "handshake failed"
        );
        return Err(e);
    }

    // Accept subsequent data streams.
    loop {
        let stream = conn.accept_bi().await;
        match stream {
            Ok((send, recv)) => {
                tokio::spawn(async move {
                    if let Err(e) =
                        handle_data_stream(send, recv).await
                    {
                        tracing::warn!(
                            error = %e,
                            "data stream error"
                        );
                    }
                });
            }
            Err(quinn::ConnectionError::ApplicationClosed(_)) => {
                tracing::info!(
                    remote = %conn.remote_address(),
                    "connection closed by peer"
                );
                break;
            }
            Err(e) => {
                tracing::warn!(
                    remote = %conn.remote_address(),
                    error = %e,
                    "connection error"
                );
                return Err(e.into());
            }
        }
    }

    Ok(())
}

/// Handle the handshake on the control stream.
///
/// Reads a `Handshake` message, validates the protocol version,
/// and sends back a `HandshakeAck`.
async fn handle_handshake(
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<(), QuicWorkerError> {
    let frame_bytes = read_frame(&mut recv).await?;
    let (msg, request_id) = decode_from_bytes(&frame_bytes)?;

    let Message::Handshake(handshake) = msg else {
        return Err(QuicWorkerError::Handshake(
            "first message must be Handshake".into(),
        ));
    };

    if handshake.version != PROTOCOL_VERSION {
        let err_msg = Message::ProtocolError(
            ProtocolErrorPayload {
                code: 0x0002,
                message: format!(
                    "unsupported version {}, expected {}",
                    handshake.version, PROTOCOL_VERSION,
                ),
            },
        );
        write_message(&mut send, &err_msg, request_id).await?;
        return Err(QuicWorkerError::Handshake(format!(
            "version mismatch: got {}, want {}",
            handshake.version, PROTOCOL_VERSION,
        )));
    }

    tracing::info!(
        pooler_id = %handshake.pooler_id,
        "handshake from pooler"
    );

    let negotiated = Capabilities::backend_defaults()
        .negotiate(handshake.capabilities);

    let pg_version = get_pg_version();

    let ack = Message::HandshakeAck(HandshakeAckPayload {
        version: PROTOCOL_VERSION,
        server_id: SERVER_ID.into(),
        capabilities: negotiated,
        pg_version,
    });

    write_message(&mut send, &ack, request_id).await?;
    Ok(())
}

/// Handle a single data stream carrying one request/response cycle.
async fn handle_data_stream(
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<(), QuicWorkerError> {
    let frame_bytes = read_frame(&mut recv).await?;
    let (msg, request_id) = decode_from_bytes(&frame_bytes)?;

    match msg {
        Message::ExecuteRawSql(payload) => {
            handle_execute_raw(
                &mut send,
                &payload.sql,
                request_id,
            )
            .await
        }
        Message::Shutdown(payload) => {
            tracing::info!(
                reason = %payload.reason,
                "shutdown requested"
            );
            Err(QuicWorkerError::Shutdown)
        }
        other => {
            tracing::warn!(
                msg_type = ?other.message_type(),
                "unsupported message type"
            );
            let err = Message::ProtocolError(
                ProtocolErrorPayload {
                    code: 0x0003,
                    message: format!(
                        "unsupported message type: {:?}",
                        other.message_type(),
                    ),
                },
            );
            write_message(&mut send, &err, request_id).await
        }
    }
}

/// Execute a raw SQL query via SPI and stream results back.
async fn handle_execute_raw(
    send: &mut SendStream,
    sql: &str,
    request_id: u64,
) -> Result<(), QuicWorkerError> {
    // SPI must run on the PostgreSQL backend thread.
    // In Phase 1, we execute synchronously via
    // `tokio::task::spawn_blocking` to call into SPI
    // from within pgrx's expected execution context.
    let sql_owned = sql.to_owned();
    let result = tokio::task::spawn_blocking(move || {
        spi_executor::execute_sql(&sql_owned)
    })
    .await
    .map_err(|e| {
        QuicWorkerError::Spi(format!("task join error: {e}"))
    })??;

    // Stream batches.
    for batch in &result.batches {
        write_message(send, batch, request_id).await?;
    }

    // Send RowEnd.
    write_message(send, &result.row_end, request_id).await?;
    Ok(())
}

/// Read a single framed message from a QUIC recv stream.
///
/// Reads the 16-byte header first to determine payload length,
/// then reads exactly that many payload bytes.
async fn read_frame(
    recv: &mut RecvStream,
) -> Result<Vec<u8>, QuicWorkerError> {
    let mut header_buf = vec![0u8; HEADER_SIZE];
    recv.read_exact(&mut header_buf).await?;

    // Extract payload length from bytes 4..8 (big-endian u32).
    let payload_len = u32::from_be_bytes([
        header_buf[4],
        header_buf[5],
        header_buf[6],
        header_buf[7],
    ]) as usize;

    let mut frame_buf =
        Vec::with_capacity(HEADER_SIZE + payload_len);
    frame_buf.extend_from_slice(&header_buf);

    let mut payload_buf = vec![0u8; payload_len];
    recv.read_exact(&mut payload_buf).await?;
    frame_buf.extend_from_slice(&payload_buf);

    Ok(frame_buf)
}

/// Write a framed message to a QUIC send stream.
async fn write_message(
    send: &mut SendStream,
    msg: &Message,
    request_id: u64,
) -> Result<(), QuicWorkerError> {
    let mut buf = BytesMut::new();
    encode_message(msg, request_id, false, &mut buf)?;
    send.write_all(&buf).await?;
    Ok(())
}

/// Get the PostgreSQL server version string via SPI.
fn get_pg_version() -> String {
    Spi::connect(|client| {
        client
            .select("SELECT version()", None, None)
            .ok()
            .and_then(|mut table| {
                table.next().and_then(|row| {
                    row.by_ordinal(1)
                        .ok()
                        .flatten()
                        .and_then(|v| v.value::<String>().ok())
                        .flatten()
                })
            })
            .unwrap_or_else(|| "PostgreSQL (unknown)".into())
    })
}
