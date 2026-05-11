//! Extension state management and GUC registration.
//!
//! Defines the GUC variables that control the extension at runtime.

use std::sync::OnceLock;

use pgrx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};

use ra_hardware::HardwareProfile;

/// GUC: master switch (`ra_planner.enabled`).
pub static RA_ENABLED: GucSetting<bool> =
    GucSetting::<bool>::new(true);

/// GUC: log all optimizer decisions for debugging.
pub static RA_LOG_DECISIONS: GucSetting<bool> =
    GucSetting::<bool>::new(false);

/// Hardware profile detected at extension initialization.
///
/// Used to make hardware-aware planning decisions (SSD vs HDD,
/// CPU cores for parallelism, available RAM for work_mem).
static HARDWARE_PROFILE: OnceLock<HardwareProfile> = OnceLock::new();

/// Get the detected hardware profile.
pub fn hardware_profile() -> &'static HardwareProfile {
    HARDWARE_PROFILE.get().expect("Hardware profile not initialized")
}

/// Detect and store hardware profile.
///
/// Called once during extension initialization (_PG_init).
pub fn init_hardware_profile() {
    let profile = ra_hardware::detect_hardware();
    HARDWARE_PROFILE.set(profile).expect("Hardware profile already initialized");
}

/// Register all GUC variables with PostgreSQL.
pub fn register_gucs() {
    GucRegistry::define_bool_guc(
        c"ra_planner.enabled",
        c"Enable or disable the RA planner extension.",
        c"When off, the extension hooks are still registered \
          but immediately defer to the standard planner.",
        &RA_ENABLED,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_bool_guc(
        c"ra_planner.log_decisions",
        c"Log all RA planner decisions.",
        c"Writes optimizer decisions to the PostgreSQL log at \
          LOG level.",
        &RA_LOG_DECISIONS,
        GucContext::Userset,
        GucFlags::default(),
    );
}
