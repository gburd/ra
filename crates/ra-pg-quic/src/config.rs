//! GUC configuration variables for the QUIC listener.
//!
//! These are registered during `_PG_init` so PostgreSQL
//! can manage them via `SET` / `SHOW` / `ALTER SYSTEM`.

use pgrx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};

/// QUIC listener port (`ra_quic.port`).
pub static RA_QUIC_PORT: GucSetting<i32> =
    GucSetting::<i32>::new(5434);

/// Master switch (`ra_quic.enabled`).
pub static RA_QUIC_ENABLED: GucSetting<bool> =
    GucSetting::<bool>::new(true);

/// Maximum concurrent QUIC connections (`ra_quic.max_connections`).
pub static RA_QUIC_MAX_CONNECTIONS: GucSetting<i32> =
    GucSetting::<i32>::new(64);

/// Maximum rows per `RowBatch` message (`ra_quic.batch_size`).
pub static RA_QUIC_BATCH_SIZE: GucSetting<i32> =
    GucSetting::<i32>::new(1000);

/// Register all GUC variables with PostgreSQL.
pub fn register_gucs() {
    GucRegistry::define_int_guc(
        c"ra_quic.port",
        c"Port for the Ra QUIC listener.",
        c"The UDP port the background worker binds to \
          for incoming QUIC connections from the Ra pooler.",
        &RA_QUIC_PORT,
        1024,
        65535,
        GucContext::Postmaster,
        GucFlags::default(),
    );

    GucRegistry::define_bool_guc(
        c"ra_quic.enabled",
        c"Enable or disable the Ra QUIC listener.",
        c"When off, the background worker starts but \
          does not accept connections.",
        &RA_QUIC_ENABLED,
        GucContext::Sighup,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        c"ra_quic.max_connections",
        c"Maximum concurrent QUIC connections.",
        c"Limits the number of simultaneous connections \
          from pooler instances.",
        &RA_QUIC_MAX_CONNECTIONS,
        1,
        1024,
        GucContext::Sighup,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        c"ra_quic.batch_size",
        c"Maximum rows per RowBatch message.",
        c"Controls how many SPI result rows are packed \
          into each RowBatch frame before flushing.",
        &RA_QUIC_BATCH_SIZE,
        1,
        100_000,
        GucContext::Userset,
        GucFlags::default(),
    );
}
