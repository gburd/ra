use ra_core::algebra::RelExpr;

use crate::provenance::PlanProvenance;
use crate::resource_budget::ResourceUsageReport;

use super::tracking::RuleTrackingResult;

/// Result of a bounded optimization run.
#[derive(Debug)]
pub struct OptimizationResult {
    /// The best plan found.
    pub plan: RelExpr,
    /// Estimated cost of the plan.
    pub cost: f64,
    /// Whether optimization completed fully or was truncated.
    pub status: OptimizationStatus,
    /// Detailed resource usage report.
    pub resource_usage: ResourceUsageReport,
    /// Rules applied during optimization (only populated if tracking enabled).
    /// Zero overhead when None.
    pub applied_rules: Option<crate::rule_registry::RuleSet>,
    /// Detailed rule tracking (only populated if tracking enabled).
    pub rule_tracking: Option<RuleTrackingResult>,
    /// Per-query metadata identifying which inputs produced this
    /// plan (cost-model snapshot, hardware profile, rule set,
    /// route, termination reason). Useful for reproducibility
    /// and debugging "this plan changed overnight" reports.
    /// `None` when the optimization path didn't go through the
    /// e-graph (e.g. `OptRoute::Skip`).
    pub provenance: Option<PlanProvenance>,
}

/// Whether optimization completed within its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationStatus {
    /// All iterations ran and the e-graph was fully explored.
    Complete,
    /// Optimization was cut short by a resource limit.
    Incomplete,
    /// Optimization failed (e.g., no plan could be extracted).
    Failed,
}

/// Statistics about an incremental optimization run.
///
/// Reports how much work the incremental optimizer did compared to
/// what a full reoptimization would require, allowing callers to
/// measure the speedup from differential updates.
#[derive(Debug, Clone)]
pub struct IncrementalStats {
    /// Number of rewrite rules evaluated.
    pub rules_evaluated: usize,
    /// Number of e-graph iterations actually used.
    pub iterations_used: usize,
    /// Maximum iterations configured.
    pub max_iterations: usize,
    /// Number of nodes in the final e-graph.
    pub nodes_in_egraph: usize,
    /// Number of tables whose stats were updated.
    pub tables_updated: usize,
    /// Number of individual deltas processed.
    pub delta_count: usize,
    /// Maximum row count change percentage.
    pub row_change_pct: f64,
    /// Whether full reoptimization was used (delta was too large).
    pub used_full_reoptimization: bool,
    /// Wall-clock time for the incremental optimization.
    pub elapsed: std::time::Duration,
}

impl IncrementalStats {
    /// Estimated speedup factor vs full optimization.
    ///
    /// Based on the ratio of iterations used vs max configured.
    /// Returns 1.0 when full reoptimization was used.
    #[must_use]
    pub fn speedup_factor(&self) -> f64 {
        if self.used_full_reoptimization || self.iterations_used == 0 {
            return 1.0;
        }
        self.max_iterations as f64 / self.iterations_used as f64
    }
}
