//! Extension state management and GUC registration.
//!
//! Defines the GUC variables that control the extension at runtime
//! and manages per-query optimizer state that flows through the
//! planner hooks.

use pgrx::prelude::*;

use ra_core::{Cost, RelExpr, Statistics};

/// GUC: master switch (`ra_planner.enabled`).
pub static RA_ENABLED: GucSetting<bool> = GucSetting::<bool>::new(true);

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

/// Register all GUC variables with PostgreSQL.
pub fn register_gucs() {
    GucRegistry::define_bool_guc(
        "ra_planner.enabled",
        "Enable or disable the RA planner extension.",
        "When off, the extension hooks are still registered \
         but immediately defer to the standard planner.",
        &RA_ENABLED,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_float_guc(
        "ra_planner.min_confidence",
        "Minimum confidence threshold for applying advice.",
        "Advice with confidence below this value is discarded.",
        &RA_MIN_CONFIDENCE,
        0.0,
        1.0,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_bool_guc(
        "ra_planner.log_decisions",
        "Log all RA planner decisions.",
        "Writes optimizer decisions to the PostgreSQL log at \
         LOG level.",
        &RA_LOG_DECISIONS,
        GucContext::Userset,
        GucFlags::default(),
    );

    GucRegistry::define_int_guc(
        "ra_planner.max_relations",
        "Maximum relations before fallback to PG planner.",
        "Queries exceeding this join count skip RA optimization.",
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
