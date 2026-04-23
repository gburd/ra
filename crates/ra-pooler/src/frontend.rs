//! PG wire protocol frontend: accepts standard PostgreSQL client
//! connections and proxies queries to QUIC backends.

use std::sync::Arc;

use bytes::{BufMut, BytesMut};
use ra_wire::messages::{Message, RowBatchPayload};
use ra_wire::types::{ResultSchema, RowData};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

use crate::backend::BackendPool;
use crate::error::PoolerError;

/// PG wire protocol version 3.0 as a big-endian i32.
const PG_PROTOCOL_V3: i32 = 196_608;

/// SSL request code (sent before startup if client wants SSL).
const SSL_REQUEST_CODE: i32 = 80_877_103;

/// Start the PG wire frontend listener.
///
/// Accepts TCP connections on `listen_addr`, spawning a task per
/// client. Each client session reads PG wire messages and forwards
/// queries to the backend pool.
///
/// # Errors
///
/// Returns `PoolerError::Bind` if the TCP listener cannot bind.
pub async fn run_frontend(
    listen_addr: std::net::SocketAddr,
    pool: Arc<BackendPool>,
) -> Result<(), PoolerError> {
    let listener = TcpListener::bind(listen_addr)
        .await
        .map_err(|e| PoolerError::Bind {
            addr: listen_addr,
            source: e,
        })?;

    info!(%listen_addr, "PG wire frontend listening");

    loop {
        let (stream, peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                warn!(error = %e, "failed to accept connection");
                continue;
            }
        };

        debug!(%peer_addr, "new client connection");

        let pool = Arc::clone(&pool);
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, &pool).await {
                error!(
                    %peer_addr,
                    error = %e,
                    "client session error"
                );
            }
            debug!(%peer_addr, "client disconnected");
        });
    }
}

/// Handle a single PG wire client session.
async fn handle_client(mut stream: TcpStream, pool: &BackendPool) -> Result<(), PoolerError> {
    handle_startup(&mut stream).await?;
    send_auth_ok(&mut stream).await?;
    send_parameter_statuses(&mut stream).await?;
    send_ready_for_query(&mut stream, b'I').await?;

    handle_query_loop(&mut stream, pool).await
}

/// Read and handle the PG startup message.
///
/// Handles SSL negotiation (always refuses with 'N') and the
/// startup message with protocol version and parameters.
/// Uses a loop to handle SSL re-negotiation without recursion.
async fn handle_startup(stream: &mut TcpStream) -> Result<(), PoolerError> {
    loop {
        let length = read_i32(stream).await?;
        if length < 8 {
            return Err(PoolerError::PgWire("startup message too short".into()));
        }

        let code = read_i32(stream).await?;

        if code == SSL_REQUEST_CODE {
            stream.write_all(b"N").await?;
            continue;
        }

        if code != PG_PROTOCOL_V3 {
            return Err(PoolerError::PgWire(format!(
                "unsupported protocol version: {code}"
            )));
        }

        let remaining = (length - 8) as usize;
        let mut params_buf = vec![0u8; remaining];
        stream.read_exact(&mut params_buf).await?;

        parse_startup_params(&params_buf);

        return Ok(());
    }
}

/// Parse null-terminated key-value pairs from startup message.
fn parse_startup_params(data: &[u8]) {
    let mut offset = 0;
    while offset < data.len() {
        let key_end = data[offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| offset + p);
        let Some(key_end) = key_end else { break };
        if key_end == offset {
            break;
        }
        let key = String::from_utf8_lossy(&data[offset..key_end]);
        offset = key_end + 1;

        let val_end = data[offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| offset + p);
        let Some(val_end) = val_end else { break };
        let value = String::from_utf8_lossy(&data[offset..val_end]);
        offset = val_end + 1;

        debug!(
            key = %key,
            value = %value,
            "startup parameter"
        );
    }
}

/// Send `AuthenticationOk` (R + 8 + 0).
async fn send_auth_ok(stream: &mut TcpStream) -> Result<(), PoolerError> {
    let mut buf = BytesMut::with_capacity(9);
    buf.put_u8(b'R');
    buf.put_i32(8);
    buf.put_i32(0);
    stream.write_all(&buf).await?;
    Ok(())
}

/// Send essential `ParameterStatus` messages that most PG clients
/// require to function correctly.
async fn send_parameter_statuses(stream: &mut TcpStream) -> Result<(), PoolerError> {
    let params = [
        ("server_version", "17.0"),
        ("server_encoding", "UTF8"),
        ("client_encoding", "UTF8"),
        ("DateStyle", "ISO, MDY"),
        ("integer_datetimes", "on"),
        ("standard_conforming_strings", "on"),
    ];

    for (name, value) in params {
        send_parameter_status(stream, name, value).await?;
    }
    Ok(())
}

/// Send a single `ParameterStatus` message.
async fn send_parameter_status(
    stream: &mut TcpStream,
    name: &str,
    value: &str,
) -> Result<(), PoolerError> {
    let payload_len = 4 + name.len() + 1 + value.len() + 1;
    let mut buf = BytesMut::with_capacity(1 + payload_len);
    buf.put_u8(b'S');
    buf.put_i32(payload_len as i32);
    buf.extend_from_slice(name.as_bytes());
    buf.put_u8(0);
    buf.extend_from_slice(value.as_bytes());
    buf.put_u8(0);
    stream.write_all(&buf).await?;
    Ok(())
}

/// Send `ReadyForQuery` with the given transaction status byte.
async fn send_ready_for_query(stream: &mut TcpStream, status: u8) -> Result<(), PoolerError> {
    let mut buf = BytesMut::with_capacity(6);
    buf.put_u8(b'Z');
    buf.put_i32(5);
    buf.put_u8(status);
    stream.write_all(&buf).await?;
    Ok(())
}

/// Main query loop: read PG wire messages and dispatch.
async fn handle_query_loop(stream: &mut TcpStream, pool: &BackendPool) -> Result<(), PoolerError> {
    loop {
        let msg_type = match read_u8(stream).await {
            Ok(b) => b,
            Err(_) => return Ok(()),
        };

        match msg_type {
            b'Q' => handle_simple_query(stream, pool).await?,
            b'X' => {
                debug!("client sent Terminate");
                return Ok(());
            }
            other => {
                let ch = other as char;
                warn!(
                    msg_type = %ch,
                    "unsupported PG message type, \
                     sending error"
                );
                send_error_response(
                    stream,
                    "XX000",
                    &format!(
                        "unsupported message type: \
                         '{}'",
                        other as char
                    ),
                )
                .await?;
                send_ready_for_query(stream, b'I').await?;
            }
        }
    }
}

/// Handle a PG simple query ('Q') message.
async fn handle_simple_query(
    stream: &mut TcpStream,
    pool: &BackendPool,
) -> Result<(), PoolerError> {
    let length = read_i32(stream).await?;
    let payload_len = (length - 4) as usize;
    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload).await?;

    let sql_end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    let sql = String::from_utf8_lossy(&payload[..sql_end]);

    debug!(sql = %sql, "executing simple query");

    match pool.execute_raw_sql(&sql).await {
        Ok(messages) => {
            translate_responses(stream, &messages).await?;
        }
        Err(e) => {
            warn!(error = %e, "backend query failed");
            send_error_response(stream, "XX000", &format!("backend error: {e}")).await?;
        }
    }

    send_ready_for_query(stream, b'I').await
}

/// Translate ra-wire response messages back into PG wire protocol
/// messages and write them to the client stream.
async fn translate_responses(
    stream: &mut TcpStream,
    messages: &[Message],
) -> Result<(), PoolerError> {
    for msg in messages {
        match msg {
            Message::RowBatch(batch) => {
                translate_row_batch(stream, batch).await?;
            }
            Message::RowEnd(end) => {
                send_command_complete(stream, &end.command_tag).await?;
            }
            Message::ErrorResponse(err) => {
                send_error_response(stream, &err.sqlstate, &err.message).await?;
            }
            other => {
                debug!(
                    msg_type = ?other.message_type(),
                    "ignoring unhandled response message"
                );
            }
        }
    }
    Ok(())
}

/// Translate a `RowBatch` into PG wire `RowDescription` (if schema
/// present) and `DataRow` messages.
///
/// Phase 1: sends the raw PG binary data as individual DataRow
/// messages. The schema's result columns are sent as a
/// RowDescription header on the first batch.
async fn translate_row_batch(
    stream: &mut TcpStream,
    batch: &RowBatchPayload,
) -> Result<(), PoolerError> {
    if let Some(schema) = &batch.schema {
        send_row_description(stream, schema).await?;
    }

    match &batch.data {
        RowData::PgBinary(data) => {
            stream.write_all(data).await?;
        }
        RowData::ArrowIpc(_) => {
            warn!("Arrow IPC data not supported in Phase 1");
            send_error_response(stream, "0A000", "Arrow IPC result format not supported").await?;
        }
    }

    Ok(())
}

/// Send a PG wire `RowDescription` message from the result schema.
async fn send_row_description(
    stream: &mut TcpStream,
    schema: &ResultSchema,
) -> Result<(), PoolerError> {
    let mut body = BytesMut::new();
    body.put_i16(schema.columns.len() as i16);

    for col in &schema.columns {
        body.extend_from_slice(col.name.as_bytes());
        body.put_u8(0);
        body.put_i32(0); // table OID
        body.put_i16(0); // column attr number
        body.put_i32(col.type_oid as i32);
        body.put_i16(col.format_len);
        body.put_i32(col.type_mod);
        body.put_i16(0); // format code: text
    }

    let mut msg = BytesMut::with_capacity(1 + 4 + body.len());
    msg.put_u8(b'T');
    msg.put_i32((4 + body.len()) as i32);
    msg.extend_from_slice(&body);
    stream.write_all(&msg).await?;
    Ok(())
}

/// Send a PG wire `CommandComplete` message.
async fn send_command_complete(stream: &mut TcpStream, tag: &str) -> Result<(), PoolerError> {
    let payload_len = 4 + tag.len() + 1;
    let mut buf = BytesMut::with_capacity(1 + payload_len);
    buf.put_u8(b'C');
    buf.put_i32(payload_len as i32);
    buf.extend_from_slice(tag.as_bytes());
    buf.put_u8(0);
    stream.write_all(&buf).await?;
    Ok(())
}

/// Send a PG wire `ErrorResponse` message.
async fn send_error_response(
    stream: &mut TcpStream,
    sqlstate: &str,
    message: &str,
) -> Result<(), PoolerError> {
    let mut body = BytesMut::new();
    // Severity
    body.put_u8(b'S');
    body.extend_from_slice(b"ERROR\0");
    // SQLSTATE
    body.put_u8(b'C');
    body.extend_from_slice(sqlstate.as_bytes());
    body.put_u8(0);
    // Message
    body.put_u8(b'M');
    body.extend_from_slice(message.as_bytes());
    body.put_u8(0);
    // Terminator
    body.put_u8(0);

    let mut msg = BytesMut::with_capacity(1 + 4 + body.len());
    msg.put_u8(b'E');
    msg.put_i32((4 + body.len()) as i32);
    msg.extend_from_slice(&body);
    stream.write_all(&msg).await?;
    Ok(())
}

/// Read a single byte from the stream.
async fn read_u8(stream: &mut TcpStream) -> Result<u8, PoolerError> {
    let mut buf = [0u8; 1];
    stream.read_exact(&mut buf).await?;
    Ok(buf[0])
}

/// Read a big-endian i32 from the stream.
async fn read_i32(stream: &mut TcpStream) -> Result<i32, PoolerError> {
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await?;
    Ok(i32::from_be_bytes(buf))
}
