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

pub mod ab_testing;
mod expr_translator;
mod extension_state;
pub(crate) mod index_resolver;
pub mod feedback_hook;
mod metadata_cache;
pub mod model_safety;
pub mod monitor;
pub(crate) mod parser_hook;
mod plan_advice_explain;
mod catalog_resolver;
mod plan_builder;
mod planner_hook;
pub(crate) mod sort_utils;
pub mod stats_bridge;

#[cfg(any(test, feature = "pg_test"))]
mod integration_tests;

#[cfg(any(test, feature = "pg_test"))]
mod dml_tests;

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
    ab_testing::register_gucs();

    // Load BitNet cost model from disk (if available)
    extension_state::init_cost_model();

    // Pay egg's one-time ~200ms global init now (in the preloaded postmaster,
    // inherited by forked backends) so it never lands on a user query's
    // planning time.
    ra_engine::warmup();

    // Initialize system fingerprint monitor for neural optimization
    monitor::init();

    // Register the EXPLAIN(PLAN_ADVICE) option and chain the
    // per-plan EXPLAIN hook so supplied plan advice is rendered
    // with feedback flags. SAFETY: PG holds the extension-load
    // mutex during _PG_init; mutating the global hook static is
    // race-free here.
    unsafe {
        plan_advice_explain::install_explain_hooks();
    }

    // Register relcache invalidation callback for metadata cache
    register_relcache_callback();

    // Hook into PostgreSQL raw parser (requires patched PG)
    parser_hook::register_parser_hook();

    // Hook into PostgreSQL planner
    planner_hook::register_hooks();

    // Hook into executor end for feedback collection
    feedback_hook::register_hooks();
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
extern "C-unwind" fn ra_relcache_callback(_arg: pgrx::pg_sys::Datum, relid: pgrx::pg_sys::Oid) {
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

    let total_mem = (hw.l3_cache_bytes * 64).max(16 * 1024 * 1024 * 1024);
    let avail_mem = (hw.l3_cache_bytes * 64).max(8 * 1024 * 1024 * 1024);
    TableIterator::once((
        hw.cpu_cores as i32,
        total_mem as f64 / 1_000_000_000.0,
        avail_mem as f64 / 1_000_000_000.0,
        hw.simd_width_bits as i32,
        hw.gpu_available,
    ))
}

// ---------------------------------------------------------------
// A/B testing status function
// ---------------------------------------------------------------

/// Get current A/B test status as JSON.
///
/// SQL: `SELECT ra.ab_test_status();`
///
/// Returns JSON with: control_count, experiment_count, control_mean_ratio,
/// experiment_mean_ratio, p_value, cohens_d, recommendation.
#[pg_extern]
fn ab_test_status() -> Result<pgrx::JsonB, String> {
    let analysis = ab_testing::analyze();

    let recommendation_str = match analysis.recommendation {
        ab_testing::Recommendation::InsufficientData => "insufficient_data",
        ab_testing::Recommendation::NoSignificantDifference => "no_significant_difference",
        ab_testing::Recommendation::Promote => "promote",
        ab_testing::Recommendation::Rollback => "rollback",
    };

    let json = serde_json::json!({
        "control_count": analysis.control_count,
        "experiment_count": analysis.experiment_count,
        "control_mean_ratio": analysis.control_mean_ratio,
        "experiment_mean_ratio": analysis.experiment_mean_ratio,
        "p_value": analysis.p_value,
        "cohens_d": analysis.cohens_d,
        "recommendation": recommendation_str,
    });

    Ok(pgrx::JsonB(json))
}

/// Reset A/B test state (e.g., after model promotion).
///
/// SQL: `SELECT ra.ab_test_reset();`
#[pg_extern]
fn ab_test_reset() {
    ab_testing::reset();
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
