//! Extension state management and GUC registration.
//!
//! Defines the GUC variables that control the extension at runtime,
//! and manages globally-shared state such as the hardware profile
//! and the BitNet cost model.

use std::ffi::CString;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use pgrx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};

use ra_engine::BitNetCostModel;
use ra_hardware::HardwareProfile;

/// Default path for the BitNet cost model file.
const DEFAULT_MODEL_PATH: &str = "models/cost_model.bitnet.json";

/// GUC: master switch (`ra_planner.enabled`).
pub static RA_ENABLED: GucSetting<bool> = GucSetting::<bool>::new(true);

/// GUC: log all optimizer decisions for debugging.
pub static RA_LOG_DECISIONS: GucSetting<bool> = GucSetting::<bool>::new(false);

/// GUC: path to the BitNet cost model file (`ra_planner.model_path`).
pub static RA_MODEL_PATH: GucSetting<Option<CString>> =
    GucSetting::<Option<CString>>::new(Some(c"models/cost_model.bitnet.json"));

/// Hardware profile detected at extension initialization.
///
/// Used to make hardware-aware planning decisions (SSD vs HDD,
/// CPU cores for parallelism, available RAM for work_mem).
static HARDWARE_PROFILE: OnceLock<HardwareProfile> = OnceLock::new();

/// Loaded BitNet cost model, shared across all planner invocations.
static COST_MODEL: OnceLock<Arc<BitNetCostModel>> = OnceLock::new();

/// Get the detected hardware profile.
pub fn hardware_profile() -> &'static HardwareProfile {
    HARDWARE_PROFILE
        .get()
        .expect("Hardware profile not initialized")
}

/// Get the loaded BitNet cost model, if available.
pub fn cost_model() -> Option<&'static Arc<BitNetCostModel>> {
    COST_MODEL.get()
}

/// Detect and store hardware profile.
///
/// Called once during extension initialization (_PG_init).
pub fn init_hardware_profile() {
    let profile = ra_hardware::detect_hardware();
    HARDWARE_PROFILE
        .set(profile)
        .expect("Hardware profile already initialized");
}

/// Return the configured model path.
///
/// Resolution order: GUC value, then `RA_MODEL_PATH` env var,
/// then the compiled-in default.
pub fn model_path() -> String {
    if let Some(guc_val) = RA_MODEL_PATH.get() {
        if let Ok(s) = guc_val.into_string() {
            if !s.is_empty() {
                return s;
            }
        }
    }
    std::env::var("RA_MODEL_PATH")
        .unwrap_or_else(|_| DEFAULT_MODEL_PATH.to_owned())
}

/// Attempt to load the BitNet cost model from disk.
///
/// Returns `Some(model)` on success, `None` if the file is missing or
/// fails to parse (with appropriate log messages).
pub fn load_cost_model() -> Option<Arc<BitNetCostModel>> {
    let path_str = model_path();
    let path = Path::new(&path_str);

    if !path.exists() {
        tracing::debug!(
            path = %path.display(),
            "No cost model file found, using traditional costing"
        );
        return None;
    }

    match BitNetCostModel::load_from_file(&path_str) {
        Ok(model) => {
            tracing::info!(
                samples_trained = model.samples_trained,
                path = %path.display(),
                "Loaded BitNet cost model"
            );
            Some(Arc::new(model))
        }
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "Failed to load BitNet cost model, falling back to \
                 traditional costing"
            );
            None
        }
    }
}

/// Load the cost model and store it in the global `OnceLock`.
///
/// Called once during extension initialization (_PG_init).
/// Safe to call multiple times; subsequent calls are no-ops.
pub fn init_cost_model() {
    if let Some(model) = load_cost_model() {
        let _ = COST_MODEL.set(model);
    }
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

    GucRegistry::define_string_guc(
        c"ra_planner.model_path",
        c"Path to the BitNet cost model file.",
        c"Supports absolute or relative paths. Falls back to \
          the RA_MODEL_PATH environment variable if unset.",
        &RA_MODEL_PATH,
        GucContext::Postmaster,
        GucFlags::default(),
    );
}
