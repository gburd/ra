//! Ra QUIC listener for PostgreSQL backends.
//!
//! Runs as a pgrx background worker. Listens for QUIC connections
//! from the Ra pooler, executes queries via PostgreSQL's SPI, and
//! streams results back using the ra-wire protocol.
//!
//! ## Architecture
//!
//! ```text
//! Ra Pooler --QUIC--> [bgworker] --SPI--> PostgreSQL
//!                     (this crate)
//! ```
//!
//! The background worker starts a tokio runtime, binds a
//! `quinn::Endpoint`, and accepts connections. Each connection
//! performs a handshake, then handles data streams carrying
//! `ExecuteRawSQL` (and later `ExecuteSQL`) messages.

use pgrx::prelude::*;

mod bgworker;
mod config;
mod error;
mod handler;
mod spi_executor;

pgrx::pg_module_magic!();

/// Extension initialization.
///
/// Registers GUC variables and loads the background worker that
/// will listen for QUIC connections.
#[allow(non_snake_case)]
#[pg_guard]
pub extern "C-unwind" fn _PG_init() {
    config::register_gucs();

    BackgroundWorkerBuilder::new("ra_quic_listener")
        .set_function("ra_quic_main")
        .set_library("ra_pg_quic")
        .set_start_time(BgWorkerStartTime::RecoveryFinished)
        .enable_shmem_access(None)
        .load();
}

/// Background worker entry point.
///
/// Called by PostgreSQL when the background worker process starts.
/// Sets up signal handlers and delegates to `bgworker::run_worker`.
///
/// # Safety
///
/// Called from PostgreSQL's background worker infrastructure.
/// Must be `extern "C"` with `#[pg_guard]` for proper signal
/// handling and longjmp protection.
#[pg_guard]
#[no_mangle]
pub extern "C-unwind" fn ra_quic_main(_arg: pg_sys::Datum) {
    BackgroundWorker::attach_signal_handlers(
        SignalWakeFlags::SIGHUP | SignalWakeFlags::SIGTERM,
    );

    BackgroundWorker::connect_worker_to_spi(
        Some(c"postgres"),
        None,
    );

    tracing::info!("ra_quic background worker starting");

    if let Err(e) = bgworker::run_worker() {
        tracing::error!(error = %e, "QUIC worker exited with error");
    }

    tracing::info!("ra_quic background worker stopped");
}

#[cfg(test)]
pub mod pg_test {
    /// Setup function called once before all `pg_test` tests.
    pub fn setup(_options: Vec<&str>) {}

    /// Required by pgrx test harness.
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![
            "shared_preload_libraries = 'ra_pg_quic'",
            "ra_quic.port = 15434",
        ]
    }
}
