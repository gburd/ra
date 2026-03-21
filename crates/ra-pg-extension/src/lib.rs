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
/// Registers GUC variables, detects hardware, and registers planner hooks.
#[allow(non_snake_case)]
#[pg_guard]
pub extern "C-unwind" fn _PG_init() {
    // Detect hardware capabilities for adaptive planning
    extension_state::init_hardware_profile();

    // Register configuration variables
    extension_state::register_gucs();

    // Hook into PostgreSQL planner
    planner_hook::register_hooks();
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
