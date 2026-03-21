//! Extension state management and GUC registration.
//!
//! Defines the GUC variables that control the extension at runtime
//! and manages per-query optimizer state that flows through the
//! planner hooks.

use std::sync::OnceLock;

use pgrx::guc::{GucContext, GucFlags, GucRegistry, GucSetting};

use ra_core::{Cost, RelExpr, Statistics};
use ra_hardware::HardwareProfile;

/// GUC: master switch (`ra_planner.enabled`).
pub static RA_ENABLED: GucSetting<bool> =
    GucSetting::<bool>::new(true);

/// GUC: minimum confidence to apply advice (`ra_planner.min_confidence`).
///
/// Values from 0.0 to 1.0. Only advice whose confidence meets or
/// exceeds this threshold is forwarded to the planner.
pub static RA_MIN_CONFIDENCE: GucSetting<f64> =
    GucSetting::<f64>::new(0.9);

/// GUC: log all optimizer decisions for debugging.
pub static RA_LOG_DECISIONS: GucSetting<bool> =
    GucSetting::<bool>::new(false);

/// GUC: maximum relations before falling back to PG planner.
///
/// Large join graphs can cause the RA optimizer to spend excessive
/// time in e-graph saturation. This limit triggers an early bail-out.
pub static RA_MAX_RELATIONS: GucSetting<i32> =
    GucSetting::<i32>::new(12);

/// Hardware profile detected at extension initialization.
///
/// Used to make hardware-aware planning decisions (SSD vs HDD,
/// CPU cores for parallelism, available RAM for work_mem).
static HARDWARE_PROFILE: OnceLock<HardwareProfile> = OnceLock::new();

/// Get the detected hardware profile.
///
/// Returns the hardware profile detected during extension initialization.
/// This information is used to adjust cost parameters and GUC values
/// based on actual system capabilities.
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

    GucRegistry::define_float_guc(
        c"ra_planner.min_confidence",
        c"Minimum confidence threshold for applying advice.",
        c"Advice with confidence below this value is discarded.",
        &RA_MIN_CONFIDENCE,
        0.0,
        1.0,
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

    GucRegistry::define_int_guc(
        c"ra_planner.max_relations",
        c"Maximum relations before fallback to PG planner.",
        c"Queries exceeding this join count skip RA optimization.",
        &RA_MAX_RELATIONS,
        1,
        100,
        GucContext::Userset,
        GucFlags::default(),
    );
}

/// Per-query state carried between planner hook invocations.
///
/// Allocated in `planner_setup` and freed in `planner_shutdown`.
pub struct RaOptimizerState {
    /// The original SQL query string (owned copy).
    pub query_string: String,

    /// The RA optimized relational expression, if optimization
    /// succeeded and met the confidence threshold.
    pub optimized_plan: Option<RelExpr>,

    /// Cost of the RA-optimized plan.
    pub ra_cost: Option<Cost>,

    /// Cached table statistics gathered for this query.
    pub statistics: Vec<(String, Statistics)>,

    /// Confidence score for the optimization (0.0 to 1.0).
    pub confidence: f64,

    /// Whether the RA plan was actually used.
    pub plan_applied: bool,
}

impl RaOptimizerState {
    /// Create empty state for a new query.
    pub fn new(query_string: String) -> Self {
        Self {
            query_string,
            optimized_plan: None,
            ra_cost: None,
            statistics: Vec::new(),
            confidence: 0.0,
            plan_applied: false,
        }
    }
}
