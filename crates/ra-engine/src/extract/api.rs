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
    // Always use the cardinality-aware cost function (IntegratedCostFn).
    // Even without per-table statistics, the default selectivity estimates
    // allow correct pushdown decisions.
    {
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
pub(crate) fn resolve_table_row_counts<S: BuildHasher>(
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

#[cfg(test)]
mod model_variation_tests {
    use super::RelAnalysis;
    use crate::cost_model::BitNetCostModel;
    use crate::egraph::to_rec_expr;
    use crate::egraph::RelLang;
    use crate::extract::HybridCostFn;
    use crate::rewrite::all_rules;
    use crate::state::SystemFingerprint;
    use ra_core::statistics::Statistics;
    use std::collections::HashMap;

    /// Build a high-confidence fingerprint so the neural blend is active
    /// (`blend_alpha` rises with `model_samples_trained` and falls with system
    /// pressure / staleness). Without this the blend is 0 and the model is
    /// deliberately shape-inert.
    fn trusted_fingerprint() -> SystemFingerprint {
        let mut fp = SystemFingerprint::default();
        fp.model_samples_trained = 100_000;
        fp.memory_pressure = 0.0;
        fp.io_saturation = 0.0;
        fp.cpu_load_fraction = 0.0;
        fp.avg_staleness = 0.0;
        fp.stats_coverage = 1.0;
        fp
    }

    /// A second well-known model: an all-zero-weight model (`new_zeros`). Same
    /// architecture, different weights — the "preserve a few models with
    /// different weights" scenario. A zero-weight model emits a constant
    /// multiplier (no per-plan discrimination); the trained model varies per
    /// plan, so their cost evaluations diverge.
    fn saturate(
        sql: &str,
    ) -> (egg::EGraph<RelLang, RelAnalysis>, egg::Id) {
        let expr = ra_parser::sql_to_relexpr(sql).expect("parse");
        let rec = to_rec_expr(&expr).expect("to_rec");
        let mut egraph: egg::EGraph<RelLang, RelAnalysis> = egg::EGraph::default();
        let root = egraph.add_expr(&rec);
        let runner = egg::Runner::default()
            .with_egraph(egraph)
            .with_node_limit(500)
            .with_iter_limit(2)
            .with_time_limit(std::time::Duration::from_secs(2))
            .run(&all_rules());
        let root = runner.egraph.find(root);
        (runner.egraph, root)
    }

    fn extract_cost(
        egraph: &egg::EGraph<RelLang, RelAnalysis>,
        root: egg::Id,
        model: &BitNetCostModel,
        fp: &SystemFingerprint,
    ) -> (f64, String) {
        let cost_fn = HybridCostFn::new(
            ra_hardware::HardwareProfile::cpu_only(),
            HashMap::<String, Statistics>::new(),
            HashMap::new(),
            model,
            fp,
        );
        assert!(
            cost_fn.blend_alpha() > 0.001,
            "neural blend must be active for this test (alpha={})",
            cost_fn.blend_alpha()
        );
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (cost, expr) = extractor.find_best(root);
        (cost.total_cost, format!("{expr}"))
    }

    /// Different model weights must produce a different cost evaluation (and may
    /// therefore select a different plan) once the neural blend is active. This
    /// is the guard that the BitNet model genuinely influences planning and that
    /// swapping model snapshots is observable — the prerequisite for trusting
    /// (and freezing, via `ra_planner.online_learning`) a specific model.
    #[test]
    fn different_model_weights_change_plan_cost() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest}/../../models/cost_model.bitnet.json");
        let trained = BitNetCostModel::load_from_file(&path).expect("load trained model");
        let zeros = BitNetCostModel::new_zeros();
        let fp = trusted_fingerprint();

        // A 3-table join: the e-graph holds several equivalent join orders,
        // so the cost model's scoring decides which is cheapest.
        let (egraph, root) = saturate(
            "SELECT a FROM t1 JOIN t2 ON t1.x = t2.x \
             JOIN t3 ON t2.y = t3.y",
        );

        let (c_trained, _) = extract_cost(&egraph, root, &trained, &fp);
        let (c_zeros, _) = extract_cost(&egraph, root, &zeros, &fp);

        // Two different weight sets must not collapse to one cost; the model is
        // not inert when the blend is active.
        assert!(
            (c_trained - c_zeros).abs() > f64::EPSILON,
            "model weights had no effect on cost: trained={c_trained}, zeros={c_zeros}"
        );
    }

    /// The same fixed model must always score a plan identically (no run-to-run
    /// drift in the cost function itself), so a frozen model yields reproducible
    /// planning.
    #[test]
    fn same_model_scores_identically_across_runs() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let path = format!("{manifest}/../../models/cost_model.bitnet.json");
        let trained = BitNetCostModel::load_from_file(&path).expect("load trained model");
        let fp = trusted_fingerprint();
        let (egraph, root) = saturate(
            "SELECT a FROM t1 JOIN t2 ON t1.x = t2.x",
        );
        let (baseline, baseline_plan) = extract_cost(&egraph, root, &trained, &fp);
        for _ in 0..8 {
            let (c, plan) = extract_cost(&egraph, root, &trained, &fp);
            assert!((c - baseline).abs() < f64::EPSILON, "cost drifted: {c} vs {baseline}");
            assert_eq!(plan, baseline_plan, "plan drifted for a fixed model");
        }
    }
}
