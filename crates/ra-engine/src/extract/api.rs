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

/// Record the physical choices derived from a just-extracted plan. `egraph`
/// supplies the post-saturation node set so the scan-method *suppression* can be
/// captured: a table for which the lowering rule introduced an
/// `index-scan-choice` (index-eligible) but whose sequential `Filter(Scan)` the
/// extractor kept means the cost model judged a sequential scan cheaper (e.g. a
/// cold/contended host) — recorded as `ScanStrategy::Seq` so plan-builder
/// suppresses its index-scan peephole. Tables with no recorded choice keep
/// plan-builder's default behaviour (no regression).
fn record_physical_choices(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    best: &egg::RecExpr<RelLang>,
) {
    use crate::plan_advice_physical::ScanStrategy;
    let mut choices = physical_choices_from_recexpr(best);

    // Tables the lowering rule made index-eligible (an index-scan-choice exists
    // somewhere in the e-graph).
    let mut eligible: std::collections::HashSet<String> = std::collections::HashSet::new();
    for class in egraph.classes() {
        for node in &class.nodes {
            if let RelLang::IndexScanChoice([_, table_id]) = node {
                if let Some(t) = symbol_text(egraph, *table_id) {
                    eligible.insert(t);
                }
            }
        }
    }
    // Tables whose scan the extractor actually realised as an index-scan-choice.
    let nodes = best.as_ref();
    let mut chosen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for node in nodes {
        if let RelLang::IndexScanChoice([_, table_id]) = node {
            if let RelLang::Symbol(s) = &nodes[usize::from(*table_id)] {
                chosen.insert(s.to_string());
            }
        }
    }
    // Eligible but not chosen => the extractor preferred the sequential scan.
    for table in eligible.difference(&chosen) {
        choices.set_scan(table.clone(), ScanStrategy::Seq);
    }
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
        record_physical_choices(egraph, &best_expr);
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
                .with_id_selectivity(resolve_index_selectivity(egraph, table_stats))
                .with_live_conditions(live);
        if let Some(ps) = page_size_bytes {
            cost_fn = cost_fn.with_page_size_bytes(ps);
        }
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        record_physical_choices(egraph, &best_expr);
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

/// Floor for estimated selectivity, so a huge NDV never yields a zero-cost
/// index scan.
const MIN_SELECTIVITY: f64 = 1.0e-6;

/// Extract a column name compared by equality within `cond` (depth-limited),
/// for index-scan selectivity estimation. Returns the first column found on
/// either side of an `eq`, descending through `and`/`or`.
fn eq_column_in(egraph: &egg::EGraph<RelLang, RelAnalysis>, id: Id, depth: u32) -> Option<String> {
    if depth == 0 {
        return None;
    }
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Eq([l, r]) => {
                if let Some(c) = col_name_of(egraph, *l).or_else(|| col_name_of(egraph, *r)) {
                    return Some(c);
                }
            }
            RelLang::And([l, r]) | RelLang::Or([l, r]) => {
                if let Some(c) = eq_column_in(egraph, *l, depth - 1)
                    .or_else(|| eq_column_in(egraph, *r, depth - 1))
                {
                    return Some(c);
                }
            }
            _ => {}
        }
    }
    None
}

/// The column name referenced by a `col`/`qcol` node in `id`'s e-class.
fn col_name_of(egraph: &egg::EGraph<RelLang, RelAnalysis>, id: Id) -> Option<String> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        let name_id = match node {
            RelLang::Col([n]) | RelLang::QCol([_, n]) => *n,
            _ => continue,
        };
        if let Some(name) = symbol_text(egraph, name_id) {
            return Some(name);
        }
    }
    None
}

/// The string of a `Symbol` leaf in `id`'s e-class.
fn symbol_text(egraph: &egg::EGraph<RelLang, RelAnalysis>, id: Id) -> Option<String> {
    let canonical = egraph.find(id);
    egraph[canonical].nodes.iter().find_map(|node| match node {
        RelLang::Symbol(s) => Some(s.to_string()),
        _ => None,
    })
}

/// Estimate the selectivity of an index-scan predicate from column statistics:
/// for an equality on a column, `(1 - null_fraction) / distinct_count` (the
/// `PostgreSQL` equality estimate). Falls back to the moderate default when the
/// predicate is not a recognised single-column equality or the column has no
/// statistics — so an un-estimated predicate never spuriously favours the index.
fn estimate_index_selectivity(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    cond_id: Id,
    stats: &Statistics,
) -> f64 {
    if let Some(col) = eq_column_in(egraph, cond_id, 4) {
        if let Some(cs) = stats.columns.get(&col) {
            let ndv = cs.distinct_count.max(1.0);
            return ((1.0 - cs.null_fraction) / ndv).clamp(MIN_SELECTIVITY, 1.0);
        }
    }
    crate::cost::DEFAULT_SELECTIVITY
}

/// Pre-extraction pass (RFC 0091 B2): resolve each `index-scan-choice` node's
/// predicate selectivity from column statistics, keyed by the canonical `cond`
/// child Id (which the cost function reads). Built after saturation, when the
/// lowering rule's `index-scan-choice` nodes exist.
fn resolve_index_selectivity<S: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    table_stats: &HashMap<String, Statistics, S>,
) -> HashMap<Id, f64> {
    let mut out = HashMap::new();
    for class in egraph.classes() {
        for node in &class.nodes {
            if let RelLang::IndexScanChoice([cond_id, table_id]) = node {
                let Some(table) = symbol_text(egraph, *table_id) else {
                    continue;
                };
                if let Some(stats) = table_stats.get(&table) {
                    let sel = estimate_index_selectivity(egraph, *cond_id, stats);
                    out.insert(egraph.find(*cond_id), sel);
                }
            }
        }
    }
    out
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
    record_physical_choices(egraph, &best_expr);
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
    tracing::debug!("Extracted plan with cardinality-aware cost: {:?}", cost);
    record_physical_choices(egraph, &best_expr);
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
    record_physical_choices(egraph, &best_expr);
    rec_expr_to_rel_expr(&best_expr)
}

#[cfg(test)]
mod selectivity_tests {
    use super::{resolve_index_selectivity, RelAnalysis};
    use crate::egraph::RelLang;
    use ra_core::statistics::{ColumnStats, Statistics};
    use std::collections::HashMap;

    /// RFC 0091 B2: index-scan selectivity is the per-column equality estimate
    /// `(1 - null_fraction) / distinct_count`, resolved from column statistics,
    /// keyed by the `index-scan-choice` cond Id.
    #[test]
    fn resolve_index_selectivity_uses_column_ndv() {
        let mut eg: egg::EGraph<RelLang, RelAnalysis> = egg::EGraph::default();
        let cname = eg.add(RelLang::Symbol("c".into()));
        let col = eg.add(RelLang::Col([cname]));
        let konst = eg.add(RelLang::Symbol("5".into()));
        let eq = eg.add(RelLang::Eq([col, konst]));
        let tsym = eg.add(RelLang::Symbol("t".into()));
        let _isc = eg.add(RelLang::IndexScanChoice([eq, tsym]));
        eg.rebuild();

        let mut stats = Statistics::new(100_000.0);
        stats.columns.insert("c".to_string(), ColumnStats::new(1000.0));
        let mut ts = HashMap::new();
        ts.insert("t".to_string(), stats);

        let sel = resolve_index_selectivity(&eg, &ts);
        let got = sel
            .get(&eg.find(eq))
            .copied()
            .expect("selectivity resolved for the index-scan-choice cond");
        assert!(
            (got - 0.001).abs() < 1e-9,
            "expected 1/NDV ≈ 0.001, got {got}"
        );
    }
}
