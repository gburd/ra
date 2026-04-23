//! Background worker entry point.
//!
//! Starts a tokio runtime, binds a QUIC endpoint with a
//! self-signed TLS certificate, and accepts connections from
//! the Ra pooler.

use std::net::SocketAddr;
use std::sync::Arc;

use pgrx::bgworkers::BackgroundWorker;

use crate::config::{
    RA_QUIC_ENABLED, RA_QUIC_MAX_CONNECTIONS, RA_QUIC_PORT,
};
use crate::error::QuicWorkerError;
use crate::handler;

/// Main loop for the background worker.
///
/// Called from `ra_quic_main` after pgrx guard setup. Builds
/// a tokio runtime and runs the QUIC accept loop until
/// PostgreSQL signals shutdown.
///
/// # Errors
///
/// Returns `QuicWorkerError` on fatal startup or runtime errors.
pub fn run_worker() -> Result<(), QuicWorkerError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    rt.block_on(async { accept_loop().await })
}

/// Async accept loop: bind QUIC endpoint and serve connections.
async fn accept_loop() -> Result<(), QuicWorkerError> {
    if !RA_QUIC_ENABLED.get() {
        tracing::info!(
            "ra_quic.enabled is false, worker idling"
        );
        // Wait for shutdown signal without consuming resources.
        loop {
            tokio::time::sleep(
                std::time::Duration::from_secs(5),
            )
            .await;
            if BackgroundWorker::sighup_received() {
                if RA_QUIC_ENABLED.get() {
                    break; // Re-enter the main path.
                }
            }
            if BackgroundWorker::sigterm_received() {
                return Ok(());
            }
        }
    }

    let port = RA_QUIC_PORT.get() as u16;
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let max_conns = RA_QUIC_MAX_CONNECTIONS.get() as u32;

    let server_config = build_server_config()?;

    let endpoint = quinn::Endpoint::server(server_config, addr)
        .map_err(QuicWorkerError::EndpointBind)?;

    tracing::info!(
        %addr,
        max_connections = max_conns,
        "QUIC endpoint listening"
    );

    let active_connections = Arc::new(
        std::sync::atomic::AtomicU32::new(0),
    );

    loop {
        // Check for PostgreSQL shutdown signals.
        if BackgroundWorker::sigterm_received() {
            tracing::info!("SIGTERM received, shutting down");
            endpoint.close(0u32.into(), b"shutdown");
            break;
        }

        // Accept with a timeout so we can poll for signals.
        let accept_future = endpoint.accept();
        let incoming = tokio::select! {
            incoming = accept_future => incoming,
            () = tokio::time::sleep(
                std::time::Duration::from_millis(500),
            ) => {
                continue;
            }
        };

        let Some(incoming) = incoming else {
            tracing::info!("endpoint closed");
            break;
        };

        let current = active_connections.load(
            std::sync::atomic::Ordering::Relaxed,
        );
        if current >= max_conns {
            tracing::warn!(
                current,
                max = max_conns,
                "connection limit reached, refusing"
            );
            incoming.refuse();
            continue;
        }

        let counter = Arc::clone(&active_connections);
        counter.fetch_add(
            1,
            std::sync::atomic::Ordering::Relaxed,
        );

        tokio::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    if let Err(e) =
                        handler::handle_connection(conn).await
                    {
                        tracing::warn!(
                            error = %e,
                            "connection handler error"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "incoming connection failed"
                    );
                }
            }
            counter.fetch_sub(
                1,
                std::sync::atomic::Ordering::Relaxed,
            );
        });
    }

    Ok(())
}

/// Build a quinn `ServerConfig` with a self-signed TLS certificate.
///
/// Phase 1 uses rcgen to generate an ephemeral self-signed cert.
/// Production deployments will load certificates from files
/// configured via GUC variables.
fn build_server_config(
) -> Result<quinn::ServerConfig, QuicWorkerError> {
    let cert_params =
        rcgen::CertificateParams::new(vec!["localhost".into()])
            .map_err(QuicWorkerError::CertGeneration)?;

    let key_pair = rcgen::KeyPair::generate()
        .map_err(QuicWorkerError::CertGeneration)?;

    let cert = cert_params
        .self_signed(&key_pair)
        .map_err(QuicWorkerError::CertGeneration)?;

    let cert_der = rustls::pki_types::CertificateDer::from(
        cert.der().to_vec(),
    );
    let key_der =
        rustls::pki_types::PrivateKeyDer::try_from(
            key_pair.serialize_der(),
        )
        .map_err(|e| {
            QuicWorkerError::Tls(rustls::Error::General(
                format!("private key conversion: {e}"),
            ))
        })?;

    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)?;

    tls_config.alpn_protocols = vec![b"ra-quic/1".to_vec()];

    let mut server_config =
        quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(
                tls_config,
            )?,
        ));

    // Tune transport parameters.
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(128u32.into());
    transport.max_concurrent_uni_streams(32u32.into());
    transport.keep_alive_interval(Some(
        std::time::Duration::from_secs(15),
    ));
    server_config
        .transport_config(Arc::new(transport));

    Ok(server_config)
}
