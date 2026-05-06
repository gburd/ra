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
mod plan_builder;
mod plan_converter;
mod planner_hook;
mod query_parser;
mod stats_bridge;
mod timeline_capture;

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

// ---------------------------------------------------------------
// Timeline snapshot capture functions
// ---------------------------------------------------------------

/// Capture a fingerprint snapshot from PostgreSQL catalogs.
///
/// SQL: `SELECT ra.capture_snapshot(ARRAY['schema.table1', 'schema.table2']);`
///
/// Returns JSON representation of the captured snapshot.
#[pg_extern]
fn capture_snapshot(
    table_names: Vec<String>,
) -> Result<pgrx::JsonB, String> {
    // Parse "schema.table" strings
    let parsed_names: Vec<(&str, &str)> = table_names
        .iter()
        .filter_map(|name| {
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            if parts.len() == 2 {
                Some((parts[0], parts[1]))
            } else {
                None
            }
        })
        .collect();

    if parsed_names.is_empty() {
        return Err("No valid table names provided (use schema.table format)".to_string());
    }

    let config = timeline_capture::CaptureConfig::default();

    let snapshot = timeline_capture::capture_snapshot_from_catalog(&parsed_names, &config)
        .map_err(|e| e.to_string())?;

    // Convert to JSON
    let json_str = serde_json::to_string(&snapshot)
        .map_err(|e| format!("Failed to serialize snapshot: {e}"))?;

    let json_value: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse JSON: {e}"))?;

    Ok(pgrx::JsonB(json_value))
}

/// Capture a snapshot and save to TOML file.
///
/// SQL: `SELECT ra.capture_snapshot_to_file(
///          ARRAY['public.orders', 'public.customers'],
///          '/tmp/snapshot.toml',
///          'Initial snapshot'
///      );`
#[pg_extern]
fn capture_snapshot_to_file(
    table_names: Vec<String>,
    output_path: String,
    label: Option<String>,
) -> Result<String, String> {
    // Parse "schema.table" strings
    let parsed_names: Vec<(&str, &str)> = table_names
        .iter()
        .filter_map(|name| {
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            if parts.len() == 2 {
                Some((parts[0], parts[1]))
            } else {
                None
            }
        })
        .collect();

    if parsed_names.is_empty() {
        return Err("No valid table names provided (use schema.table format)".to_string());
    }

    let mut config = timeline_capture::CaptureConfig::default();
    config.label = label;

    let snapshot = timeline_capture::capture_snapshot_from_catalog(&parsed_names, &config)
        .map_err(|e| e.to_string())?;

    // Serialize to TOML
    let toml_str = timeline_capture::snapshot_to_toml(&snapshot)
        .map_err(|e| e.to_string())?;

    // Write to file
    std::fs::write(&output_path, toml_str)
        .map_err(|e| format!("Failed to write file: {e}"))?;

    Ok(format!("Snapshot written to {output_path}"))
}

/// Get hardware profile detected by the extension.
///
/// SQL: `SELECT * FROM ra.hardware_profile();`
#[pg_extern]
fn hardware_profile() -> TableIterator<
    'static,
    (
        name!(cpu_cores, i32),
        name!(total_memory_gb, f64),
        name!(available_memory_gb, f64),
        name!(simd_width, i32),
        name!(has_gpu, bool),
    ),
> {
    let hw = extension_state::hardware_profile();

    TableIterator::once((
        hw.cpu_cores as i32,
        hw.total_memory as f64 / 1_000_000_000.0,
        hw.available_memory as f64 / 1_000_000_000.0,
        hw.simd_width as i32,
        hw.has_gpu,
    ))
}

// Integration tests are in integration_tests.rs

#[cfg(test)]
pub mod pg_test {
    /// Setup function called once before all `pg_test` tests.
    pub fn setup(_options: Vec<&str>) {}

    /// Required by pgrx test harness.
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec!["shared_preload_libraries = 'pg_ra_planner'"]
    }
}
