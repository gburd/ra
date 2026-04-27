use std::collections::HashMap;
use std::hash::BuildHasher;

use egg::Id;
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_stats::accuracy::Staleness;

use crate::analysis::RelAnalysis;
use crate::cost::IntegratedCostFn;
use crate::egraph::{EGraphError, RelLang};

use super::convert::rec_expr_to_rel_expr;
use super::cost::RelCostFn;

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
