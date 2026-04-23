//! QUIC transport layer for connecting to PostgreSQL backends
//! running the ra-pg-quic extension.

use std::net::SocketAddr;
use std::sync::Arc;

use bytes::BytesMut;
use quinn::crypto::rustls::QuicClientConfig;
use ra_wire::capabilities::Capabilities;
use ra_wire::codec::{decode_from_bytes, encode_message};
use ra_wire::frame::{Frame, HEADER_SIZE};
use ra_wire::messages::{HandshakeAckPayload, HandshakePayload, Message};
use tracing::{debug, info};

use crate::error::PoolerError;

/// Phase 1: skip server certificate verification for dev/testing.
/// This accepts any server certificate without validation.
#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Build a quinn `ClientConfig` for QUIC connections to backends.
///
/// Phase 1 uses a permissive TLS verifier that skips server
/// certificate validation (suitable for development only).
///
/// # Errors
///
/// Returns `PoolerError::Config` if the TLS or QUIC configuration
/// cannot be constructed.
pub fn build_client_config() -> Result<quinn::ClientConfig, PoolerError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let crypto = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| PoolerError::Config(format!("failed to set TLS protocol versions: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification(provider)))
        .with_no_client_auth();

    let quic_crypto = QuicClientConfig::try_from(crypto)
        .map_err(|e| PoolerError::Config(format!("failed to create QUIC client config: {e}")))?;

    Ok(quinn::ClientConfig::new(Arc::new(quic_crypto)))
}

/// Create a QUIC endpoint bound to an ephemeral local address.
///
/// # Errors
///
/// Returns `PoolerError::Io` if the socket cannot be bound.
pub fn create_endpoint(client_config: quinn::ClientConfig) -> Result<quinn::Endpoint, PoolerError> {
    let bind_addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], 0));
    let mut endpoint = quinn::Endpoint::client(bind_addr)?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

/// Connect to a backend and perform the ra-wire handshake.
///
/// Returns the established QUIC connection and the negotiated
/// capabilities from the `HandshakeAck`.
///
/// # Errors
///
/// Returns `PoolerError` if the QUIC connection or handshake fails.
pub async fn connect_backend(
    endpoint: &quinn::Endpoint,
    addr: SocketAddr,
) -> Result<(quinn::Connection, Capabilities), PoolerError> {
    debug!(%addr, "connecting to backend via QUIC");

    let connection =
        endpoint
            .connect(addr, "ra-backend")
            .map_err(|e| PoolerError::QuicConnect {
                addr,
                source: e.into(),
            })?;

    let conn = connection.await.map_err(|e| PoolerError::QuicConnect {
        addr,
        source: e.into(),
    })?;

    info!(%addr, "QUIC connection established, performing handshake");

    let capabilities = perform_handshake(&conn).await?;

    info!(
        %addr,
        ?capabilities,
        "backend handshake complete"
    );

    Ok((conn, capabilities))
}

/// Send a Handshake message and wait for `HandshakeAck`.
async fn perform_handshake(conn: &quinn::Connection) -> Result<Capabilities, PoolerError> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| {
        PoolerError::Handshake(format!("failed to open bi-directional stream: {e}"))
    })?;

    let handshake = Message::Handshake(HandshakePayload {
        version: 1,
        capabilities: Capabilities::pooler_defaults(),
        pooler_id: "ra-pooler".into(),
        auth_token: Vec::new(),
    });

    let mut buf = BytesMut::new();
    encode_message(&handshake, 0, false, &mut buf)?;
    send.write_all(&buf)
        .await
        .map_err(|e| PoolerError::Handshake(format!("failed to send handshake: {e}")))?;
    send.finish()
        .map_err(|e| PoolerError::Handshake(format!("failed to finish send stream: {e}")))?;

    let response_bytes = recv
        .read_to_end(64 * 1024)
        .await
        .map_err(|e| PoolerError::Handshake(format!("failed to read handshake ack: {e}")))?;

    let (msg, _request_id) = decode_from_bytes(&response_bytes)?;

    let Message::HandshakeAck(HandshakeAckPayload {
        capabilities,
        pg_version,
        server_id,
        ..
    }) = msg
    else {
        return Err(PoolerError::Handshake(format!(
            "expected HandshakeAck, got {msg:?}"
        )));
    };

    info!(
        %server_id,
        %pg_version,
        "backend identified"
    );

    let negotiated = Capabilities::pooler_defaults().negotiate(capabilities);
    Ok(negotiated)
}

/// Send an `ExecuteRawSql` message over a new bidirectional QUIC
/// stream and collect all response frames.
///
/// Returns the list of response `Message` values (typically
/// `RowBatch` and/or `RowEnd`, or `ErrorResponse`).
///
/// # Errors
///
/// Returns `PoolerError` if the stream or wire protocol fails.
pub async fn execute_raw_sql(
    conn: &quinn::Connection,
    sql: &str,
) -> Result<Vec<Message>, PoolerError> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|e| PoolerError::QuicStream(format!("failed to open stream: {e}")))?;

    let msg = Message::ExecuteRawSql(ra_wire::messages::ExecuteRawSqlPayload {
        sql: sql.to_owned(),
        params: None,
    });

    let mut buf = BytesMut::new();
    encode_message(&msg, 1, false, &mut buf)?;
    send.write_all(&buf)
        .await
        .map_err(|e| PoolerError::QuicStream(format!("failed to write SQL to backend: {e}")))?;
    send.finish()
        .map_err(|e| PoolerError::QuicStream(format!("failed to finish send stream: {e}")))?;

    let response_bytes = recv
        .read_to_end(64 * 1024 * 1024)
        .await
        .map_err(|e| PoolerError::QuicStream(format!("failed to read response: {e}")))?;

    decode_response_frames(&response_bytes)
}

/// Decode a byte buffer containing one or more concatenated
/// ra-wire frames into a vector of `Message` values.
fn decode_response_frames(data: &[u8]) -> Result<Vec<Message>, PoolerError> {
    let mut messages = Vec::new();
    let mut remaining = bytes::Bytes::copy_from_slice(data);

    while remaining.len() >= HEADER_SIZE {
        let Some(frame) = Frame::decode(&mut remaining)? else {
            break;
        };
        let msg = ra_wire::codec::decode_message(&frame)?;
        messages.push(msg);
    }

    Ok(messages)
}
