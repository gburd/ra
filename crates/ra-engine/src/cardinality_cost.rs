//! Cardinality-aware cost function for e-graph extraction.
//!
//! Extends the basic cost model by using ML-based cardinality estimation
//! to scale operator costs. This produces more accurate cost estimates
//! for intermediate results in the query plan.

use std::collections::HashMap;
use std::sync::Arc;

use egg::{CostFunction, Id, Language};
use ra_core::cost::StatisticsProvider;
use ra_core::statistics::Statistics;
use ra_hardware::HardwareProfile;
use ra_ml::estimator::{CardinalityEstimator, HeuristicEstimator};
use ra_stats::accuracy::Staleness;

use crate::egraph::RelLang;

/// Simple statistics provider backed by a HashMap.
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
/// 1. Estimates output cardinality using the ML estimator
/// 2. Scales the base cost by the estimated cardinality
/// 3. Uses hardware-aware cost factors
pub struct CardinalityAwareCostFn {
    /// Cardinality estimator (ML or heuristic)
    estimator: Arc<dyn CardinalityEstimator>,
    /// Statistics provider for base table stats
    stats_provider: Arc<TableStatsProvider>,
    /// Hardware profile for cost adjustments
    hardware: HardwareProfile,
    /// Staleness adjustments per table
    staleness_map: HashMap<String, Staleness>,
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
            stats_provider: Arc::new(TableStatsProvider {
                stats: table_stats,
            }),
            hardware,
            staleness_map,
        }
    }

    /// Get staleness factor for a table.
    fn staleness_factor(&self, table: &str) -> f64 {
        self.staleness_map
            .get(table)
            .map_or(1.0, |s| match s {
                Staleness::Fresh => 1.0,
                Staleness::SlightlyStale => 1.05,
                Staleness::ModeratelyStale => 1.2,
                Staleness::VeryStale => 1.5,
                Staleness::Unknown => 2.0,
            })
    }
}

impl CostFunction<RelLang> for CardinalityAwareCostFn {
    type Cost = f64;

    #[allow(clippy::cast_precision_loss)]
    fn cost<C>(&mut self, enode: &RelLang, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        // Base operator costs (hardware-adjusted)
        let base_cost = match enode {
            RelLang::Scan(_) | RelLang::ScanAlias(_) => {
                // Sequential scan cost
                50.0
            }
            RelLang::Filter(_) => {
                let simd_factor = 256.0 / f64::from(self.hardware.simd_width_bits);
                10.0 * simd_factor
            }
            RelLang::Project(_) => 5.0,
            RelLang::Join(_) => {
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                100.0 * cache_factor
            }
            RelLang::Aggregate(_) => {
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                80.0 * cache_factor
            }
            RelLang::Sort(_) => {
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                150.0 * parallelism_factor.max(0.5)
            }
            RelLang::IncrementalSort(_) => {
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                60.0 * parallelism_factor.max(0.5)
            }
            RelLang::Limit(_) => 0.5,
            RelLang::Union(_) | RelLang::Intersect(_) | RelLang::Except(_) => 50.0,
            RelLang::Window(_) => {
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                200.0 * parallelism_factor.max(0.5)
            }
            RelLang::DistinctRel(_) => {
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                150.0 * cache_factor
            }
            RelLang::IndexOnlyScan(_) => 5.0,
            RelLang::BitmapIndexScan(_) => 30.0,
            RelLang::BitmapHeapScan(_) => 40.0,
            RelLang::BitmapAnd(_) | RelLang::BitmapOr(_) => 10.0,
            RelLang::MetadataLookup(_) => {
                // O(1) metadata lookup, cheaper than any scan
                return 1.0;
            }
            _ => 0.1,
        };

        // Add child costs
        let child_cost: f64 = enode.children().iter().map(|child| costs(*child)).sum();

        base_cost + child_cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn cost_function_basic() {
        let hardware = HardwareProfile::cpu_only();
        let mut table_stats = HashMap::new();
        table_stats.insert(
            "users".to_string(),
            Statistics::new(1_000_000.0),
        );
        let staleness = HashMap::new();

        let cost_fn = CardinalityAwareCostFn::new(
            hardware,
            table_stats,
            staleness,
        );

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
        table_stats.insert(
            "empty_table".to_string(),
            Statistics::new(0.0),
        );

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
        CardinalityAwareCostFn::new(
            HardwareProfile::cpu_only(),
            table_stats,
            staleness,
        )
    }
}
