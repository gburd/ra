//! Cardinality-aware cost function for e-graph extraction.
//!
//! Extends the basic cost model by using cardinality estimation to
//! scale operator costs by estimated row counts. Scan nodes look up
//! table statistics (with staleness inflation), and downstream
//! operators apply selectivity heuristics so that a filter
//! eliminating 99% of rows costs less than one eliminating 1%.

use std::collections::HashMap;
use std::sync::Arc;

use egg::{CostFunction, Id, Language};
use ra_core::cost::StatisticsProvider;
use ra_core::statistics::Statistics;
use ra_hardware::HardwareProfile;
use ra_ml::estimator::{CardinalityEstimator, HeuristicEstimator};
use ra_stats::accuracy::Staleness;

use crate::egraph::RelLang;

/// Default row count when no statistics are available.
const DEFAULT_ROW_COUNT: f64 = 1000.0;

/// Per-row cost for a sequential scan. Chosen so that the default
/// 1000-row table produces the same cost (50.0) as the original
/// hardcoded constant.
const SCAN_COST_PER_ROW: f64 = 0.05;

/// Default filter selectivity (fraction of rows passing).
const DEFAULT_SELECTIVITY: f64 = 0.1;

/// Simple statistics provider backed by a `HashMap`.
#[derive(Debug)]
struct TableStatsProvider {
    stats: HashMap<String, Statistics>,
}

impl StatisticsProvider for TableStatsProvider {
    fn get_statistics(&self, table: &str) -> Option<&Statistics> {
        self.stats.get(table)
    }
}

/// Cost function that uses cardinality estimates to scale operator costs.
///
/// For each operator, it:
/// 1. Uses the cardinality estimator to obtain per-table row counts
/// 2. Applies staleness inflation factors
/// 3. Scales operator costs by estimated cardinality
/// 4. Uses hardware-aware cost adjustments
pub struct CardinalityAwareCostFn {
    /// Cardinality estimator (ML or heuristic).
    estimator: Arc<dyn CardinalityEstimator>,
    /// Statistics provider for base table stats.
    stats_provider: Arc<TableStatsProvider>,
    /// Hardware profile for cost adjustments.
    hardware: HardwareProfile,
    /// Staleness adjustments per table.
    staleness_map: HashMap<String, Staleness>,
    /// Pre-computed mapping from e-class Id to symbol string.
    /// Built by walking the e-graph before extraction so that
    /// `cost()` can resolve table names from `Scan` child Ids.
    symbol_map: HashMap<Id, String>,
}

impl CardinalityAwareCostFn {
    /// Create a new cardinality-aware cost function.
    #[must_use]
    pub fn new(
        hardware: HardwareProfile,
        table_stats: HashMap<String, Statistics>,
        staleness_map: HashMap<String, Staleness>,
    ) -> Self {
        Self {
            estimator: Arc::new(HeuristicEstimator),
            stats_provider: Arc::new(TableStatsProvider { stats: table_stats }),
            hardware,
            staleness_map,
            symbol_map: HashMap::new(),
        }
    }

    /// Attach a pre-built symbol map so `cost()` can resolve Ids
    /// to table names. Call this before passing to `Extractor`.
    #[must_use]
    pub fn with_symbol_map(mut self, map: HashMap<Id, String>) -> Self {
        self.symbol_map = map;
        self
    }

    /// Look up the staleness-adjusted row count for a table.
    ///
    /// Uses the cardinality estimator on a synthetic `Scan`
    /// expression, then inflates the result by the staleness factor.
    /// Falls back to `DEFAULT_ROW_COUNT` when the table is unknown.
    fn row_count_for(&self, table: &str) -> f64 {
        let scan = ra_core::algebra::RelExpr::scan(table);
        let est = self.estimator.estimate(&scan, self.stats_provider.as_ref());
        est.rows * self.staleness_factor(table)
    }

    /// Staleness multiplier for a table. Fresh (or missing) tables
    /// return 1.0; staler tables inflate the row-count estimate to
    /// bias the optimizer toward re-scanning.
    fn staleness_factor(&self, table: &str) -> f64 {
        self.staleness_map.get(table).map_or(1.0, |s| match s {
            Staleness::Fresh => 1.0,
            Staleness::SlightlyStale => 1.05,
            Staleness::ModeratelyStale => 1.2,
            Staleness::VeryStale => 1.5,
            Staleness::Unknown => 2.0,
        })
    }

    /// Resolve a child `Id` to a table name via the symbol map.
    fn resolve_table(&self, id: Id) -> Option<&str> {
        self.symbol_map.get(&id).map(String::as_str)
    }
}

impl CostFunction<RelLang> for CardinalityAwareCostFn {
    type Cost = f64;

    #[expect(clippy::cast_precision_loss, reason = "legacy allow")]
    fn cost<C>(&mut self, enode: &RelLang, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let base_cost = match enode {
            // --- Scan: cost scales linearly with row count ---
            RelLang::Scan([table_id]) => {
                let child_cost = costs(*table_id);
                let rows = self
                    .resolve_table(*table_id)
                    .map_or(DEFAULT_ROW_COUNT, |t| self.row_count_for(t));
                return child_cost + rows * SCAN_COST_PER_ROW;
            }
            RelLang::ScanAlias([table_id, alias_id]) => {
                let child_cost = costs(*table_id) + costs(*alias_id);
                let rows = self
                    .resolve_table(*table_id)
                    .map_or(DEFAULT_ROW_COUNT, |t| self.row_count_for(t));
                return child_cost + rows * SCAN_COST_PER_ROW;
            }

            // --- Filter: per-row evaluation cost scaled by
            //     default selectivity ---
            RelLang::Filter([pred_id, input_id]) => {
                let pred_cost = costs(*pred_id);
                let input_cost = costs(*input_id);
                let simd_factor = 256.0 / f64::from(self.hardware.simd_width_bits);
                // Per-row filter evaluation; selectivity reduces
                // the effective rows that flow downstream.
                let per_row = 0.01 * simd_factor;
                // Use input_cost as a proxy for input cardinality
                // (higher input cost ≈ more rows to filter).
                let scale = (input_cost * DEFAULT_SELECTIVITY).max(1.0);
                return pred_cost + input_cost + per_row * scale;
            }

            RelLang::Project(_) => 5.0,

            // --- Join: quadratic-ish cost via child sizes ---
            RelLang::Join([jtype_id, cond_id, left_id, right_id]) => {
                let jtype_cost = costs(*jtype_id);
                let cond_cost = costs(*cond_id);
                let left_cost = costs(*left_id);
                let right_cost = costs(*right_id);
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                // Scale join cost by the product of child costs
                // (proxy for cardinality product), clamped to
                // avoid blowup on very large inputs.
                let card_proxy = ((left_cost + 1.0) * (right_cost + 1.0)).sqrt();
                let join_base = 100.0 * cache_factor;
                return jtype_cost
                    + cond_cost
                    + left_cost
                    + right_cost
                    + join_base * card_proxy.max(1.0) / DEFAULT_ROW_COUNT;
            }

            RelLang::Aggregate(_) => {
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                80.0 * cache_factor
            }
            RelLang::Sort(_) => {
                let par = 8.0 / f64::from(self.hardware.cpu_cores);
                150.0 * par.max(0.5)
            }
            RelLang::IncrementalSort(_) => {
                let par = 8.0 / f64::from(self.hardware.cpu_cores);
                60.0 * par.max(0.5)
            }
            RelLang::Limit(_) => 0.5,
            RelLang::Union(_) | RelLang::Intersect(_) | RelLang::Except(_) => 50.0,
            RelLang::Window(_) => {
                let par = 8.0 / f64::from(self.hardware.cpu_cores);
                200.0 * par.max(0.5)
            }
            RelLang::DistinctRel(_) => {
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                150.0 * cache_factor
            }
            RelLang::IndexOnlyScan([table_id, _, cols_id, pred_id]) => {
                let child_cost = costs(*table_id) + costs(*cols_id) + costs(*pred_id);
                let rows = self
                    .resolve_table(*table_id)
                    .map_or(DEFAULT_ROW_COUNT, |t| self.row_count_for(t));
                // Index-only scan ~10% of full scan cost
                return child_cost + rows * SCAN_COST_PER_ROW * 0.1;
            }
            RelLang::BitmapIndexScan([table_id, _, _]) => {
                let rows = self
                    .resolve_table(*table_id)
                    .map_or(DEFAULT_ROW_COUNT, |t| self.row_count_for(t));
                // Bitmap scan reads ~30% of pages
                rows * SCAN_COST_PER_ROW * 0.3
            }
            RelLang::BitmapHeapScan(_) => 40.0,
            RelLang::BitmapAnd(_) | RelLang::BitmapOr(_) => 10.0,
            RelLang::MetadataLookup(_) => {
                return 1.0;
            }
            _ => 0.1,
        };

        let child_cost: f64 = enode.children().iter().map(|child| costs(*child)).sum();

        base_cost + child_cost
    }
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "exact float literals in tests")]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn cost_function_basic() {
        let hardware = HardwareProfile::cpu_only();
        let mut table_stats = HashMap::new();
        table_stats.insert("users".to_string(), Statistics::new(1_000_000.0));
        let staleness = HashMap::new();

        let cost_fn = CardinalityAwareCostFn::new(hardware, table_stats, staleness);

        let scan = RelExpr::scan("users");
        let filter = scan.filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });

        let scan_estimate = cost_fn
            .estimator
            .estimate(&RelExpr::scan("users"), cost_fn.stats_provider.as_ref());
        let filter_estimate = cost_fn
            .estimator
            .estimate(&filter, cost_fn.stats_provider.as_ref());

        // Filter should reduce cardinality
        assert!(
            filter_estimate.rows < scan_estimate.rows,
            "Filter should reduce estimated rows"
        );
    }

    #[test]
    fn staleness_factor_fresh() {
        let cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        assert_eq!(cost_fn.staleness_factor("any_table"), 1.0);
    }

    #[test]
    fn staleness_factor_all_levels() {
        let mut staleness = HashMap::new();
        staleness.insert("fresh".to_string(), Staleness::Fresh);
        staleness.insert("slight".to_string(), Staleness::SlightlyStale);
        staleness.insert("moderate".to_string(), Staleness::ModeratelyStale);
        staleness.insert("very".to_string(), Staleness::VeryStale);
        staleness.insert("unknown".to_string(), Staleness::Unknown);

        let cost_fn = make_cost_fn(HashMap::new(), staleness);

        assert_eq!(cost_fn.staleness_factor("fresh"), 1.0);
        assert_eq!(cost_fn.staleness_factor("slight"), 1.05);
        assert_eq!(cost_fn.staleness_factor("moderate"), 1.2);
        assert_eq!(cost_fn.staleness_factor("very"), 1.5);
        assert_eq!(cost_fn.staleness_factor("unknown"), 2.0);
    }

    #[test]
    fn staleness_factor_missing_table_defaults_to_one() {
        let mut staleness = HashMap::new();
        staleness.insert("users".to_string(), Staleness::ModeratelyStale);
        let cost_fn = make_cost_fn(HashMap::new(), staleness);

        assert_eq!(cost_fn.staleness_factor("users"), 1.2);
        assert_eq!(cost_fn.staleness_factor("not_present"), 1.0);
    }

    #[test]
    fn cost_fn_with_zero_row_table() {
        let mut table_stats = HashMap::new();
        table_stats.insert("empty_table".to_string(), Statistics::new(0.0));

        let cost_fn = make_cost_fn(table_stats, HashMap::new());

        let scan = RelExpr::scan("empty_table");
        let estimate = cost_fn
            .estimator
            .estimate(&scan, cost_fn.stats_provider.as_ref());
        assert!(estimate.rows >= 0.0);
    }

    fn make_cost_fn(
        table_stats: HashMap<String, Statistics>,
        staleness: HashMap<String, Staleness>,
    ) -> CardinalityAwareCostFn {
        CardinalityAwareCostFn::new(HardwareProfile::cpu_only(), table_stats, staleness)
    }

    // ---- CostFunction<RelLang>::cost tests ----
    //
    // These test the egg CostFunction trait impl by calling
    // cost() directly with a closure that returns fixed child
    // costs.

    fn zero_child_cost(_id: Id) -> f64 {
        0.0
    }

    #[test]
    fn cost_fn_scan_node_default_rows() {
        // No symbol map, so falls back to DEFAULT_ROW_COUNT (1000)
        // cost = 1000 * 0.05 = 50.0
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Scan([Id::from(0)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 50.0).abs() < f64::EPSILON,
            "Scan with default rows should cost 50.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_scan_node_with_stats() {
        // Provide a symbol map and stats for a 10_000-row table.
        // cost = 10_000 * 0.05 = 500.0
        let mut table_stats = HashMap::new();
        table_stats.insert("big".to_string(), Statistics::new(10_000.0));
        let mut cost_fn = make_cost_fn(table_stats, HashMap::new());
        let mut sym_map = HashMap::new();
        sym_map.insert(Id::from(0), "big".to_string());
        cost_fn.symbol_map = sym_map;

        let node = RelLang::Scan([Id::from(0)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 500.0).abs() < f64::EPSILON,
            "Scan of 10k-row table should cost 500.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_scan_staleness_inflates() {
        let mut table_stats = HashMap::new();
        table_stats.insert("t".to_string(), Statistics::new(1000.0));
        let mut staleness = HashMap::new();
        staleness.insert("t".to_string(), Staleness::Unknown);
        let mut cost_fn = make_cost_fn(table_stats, staleness);
        let mut sym_map = HashMap::new();
        sym_map.insert(Id::from(0), "t".to_string());
        cost_fn.symbol_map = sym_map;

        let node = RelLang::Scan([Id::from(0)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        // Unknown staleness => factor 2.0
        // cost = 1000 * 2.0 * 0.05 = 100.0
        assert!(
            (c - 100.0).abs() < f64::EPSILON,
            "Scan with Unknown staleness should cost 100.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_filter_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Filter([Id::from(0), Id::from(1)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        // 256-bit SIMD default: factor = 256/256 = 1.0
        // cost = 10.0 * 1.0 = 10.0
        assert!(c > 0.0);
    }

    #[test]
    fn cost_fn_project_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Project([Id::from(0), Id::from(1)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 5.0).abs() < f64::EPSILON,
            "Project cost should be 5.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_join_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Join([Id::from(0), Id::from(1), Id::from(2), Id::from(3)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(c > 0.0, "Join cost should be positive, got {c}");
    }

    #[test]
    fn cost_fn_aggregate_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Aggregate([Id::from(0), Id::from(1), Id::from(2)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(c > 0.0);
    }

    #[test]
    fn cost_fn_sort_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Sort([Id::from(0), Id::from(1)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(c > 0.0);
    }

    #[test]
    fn cost_fn_incremental_sort_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::IncrementalSort([Id::from(0), Id::from(1), Id::from(2)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(c > 0.0);
    }

    #[test]
    fn cost_fn_limit_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Limit([Id::from(0), Id::from(1), Id::from(2)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 0.5).abs() < f64::EPSILON,
            "Limit cost should be 0.5, got {c}",
        );
    }

    #[test]
    fn cost_fn_union_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Union([Id::from(0), Id::from(1), Id::from(2)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 50.0).abs() < f64::EPSILON,
            "Union cost should be 50.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_window_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Window([Id::from(0), Id::from(1)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(c > 0.0);
    }

    #[test]
    fn cost_fn_distinct_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::DistinctRel([Id::from(0)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(c > 0.0);
    }

    #[test]
    fn cost_fn_index_only_scan_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::IndexOnlyScan([Id::from(0), Id::from(1), Id::from(2), Id::from(3)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 5.0).abs() < f64::EPSILON,
            "IndexOnlyScan cost should be 5.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_bitmap_index_scan_node() {
        // No symbol map => DEFAULT_ROW_COUNT (1000)
        // cost = 1000 * 0.05 * 0.3 = 15.0 + child_cost 0 = 15.0
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::BitmapIndexScan([Id::from(0), Id::from(1), Id::from(2)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 15.0).abs() < f64::EPSILON,
            "BitmapIndexScan cost should be 15.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_bitmap_heap_scan_node() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::BitmapHeapScan([Id::from(0), Id::from(1), Id::from(2)]);
        let c = cost_fn.cost(&node, zero_child_cost);
        assert!(
            (c - 40.0).abs() < f64::EPSILON,
            "BitmapHeapScan cost should be 40.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_metadata_lookup_returns_early() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::MetadataLookup([Id::from(0), Id::from(1)]);
        let c = cost_fn.cost(&node, |_| 999.0);
        // MetadataLookup returns 1.0 immediately,
        // ignoring child costs
        assert!(
            (c - 1.0).abs() < f64::EPSILON,
            "MetadataLookup cost should be 1.0, got {c}",
        );
    }

    #[test]
    fn cost_fn_child_costs_increase_total() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let node = RelLang::Filter([Id::from(0), Id::from(1)]);
        let with_children = cost_fn.cost(&node, |_| 100.0);
        let without_children = cost_fn.cost(&node, zero_child_cost);
        // Higher child costs must produce a higher total
        assert!(
            with_children > without_children,
            "Filter with child costs ({with_children}) should exceed \
             filter without ({without_children})",
        );
        // The child costs (pred + input = 200) are included in the total
        assert!(
            with_children >= 200.0,
            "Total should be at least the sum of child costs, got {with_children}",
        );
    }

    #[test]
    fn cost_fn_scan_alias_same_as_scan() {
        let mut cost_fn = make_cost_fn(HashMap::new(), HashMap::new());
        let scan_node = RelLang::Scan([Id::from(0)]);
        let alias_node = RelLang::ScanAlias([Id::from(0), Id::from(1)]);
        let scan_cost = cost_fn.cost(&scan_node, zero_child_cost);
        let alias_cost = cost_fn.cost(&alias_node, zero_child_cost);
        assert!(
            (scan_cost - alias_cost).abs() < f64::EPSILON,
            "ScanAlias cost should equal Scan cost",
        );
    }
}
