//! `ra_planner` -- PostgreSQL extension that augments the planner
//! with RA optimizer advice.
//!
//! Uses PostgreSQL v19 committed infrastructure:
//! - `planner_hook` to intercept query planning
//! - GUC variables for runtime configuration
//!
//! For pre-v19 servers the extension hooks the existing
//! `planner_hook` and injects advice through cost manipulation.

use pgrx::prelude::*;

mod cost_mapper;
mod extension_state;
mod metadata_cache;
mod pg_constants;
mod plan_converter;
mod planner_hook;
mod query_parser;
mod stats_bridge;

#[cfg(any(test, feature = "pg_test"))]
mod integration_tests;

pgrx::pg_module_magic!();

/// Extension initialization -- called when the shared library is loaded.
///
/// Registers GUC variables, detects hardware, registers planner hooks,
/// and sets up relcache invalidation callbacks.
#[allow(non_snake_case)]
#[pg_guard]
pub extern "C-unwind" fn _PG_init() {
    // Detect hardware capabilities for adaptive planning
    extension_state::init_hardware_profile();

    // Register configuration variables
    extension_state::register_gucs();

    // Register relcache invalidation callback for metadata cache
    register_relcache_callback();

    // Hook into PostgreSQL planner
    planner_hook::register_hooks();
}

/// Register callback for relcache invalidations.
///
/// Called during extension initialization to set up the metadata cache
/// invalidation mechanism. When PostgreSQL invalidates a relation's
/// relcache entry (due to DDL, ANALYZE, etc.), our callback is invoked
/// to mark the cached metadata as stale.
fn register_relcache_callback() {
    unsafe {
        // CacheRegisterRelcacheCallback registers a function to be called
        // whenever a relcache entry is invalidated. The callback receives
        // the relation OID as an argument.
        pgrx::pg_sys::CacheRegisterRelcacheCallback(
            Some(ra_relcache_callback),
            pgrx::pg_sys::Datum::from(0),
        );
    }
}

/// Relcache invalidation callback (C ABI).
///
/// Called by PostgreSQL when a relation's relcache entry is invalidated.
/// Forwards the invalidation to the Rust metadata cache implementation.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process with valid
/// memory context. Called automatically by PostgreSQL's cache invalidation
/// system.
#[pg_guard]
extern "C-unwind" fn ra_relcache_callback(
    _arg: pgrx::pg_sys::Datum,
    relid: pgrx::pg_sys::Oid,
) {
    // Forward to Rust implementation
    metadata_cache::ra_rust_invalidate_table(relid);
}

// ---------------------------------------------------------------
// SQL functions for metadata cache management
// ---------------------------------------------------------------

/// Clear all cached metadata (forces refresh on next query).
///
/// SQL: `SELECT ra.clear_metadata_cache();`
#[pg_extern]
fn clear_metadata_cache() {
    metadata_cache::clear_cache();
}

/// Get metadata cache statistics.
///
/// SQL: `SELECT * FROM ra.metadata_cache_stats();`
#[pg_extern]
fn metadata_cache_stats() -> TableIterator<
    'static,
    (
        name!(entries, i32),
        name!(invalidated, i32),
        name!(hits, i64),
        name!(misses, i64),
        name!(invalidations, i64),
        name!(hit_rate, f64),
    ),
> {
    let stats = metadata_cache::get_cache_stats();

    TableIterator::once(match stats {
        Some(s) => (
            s.entries as i32,
            s.invalidated as i32,
            s.hits as i64,
            s.misses as i64,
            s.invalidations as i64,
            s.hit_rate,
        ),
        None => (0, 0, 0, 0, 0, 0.0),
    })
}

// Integration tests are in integration_tests.rs

#[cfg(test)]
pub mod pg_test {
    /// Setup function called once before all `pg_test` tests.
    pub fn setup(_options: Vec<&str>) {}

    /// Required by pgrx test harness.
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec!["shared_preload_libraries = 'ra_pg_extension'"]
    }
}
