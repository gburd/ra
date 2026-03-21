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
mod stats_bridge;

pgrx::pg_module_magic!();

/// Extension initialization -- called when the shared library is loaded.
///
/// Registers GUC variables and planner hooks.
#[allow(non_snake_case)]
#[pg_guard]
pub extern "C-unwind" fn _PG_init() {
    planner_hook::register_hooks();
    extension_state::register_gucs();
}

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    #[pg_test]
    fn test_extension_loads() {
        // Extension loaded successfully if we reach here.
        assert!(true);
    }
}

#[cfg(test)]
pub mod pg_test {
    /// Setup function called once before all `pg_test` tests.
    pub fn setup(_options: Vec<&str>) {}

    /// Required by pgrx test harness.
    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec!["shared_preload_libraries = 'ra_pg_extension'"]
    }
}
