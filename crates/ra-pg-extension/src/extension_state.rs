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

/// Active plan-advice string, read from PostgreSQL's own
/// `pg_plan_advice.advice` GUC (the feature's setting) rather than a separate
/// Ra-specific GUC. PostgreSQL retains a value set for an as-yet-unloaded
/// custom GUC as a placeholder, so `SET pg_plan_advice.advice = '...'` is
/// honored whether or not the `pg_plan_advice` module itself is loaded.
/// Returns `None` when unset or empty.
#[must_use]
pub fn effective_plan_advice() -> Option<String> {
    // SAFETY: `GetConfigOption(name, missing_ok=true, restrict_privileged=false)`
    // returns NULL for an undefined GUC, otherwise a pointer into GUC-owned
    // memory which we copy out immediately.
    let ptr = unsafe {
        pgrx::pg_sys::GetConfigOption(c"pg_plan_advice.advice".as_ptr(), true, false)
    };
    if ptr.is_null() {
        return None;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// GUC: always include "Supplied Plan Advice" in EXPLAIN output
/// when advice was supplied (`ra_planner.always_explain_supplied_advice`).
///
/// Mirrors `pg_plan_advice.always_explain_supplied_advice`.
/// Default true. When the EXPLAIN-extension hook is wired up
/// (currently a TODO; pgrx 0.17 doesn't expose
/// `RegisterExtensionExplainOption`), this controls whether
/// supplied advice is rendered alongside the plan.
pub static RA_ALWAYS_EXPLAIN_SUPPLIED_ADVICE: GucSetting<bool> =
    GucSetting::<bool>::new(true);

/// GUC: always store advice details on the produced plan
/// (`ra_planner.always_store_advice_details`).
///
/// Mirrors `pg_plan_advice.always_store_advice_details`. Default
/// false. When true, generated advice is computed and stored on
/// the produced plan even when no EXPLAIN is in flight, so that
/// `EXPLAIN EXECUTE` of a prepared statement can later display
/// it.
pub static RA_ALWAYS_STORE_ADVICE_DETAILS: GucSetting<bool> =
    GucSetting::<bool>::new(false);

/// GUC: warn when supplied advice does not apply cleanly
/// (`ra_planner.plan_advice_feedback_warnings`).
///
/// Mirrors `pg_plan_advice.feedback_warnings`. Default false.
/// When true, the planner emits a `WARNING` for each supplied
/// advice item that ends up flagged `FAILED` or `CONFLICTING`.
pub static RA_PLAN_ADVICE_FEEDBACK_WARNINGS: GucSetting<bool> =
    GucSetting::<bool>::new(false);

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

/// Host-calibrated cost model derived from the detected hardware profile.
///
/// Provides per-operation I/O and CPU costs measured/detected for this host
/// instead of hardcoded planner constants. Falls back to the reference
/// machine if hardware detection has not run (e.g. outside a backend).
pub fn calibrated_cost_model() -> &'static ra_hardware::calibration::CalibratedCostModel {
    static CALIBRATED: OnceLock<ra_hardware::calibration::CalibratedCostModel> = OnceLock::new();
    CALIBRATED.get_or_init(|| {
        HARDWARE_PROFILE.get().map_or_else(
            ra_hardware::calibration::CalibratedCostModel::reference,
            ra_hardware::calibration::CalibratedCostModel::from_profile,
        )
    })
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

    GucRegistry::define_bool_guc(
        c"ra_planner.always_explain_supplied_advice",
        c"Show supplied plan advice in EXPLAIN output.",
        c"Mirrors pg_plan_advice.always_explain_supplied_advice. \
          Default true. The full EXPLAIN(PLAN_ADVICE) option \
          requires custom-EXPLAIN-option registration which \
          pgrx 0.17 does not yet expose; for now this GUC \
          controls log-level emission of supplied advice.",
        &RA_ALWAYS_EXPLAIN_SUPPLIED_ADVICE,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_bool_guc(
        c"ra_planner.always_store_advice_details",
        c"Generate plan advice for every plan, not just \
          plans that will be EXPLAINed.",
        c"Mirrors pg_plan_advice.always_store_advice_details. \
          Default false.",
        &RA_ALWAYS_STORE_ADVICE_DETAILS,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_bool_guc(
        c"ra_planner.plan_advice_feedback_warnings",
        c"Emit WARNINGs when supplied advice does not apply cleanly.",
        c"Mirrors pg_plan_advice.feedback_warnings. Default false.",
        &RA_PLAN_ADVICE_FEEDBACK_WARNINGS,
        GucContext::Userset,
        GucFlags::default(),
    );
}
