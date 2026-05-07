use std::collections::HashMap;
use std::hash::BuildHasher;

use egg::Id;
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_stats::accuracy::Staleness;

use crate::analysis::RelAnalysis;
use crate::cost::IntegratedCostFn;
use crate::cost_model::fast_model::FastCostModel;
use crate::egraph::{EGraphError, RelLang};
use crate::state::SystemFingerprint;

use super::convert::rec_expr_to_rel_expr;
use super::cost::RelCostFn;
use super::hybrid_cost::HybridCostFn;
use super::neural_cost::NeuralPlanScorer;
use super::plan_variants::{generate_variants, select_best_by_neural};

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
) -> Result<RelExpr, EGraphError> {
    if table_stats.is_empty() {
        let cost_fn = RelCostFn::new(hardware.clone());
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        rec_expr_to_rel_expr(&best_expr)
    } else {
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

        // IntegratedCostFn::new wraps these in Arc internally, so subsequent
        // clones of IntegratedCostFn are cheap (just Arc reference count increments)
        let cost_fn = IntegratedCostFn::new(hardware.clone(), stats, staleness_map);
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        rec_expr_to_rel_expr(&best_expr)
    }
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
    rec_expr_to_rel_expr(&best_expr)
}

/// Extract the best plan using multi-candidate neural re-ranking.
///
/// # Algorithm
///
/// 1. **E-graph extraction** — `IntegratedCostFn` selects the single best plan
///    (identical to [`extract_best`]).
///
/// 2. **Variant generation** — up to 3 structural variants are created by
///    [`generate_variants`] (original + join-swapped + join-order-reversed).
///
/// 3. **Neural selection** — each variant is scored by `scorer`.  When the
///    model confidence exceeds [`MIN_CONFIDENCE_FOR_VARIANT_SELECTION`] (0.3),
///    the variant with the lowest neural cost is returned.  Otherwise the
///    original plan from step 1 is returned unchanged (safe fallback).
///
/// This upgrades the previous "monitoring only" approach: when the neural
/// model has seen enough training data (≥1500 samples for 0.3 confidence),
/// it actively influences plan selection by preferring cheaper alternatives.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted back to
/// a [`RelExpr`].
pub fn extract_best_with_neural<S: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    hardware: &ra_hardware::HardwareProfile,
    scorer: &NeuralPlanScorer,
) -> Result<RelExpr, EGraphError> {
    // Phase 1: standard e-graph extraction
    let base_plan = extract_best(egraph, root, table_stats, hardware)?;

    // Phase 2: generate structural variants
    let candidates = generate_variants(&base_plan);
    if candidates.len() <= 1 {
        // No join variants generated; log and return base
        let (neural_cost, confidence) = scorer.score(&base_plan);
        tracing::debug!(neural_cost, confidence, "neural plan score (no variants)");
        return Ok(base_plan);
    }

    // Phase 3: neural selection
    let (best_idx, best_neural_cost) = select_best_by_neural(&candidates, scorer);
    let best = &candidates[best_idx];

    tracing::debug!(
        source = best.source,
        neural_cost = best_neural_cost,
        n_variants = candidates.len(),
        "neural plan selection"
    );

    Ok(best.plan.clone())
}

/// Extract the lowest-cost plan using the hybrid neural/traditional cost function.
///
/// This is the primary extraction path for the full neural pipeline. It replaces
/// both `extract_best` and `extract_best_with_neural` by integrating the neural
/// model directly into the egg cost function (rather than as a post-hoc re-ranker).
///
/// # Blend Behavior
///
/// - When `fast_model` is untrained (0 samples), blend_alpha = 0.0 and this
///   produces identical results to `extract_best_with_staleness`.
/// - As the model trains, alpha increases toward 0.9, blending neural predictions
///   with traditional cost estimates at every node in the e-graph.
/// - The blend never reaches 1.0 — traditional cost always contributes at least 10%.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted back to a [`RelExpr`].
pub fn extract_best_hybrid<S: BuildHasher, S2: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    staleness_map: &HashMap<String, Staleness, S2>,
    hardware: &ra_hardware::HardwareProfile,
    fast_model: &FastCostModel,
    fingerprint: &SystemFingerprint,
) -> Result<RelExpr, EGraphError> {
    let cost_fn = HybridCostFn::new(
        hardware.clone(),
        table_stats
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        staleness_map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        fast_model,
        fingerprint,
    );

    tracing::debug!(
        blend_alpha = cost_fn.blend_alpha(),
        "hybrid extraction with neural blend"
    );

    let extractor = egg::Extractor::new(egraph, cost_fn);
    let (_, best_expr) = extractor.find_best(root);
    rec_expr_to_rel_expr(&best_expr)
}
