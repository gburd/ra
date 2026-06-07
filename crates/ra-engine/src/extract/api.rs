use std::collections::HashMap;
use std::hash::BuildHasher;

use egg::Id;
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_stats::accuracy::Staleness;

use crate::analysis::RelAnalysis;
use crate::cost::IntegratedCostFn;
use crate::cost_model::BitNetCostModel;
use crate::egraph::{EGraphError, RelLang};
use crate::state::SystemFingerprint;

use super::convert::rec_expr_to_rel_expr;
use super::cost::RelCostFn;
use super::hybrid_cost::HybridCostFn;

use crate::plan_advice_physical::{physical_choices_from_recexpr, PhysicalChoices};

thread_local! {
    /// Per-query side-channel carrying the cost-driven physical-join choices the
    /// extractor made (RFC 0090 Phase 3 chunk 4). `from_rec` collapses physical
    /// join variants back to a logical `RelExpr::Join`, so the chosen method is
    /// stashed here for the caller (planner_hook) to route to the plan-builder
    /// via the sidecar. PG plans a query per backend thread, so a thread-local
    /// is a safe transient channel; callers clear it before optimizing and take
    /// it after (an empty value means a fast path skipped extraction).
    static LAST_PHYSICAL_CHOICES: std::cell::RefCell<PhysicalChoices> =
        std::cell::RefCell::new(PhysicalChoices::new());
}

/// Record the physical choices derived from a just-extracted plan.
fn record_physical_choices(best: &egg::RecExpr<RelLang>) {
    let choices = physical_choices_from_recexpr(best);
    LAST_PHYSICAL_CHOICES.with(|c| *c.borrow_mut() = choices);
}

/// Take (and clear) the physical choices from the last extraction on this
/// thread. Returns empty when the last `optimize` took a non-e-graph fast path.
#[must_use]
pub fn take_last_physical_choices() -> PhysicalChoices {
    LAST_PHYSICAL_CHOICES.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// Clear the physical-choices side-channel. Call before `optimize` so a fast
/// path that skips extraction doesn't surface a previous query's choices.
pub fn clear_last_physical_choices() {
    LAST_PHYSICAL_CHOICES.with(|c| *c.borrow_mut() = PhysicalChoices::new());
}

/// Extract the lowest-cost plan from the e-graph.
///
/// Uses both the hardware profile and table statistics to compute
/// costs. When table statistics are available, staleness adjustments
/// inflate row count estimates to bias toward robust plans.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted
/// back to a [`RelExpr`].
pub fn extract_best<S: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    hardware: &ra_hardware::HardwareProfile,
    live: crate::cost::LiveConditions,
    page_size_bytes: Option<f64>,
) -> Result<RelExpr, EGraphError> {
    if table_stats.is_empty() {
        let cost_fn = RelCostFn::new(hardware.clone());
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        record_physical_choices(&best_expr);
        rec_expr_to_rel_expr(&best_expr)
    } else {
        // Pre-resolve: scan the e-graph to build a mapping from
        // canonical symbol Ids → row counts. This allows the cost
        // function to access per-table statistics without needing
        // the e-graph during extraction.
        let id_row_counts = resolve_table_row_counts(egraph, table_stats);

        // Clone once to create owned HashMap (unavoidable)
        let stats: HashMap<String, Statistics> = table_stats
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Create staleness map (all Fresh by default)
        let staleness_map: HashMap<String, Staleness> = stats
            .keys()
            .map(|k| (k.clone(), Staleness::Fresh))
            .collect();

        let mut cost_fn =
            IntegratedCostFn::with_id_row_counts(hardware.clone(), stats, staleness_map, id_row_counts)
                .with_live_conditions(live);
        if let Some(ps) = page_size_bytes {
            cost_fn = cost_fn.with_page_size_bytes(ps);
        }
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        record_physical_choices(&best_expr);
        rec_expr_to_rel_expr(&best_expr)
    }
}

/// Pre-resolve table symbol IDs to row counts by scanning the e-graph.
///
/// For each e-class containing a `Symbol` node whose name matches a
/// table in `table_stats`, records the canonical Id → `row_count` mapping.
/// This is O(n) in the number of e-classes (typically small) and runs
/// once before extraction.
fn resolve_table_row_counts<S: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    table_stats: &HashMap<String, Statistics, S>,
) -> HashMap<Id, f64> {
    let mut id_row_counts = HashMap::new();
    for class in egraph.classes() {
        for node in &class.nodes {
            if let RelLang::Symbol(s) = node {
                let name = s.to_string();
                if let Some(stats) = table_stats.get(&name) {
                    let canonical = egraph.find(class.id);
                    id_row_counts.insert(canonical, stats.row_count);
                }
            }
        }
    }
    id_row_counts
}

/// Extract the lowest-cost plan using staleness-aware statistics.
///
/// Unlike [`extract_best`], this function accepts per-table staleness
/// information, allowing the cost function to inflate estimates for
/// tables with stale statistics.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted
/// back to a [`RelExpr`].
pub fn extract_best_with_staleness<S: BuildHasher, S2: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    staleness_map: &HashMap<String, Staleness, S2>,
    hardware: &ra_hardware::HardwareProfile,
) -> Result<RelExpr, EGraphError> {
    let cost_fn = IntegratedCostFn::new(
        hardware.clone(),
        table_stats
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        staleness_map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
    );
    let extractor = egg::Extractor::new(egraph, cost_fn);
    let (_, best_expr) = extractor.find_best(root);
    record_physical_choices(&best_expr);
    rec_expr_to_rel_expr(&best_expr)
}

/// Extract the lowest-cost plan using cardinality-aware costing.
///
/// Uses ML-based cardinality estimation to scale operator costs
/// based on estimated intermediate result sizes. This produces more
/// accurate cost estimates than pure operator-based costing.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted
/// back to a [`RelExpr`].
#[cfg(feature = "ml")]
pub fn extract_best_with_cardinality<S: BuildHasher, S2: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    staleness_map: &HashMap<String, Staleness, S2>,
    hardware: &ra_hardware::HardwareProfile,
) -> Result<RelExpr, EGraphError> {
    // Build a map from e-class Id -> symbol string so the cost
    // function can resolve table names from Scan child Ids.
    let symbol_map: HashMap<Id, String> = egraph
        .classes()
        .filter_map(|class| {
            for node in &class.nodes {
                if let RelLang::Symbol(s) = node {
                    return Some((class.id, s.to_string()));
                }
            }
            None
        })
        .collect();

    let cost_fn = crate::cardinality_cost::CardinalityAwareCostFn::new(
        hardware.clone(),
        table_stats
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        staleness_map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
    )
    .with_symbol_map(symbol_map);
    let extractor = egg::Extractor::new(egraph, cost_fn);
    let (cost, best_expr) = extractor.find_best(root);
    tracing::debug!("Extracted plan with cardinality-aware cost: {}", cost);
    record_physical_choices(&best_expr);
    rec_expr_to_rel_expr(&best_expr)
}

/// Extract the lowest-cost plan using the hybrid neural/traditional cost function.
///
/// This is the primary extraction path for the full neural pipeline. It replaces
/// both `extract_best` and `extract_best_with_neural` by integrating the neural
/// model directly into the egg cost function (rather than as a post-hoc re-ranker).
///
/// # Blend Behavior
///
/// - When `fast_model` is untrained (0 samples), `blend_alpha` = 0.0 and this
///   produces identical results to `extract_best_with_staleness`.
/// - As the model trains, alpha increases toward 0.9, blending neural predictions
///   with traditional cost estimates at every node in the e-graph.
/// - The blend never reaches 1.0 — traditional cost always contributes at least 10%.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted back to a [`RelExpr`].
pub fn extract_best_bitnet<S: BuildHasher, S2: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    staleness_map: &HashMap<String, Staleness, S2>,
    hardware: &ra_hardware::HardwareProfile,
    model: &BitNetCostModel,
    fingerprint: &SystemFingerprint,
) -> Result<RelExpr, EGraphError> {
    let cost_fn = HybridCostFn::new(
        hardware.clone(),
        table_stats.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        staleness_map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        model,
        fingerprint,
    );

    tracing::debug!(
        blend_alpha = cost_fn.blend_alpha(),
        "hybrid extraction with neural blend"
    );

    let extractor = egg::Extractor::new(egraph, cost_fn);
    let (_, best_expr) = extractor.find_best(root);
    record_physical_choices(&best_expr);
    rec_expr_to_rel_expr(&best_expr)
}
