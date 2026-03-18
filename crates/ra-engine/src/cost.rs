//! Integrated cost model combining statistics and hardware awareness.
//!
//! Bridges [`ra_stats`] statistics tracking with [`ra_hardware`] cost
//! models, producing staleness-adjusted cost estimates for the
//! equality saturation optimizer.

use std::collections::HashMap;
use std::hash::BuildHasher;

use egg::Language;
use ra_core::statistics::Statistics;
use ra_hardware::HardwareProfile;
use ra_stats::accuracy::{QualityMetrics, Staleness, StatisticsState};
use ra_stats::integration::{ManagedTableStats, StatisticsAdapter};
use ra_stats::profiles::StatisticsProfile;

/// Staleness inflation factors applied to row count estimates.
///
/// When statistics are stale, we inflate row count estimates to
/// account for uncertainty. This biases the optimizer toward plans
/// that are more robust to cardinality mis-estimation (e.g.,
/// preferring hash joins over nested loops).
fn staleness_factor(staleness: Staleness) -> f64 {
    match staleness {
        Staleness::Fresh => 1.0,
        Staleness::SlightlyStale => 1.05,
        Staleness::ModeratelyStale => 1.2,
        Staleness::VeryStale => 1.5,
        Staleness::Unknown => 2.0,
    }
}

/// Default row count assumed when no statistics are available.
const DEFAULT_ROW_COUNT: f64 = 1000.0;

/// Confidence discount applied to cost estimates.
///
/// Low-confidence statistics produce wider cost ranges, which
/// the optimizer should treat conservatively. Returns a multiplier
/// in `[1.0, 2.0]` where 1.0 = full confidence, 2.0 = no confidence.
fn confidence_discount(confidence: f64) -> f64 {
    let clamped = confidence.clamp(0.0, 1.0);
    2.0 - clamped
}

/// Combined cost model integrating statistics staleness and hardware.
///
/// For each operator, it:
/// 1. Looks up table statistics (falling back to defaults)
/// 2. Adjusts row counts based on staleness
/// 3. Applies hardware-specific cost factors
/// 4. Discounts by confidence level
#[derive(Debug)]
pub struct IntegratedCostModel {
    adapter: StatisticsAdapter,
    hardware: HardwareProfile,
}

impl IntegratedCostModel {
    /// Create a new integrated cost model.
    #[must_use]
    pub fn new(
        profile: StatisticsProfile,
        hardware: HardwareProfile,
    ) -> Self {
        Self {
            adapter: StatisticsAdapter::new(profile),
            hardware,
        }
    }

    /// Register managed statistics for a table.
    pub fn add_table(
        &mut self,
        name: String,
        stats: ManagedTableStats,
    ) {
        self.adapter.add_table(name, stats);
    }

    /// Get the statistics profile.
    #[must_use]
    pub fn profile(&self) -> &StatisticsProfile {
        self.adapter.profile()
    }

    /// Get the hardware profile.
    #[must_use]
    pub fn hardware(&self) -> &HardwareProfile {
        &self.hardware
    }

    /// Number of registered tables.
    #[must_use]
    pub fn table_count(&self) -> usize {
        self.adapter.table_count()
    }

    /// Whether statistics for the given table should be refreshed.
    #[must_use]
    pub fn should_refresh(&self, table: &str) -> bool {
        self.adapter
            .get_table_stats(table)
            .map_or(true, |m| self.adapter.should_reject(&m.state))
    }

    /// Get quality metrics for a table's statistics.
    #[must_use]
    pub fn quality_metrics(
        &self,
        table: &str,
    ) -> Option<QualityMetrics> {
        self.adapter
            .get_table_stats(table)
            .map(|m| QualityMetrics::from_state(&m.state))
    }

    /// Convert managed stats to core Statistics with staleness
    /// adjustments, or return defaults if the table is unknown.
    #[must_use]
    pub fn effective_statistics(
        &self,
        table: &str,
    ) -> Statistics {
        if let Some(managed) = self.adapter.get_table_stats(table) {
            self.adapter.to_core_statistics(managed)
        } else {
            Statistics::new(1000.0)
        }
    }

    /// Estimate cost for a scan operator, incorporating both
    /// statistics staleness and hardware characteristics.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn scan_cost(&self, table: &str) -> f64 {
        let stats = self.effective_statistics(table);
        let row_count = stats.row_count;
        let avg_size = stats.avg_row_size.max(1) as f64;

        let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
        let base = row_count * avg_size / (1024.0 * 1024.0);
        let cost = base * storage_factor;

        let disc = self.confidence_for_table(table);
        cost * disc
    }

    /// Estimate cost for a filter operator.
    #[must_use]
    pub fn filter_cost(&self, table: &str) -> f64 {
        let stats = self.effective_statistics(table);
        let simd_factor =
            256.0 / f64::from(self.hardware.simd_width_bits);
        let cost = stats.row_count * 0.001 * simd_factor;

        let disc = self.confidence_for_table(table);
        cost * disc
    }

    /// Estimate cost for a join operator.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn join_cost(
        &self,
        left_table: &str,
        right_table: &str,
    ) -> f64 {
        let left_stats = self.effective_statistics(left_table);
        let right_stats = self.effective_statistics(right_table);

        let cache_mb =
            self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
        let cache_factor = 16.0 / cache_mb.max(1.0);

        let build_rows = left_stats.row_count.min(right_stats.row_count);
        let probe_rows = left_stats.row_count.max(right_stats.row_count);

        let cost = (build_rows * 100e-6 + probe_rows * 50e-6)
            * cache_factor;

        let disc_left = self.confidence_for_table(left_table);
        let disc_right = self.confidence_for_table(right_table);
        cost * disc_left.max(disc_right)
    }

    /// Estimate cost for a sort operator.
    #[must_use]
    pub fn sort_cost(&self, table: &str) -> f64 {
        let stats = self.effective_statistics(table);
        let n = stats.row_count;
        let n_log_n = if n > 1.0 { n * n.log2() } else { n };

        let par_factor =
            8.0 / f64::from(self.hardware.cpu_cores).max(1.0);
        let cost = n_log_n * 200e-9 * par_factor.max(0.5);

        let disc = self.confidence_for_table(table);
        cost * disc
    }

    /// Estimate cost for an aggregate operator.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn aggregate_cost(
        &self,
        table: &str,
        group_count: f64,
    ) -> f64 {
        let stats = self.effective_statistics(table);
        let cache_mb =
            self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
        let cache_factor = 16.0 / cache_mb.max(1.0);

        let cost = (stats.row_count * 80e-9
            + group_count * 64.0 * cache_factor * 1e-9)
            * cache_factor;

        let disc = self.confidence_for_table(table);
        cost * disc
    }

    /// Compute the confidence discount for a table.
    fn confidence_for_table(&self, table: &str) -> f64 {
        self.adapter
            .get_table_stats(table)
            .map_or(
                confidence_discount(0.3),
                |m| confidence_discount(m.state.confidence),
            )
    }

    /// Get staleness classification for a table.
    #[must_use]
    pub fn staleness(&self, table: &str) -> Staleness {
        self.adapter
            .get_table_stats(table)
            .map_or(Staleness::Unknown, |m| m.state.staleness())
    }

    /// Build a `HashMap` of core Statistics for all registered tables,
    /// suitable for passing to `extract_best`.
    #[must_use]
    pub fn all_core_statistics(&self) -> HashMap<String, Statistics> {
        // StatisticsAdapter does not expose an iterator over table
        // names, so callers should track names externally or use
        // `effective_statistics` per table.
        HashMap::new()
    }
}

/// Build an [`IntegratedCostModel`] from raw core statistics and
/// a hardware profile. Wraps each entry in a fresh
/// `ManagedTableStats` with `ExactCount` source.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn from_core_statistics<S: BuildHasher>(
    table_stats: &HashMap<String, Statistics, S>,
    hardware: &HardwareProfile,
    profile: StatisticsProfile,
) -> IntegratedCostModel {
    use ra_stats::accuracy::StatisticsSource;
    use ra_stats::types::TableStats;

    let mut model = IntegratedCostModel::new(
        profile,
        hardware.clone(),
    );

    for (name, stats) in table_stats {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let row_count = stats.row_count as u64;

        let managed = ManagedTableStats {
            table: TableStats {
                row_count,
                page_count: (stats.total_size / 8192).max(1),
                average_row_size: stats.avg_row_size as f64,
                table_size_bytes: stats.total_size,
                live_tuples: Some(row_count),
                dead_tuples: Some(0),
                last_analyzed: None,
            },
            columns: HashMap::new(),
            state: StatisticsState::new(
                StatisticsSource::ExactCount,
                row_count,
            ),
        };
        model.add_table(name.clone(), managed);
    }

    model
}

/// Extended cost function for egg that uses integrated statistics
/// and hardware information.
///
/// Replaces the basic `RelCostFn` when full stats/hardware integration
/// is desired.
#[derive(Debug)]
pub struct IntegratedCostFn {
    hardware: HardwareProfile,
    table_stats: HashMap<String, Statistics>,
    staleness_map: HashMap<String, Staleness>,
}

impl IntegratedCostFn {
    /// Create a new integrated cost function.
    #[must_use]
    pub fn new(
        hardware: HardwareProfile,
        table_stats: HashMap<String, Statistics>,
        staleness_map: HashMap<String, Staleness>,
    ) -> Self {
        Self {
            hardware,
            table_stats,
            staleness_map,
        }
    }

    /// Create from an `IntegratedCostModel`, extracting necessary data.
    #[must_use]
    pub fn from_model(
        model: &IntegratedCostModel,
        table_names: &[String],
    ) -> Self {
        let mut table_stats = HashMap::new();
        let mut staleness_map = HashMap::new();

        for name in table_names {
            table_stats.insert(
                name.clone(),
                model.effective_statistics(name),
            );
            staleness_map.insert(
                name.clone(),
                model.staleness(name),
            );
        }

        Self {
            hardware: model.hardware().clone(),
            table_stats,
            staleness_map,
        }
    }

    /// Look up adjusted row count for a table symbol.
    ///
    /// Returns the base row count inflated by the staleness factor.
    /// Defaults to 1000 rows with `Unknown` staleness if the table
    /// is not registered.
    #[must_use]
    pub fn row_count_for(&self, table_name: &str) -> f64 {
        let base = self
            .table_stats
            .get(table_name)
            .map_or(DEFAULT_ROW_COUNT, |s| s.row_count);

        let factor = self
            .staleness_map
            .get(table_name)
            .copied()
            .map_or(
                staleness_factor(Staleness::Unknown),
                staleness_factor,
            );

        base * factor
    }
}

impl egg::CostFunction<crate::egraph::RelLang> for IntegratedCostFn {
    type Cost = f64;

    fn cost<C>(
        &mut self,
        enode: &crate::egraph::RelLang,
        mut costs: C,
    ) -> Self::Cost
    where
        C: FnMut(egg::Id) -> Self::Cost,
    {
        use crate::egraph::RelLang;

        let base_cost = match enode {
            RelLang::Scan([table_id]) => {
                let child_cost = costs(*table_id);
                let storage_factor =
                    100.0 / self.hardware.storage_bandwidth_gbps;
                return child_cost + (100.0 * storage_factor);
            }
            RelLang::ScanAlias([table_id, alias_id]) => {
                let storage_factor =
                    100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id)
                    + costs(*alias_id)
                    + (100.0 * storage_factor);
            }
            RelLang::Filter(_) | RelLang::Project(_) => {
                let simd_factor = 256.0
                    / f64::from(self.hardware.simd_width_bits);
                1.0 * simd_factor
            }
            RelLang::Join(_) => {
                #[allow(clippy::cast_precision_loss)]
                let cache_mb = self.hardware.l3_cache_bytes as f64
                    / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                500.0 * cache_factor
            }
            RelLang::Aggregate(_) => {
                #[allow(clippy::cast_precision_loss)]
                let cache_mb = self.hardware.l3_cache_bytes as f64
                    / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                200.0 * cache_factor
            }
            RelLang::Sort(_) => {
                let par_factor =
                    8.0 / f64::from(self.hardware.cpu_cores);
                150.0 * par_factor.max(0.5)
            }
            RelLang::Limit(_) => 0.5,
            RelLang::Union(_)
            | RelLang::Intersect(_)
            | RelLang::Except(_) => 50.0,
            _ => 0.1,
        };

        let child_cost: f64 = enode
            .children()
            .iter()
            .map(|child| costs(*child))
            .sum();

        base_cost + child_cost
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use ra_hardware::HardwareProfile;
    use ra_stats::accuracy::{StatisticsSource, StatisticsState};
    use ra_stats::profiles::StatisticsProfile;
    use ra_stats::types::TableStats;

    fn make_managed(
        row_count: u64,
        source: StatisticsSource,
    ) -> ManagedTableStats {
        ManagedTableStats {
            table: TableStats {
                row_count,
                page_count: row_count / 100 + 1,
                average_row_size: 100.0,
                table_size_bytes: row_count * 100,
                live_tuples: Some(row_count),
                dead_tuples: Some(0),
                last_analyzed: None,
            },
            columns: HashMap::new(),
            state: StatisticsState::new(source, row_count),
        }
    }

    fn make_stale_managed(
        row_count: u64,
        modifications: u64,
    ) -> ManagedTableStats {
        let mut m = make_managed(
            row_count,
            StatisticsSource::ExactCount,
        );
        m.state.record_modifications(modifications);
        m
    }

    // ---- IntegratedCostModel creation ----

    #[test]
    fn model_creation() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        assert_eq!(model.table_count(), 0);
        assert_eq!(model.profile().name, "Standard");
    }

    #[test]
    fn model_add_table() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "users".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        assert_eq!(model.table_count(), 1);
    }

    #[test]
    fn model_hardware_accessor() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::gpu_server(),
        );
        assert!(model.hardware().gpu_available);
    }

    // ---- staleness_factor ----

    #[test]
    fn staleness_factor_fresh() {
        assert_eq!(staleness_factor(Staleness::Fresh), 1.0);
    }

    #[test]
    fn staleness_factor_slightly_stale() {
        assert_eq!(staleness_factor(Staleness::SlightlyStale), 1.05);
    }

    #[test]
    fn staleness_factor_moderately_stale() {
        assert_eq!(staleness_factor(Staleness::ModeratelyStale), 1.2);
    }

    #[test]
    fn staleness_factor_very_stale() {
        assert_eq!(staleness_factor(Staleness::VeryStale), 1.5);
    }

    #[test]
    fn staleness_factor_unknown() {
        assert_eq!(staleness_factor(Staleness::Unknown), 2.0);
    }

    // ---- confidence_discount ----

    #[test]
    fn confidence_discount_full() {
        assert_eq!(confidence_discount(1.0), 1.0);
    }

    #[test]
    fn confidence_discount_half() {
        assert_eq!(confidence_discount(0.5), 1.5);
    }

    #[test]
    fn confidence_discount_zero() {
        assert_eq!(confidence_discount(0.0), 2.0);
    }

    #[test]
    fn confidence_discount_clamps_above_one() {
        assert_eq!(confidence_discount(1.5), 1.0);
    }

    #[test]
    fn confidence_discount_clamps_below_zero() {
        assert_eq!(confidence_discount(-0.5), 2.0);
    }

    // ---- effective_statistics ----

    #[test]
    fn effective_stats_known_table() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "users".into(),
            make_managed(50_000, StatisticsSource::ExactCount),
        );
        let stats = model.effective_statistics("users");
        assert!((stats.row_count - 50_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_stats_unknown_table() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        let stats = model.effective_statistics("nonexistent");
        assert!((stats.row_count - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_stats_stale_inflated() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "orders".into(),
            make_stale_managed(10_000, 5_000),
        );
        let stats = model.effective_statistics("orders");
        // 5_000 / 10_000 = 50% change => VeryStale => factor 1.5
        assert!(stats.row_count > 10_000.0);
    }

    // ---- staleness classification ----

    #[test]
    fn staleness_fresh_table() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        assert_eq!(model.staleness("t"), Staleness::Fresh);
    }

    #[test]
    fn staleness_stale_table() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_stale_managed(10_000, 3_000),
        );
        assert_eq!(model.staleness("t"), Staleness::VeryStale);
    }

    #[test]
    fn staleness_unknown_table() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        assert_eq!(model.staleness("missing"), Staleness::Unknown);
    }

    // ---- should_refresh ----

    #[test]
    fn should_refresh_fresh() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        assert!(!model.should_refresh("t"));
    }

    #[test]
    fn should_refresh_unknown() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        assert!(model.should_refresh("missing"));
    }

    // ---- quality_metrics ----

    #[test]
    fn quality_metrics_exact_fresh() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        let qm = model.quality_metrics("t").expect("should exist");
        assert_eq!(qm.quality_score, 1.0);
    }

    #[test]
    fn quality_metrics_none_for_missing() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        assert!(model.quality_metrics("missing").is_none());
    }

    // ---- scan_cost ----

    #[test]
    fn scan_cost_known_table() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        let cost = model.scan_cost("t");
        assert!(cost > 0.0);
        assert!(cost.is_finite());
    }

    #[test]
    fn scan_cost_unknown_table() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        let cost = model.scan_cost("missing");
        assert!(cost > 0.0);
    }

    #[test]
    fn scan_cost_faster_with_better_storage() {
        let mut model_slow = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        let mut hw_fast = HardwareProfile::cpu_only();
        hw_fast.storage_bandwidth_gbps = 14.0;
        let mut model_fast = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw_fast,
        );

        let managed =
            make_managed(1_000_000, StatisticsSource::ExactCount);
        model_slow.add_table("t".into(), managed.clone());
        model_fast.add_table("t".into(), managed);

        assert!(model_fast.scan_cost("t") < model_slow.scan_cost("t"));
    }

    // ---- filter_cost ----

    #[test]
    fn filter_cost_positive() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        assert!(model.filter_cost("t") > 0.0);
    }

    #[test]
    fn filter_cost_wider_simd_cheaper() {
        let mut hw_narrow = HardwareProfile::cpu_only();
        hw_narrow.simd_width_bits = 128;
        let mut hw_wide = HardwareProfile::cpu_only();
        hw_wide.simd_width_bits = 512;

        let mut model_narrow = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw_narrow,
        );
        let mut model_wide = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw_wide,
        );

        let managed =
            make_managed(100_000, StatisticsSource::ExactCount);
        model_narrow.add_table("t".into(), managed.clone());
        model_wide.add_table("t".into(), managed);

        assert!(model_wide.filter_cost("t") < model_narrow.filter_cost("t"));
    }

    // ---- join_cost ----

    #[test]
    fn join_cost_positive() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "a".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        model.add_table(
            "b".into(),
            make_managed(1_000, StatisticsSource::ExactCount),
        );
        assert!(model.join_cost("a", "b") > 0.0);
    }

    #[test]
    fn join_cost_bigger_cache_cheaper() {
        let mut hw_small_cache = HardwareProfile::cpu_only();
        hw_small_cache.l3_cache_bytes = 8 * 1024 * 1024;
        let mut hw_big_cache = HardwareProfile::cpu_only();
        hw_big_cache.l3_cache_bytes = 128 * 1024 * 1024;

        let mut model_small = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw_small_cache,
        );
        let mut model_big = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw_big_cache,
        );

        let a = make_managed(100_000, StatisticsSource::ExactCount);
        let b = make_managed(10_000, StatisticsSource::ExactCount);
        model_small.add_table("a".into(), a.clone());
        model_small.add_table("b".into(), b.clone());
        model_big.add_table("a".into(), a);
        model_big.add_table("b".into(), b);

        assert!(
            model_big.join_cost("a", "b")
                < model_small.join_cost("a", "b")
        );
    }

    // ---- sort_cost ----

    #[test]
    fn sort_cost_positive() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        assert!(model.sort_cost("t") > 0.0);
    }

    #[test]
    fn sort_cost_more_cores_cheaper() {
        let mut hw_few = HardwareProfile::cpu_only();
        hw_few.cpu_cores = 4;
        let mut hw_many = HardwareProfile::cpu_only();
        hw_many.cpu_cores = 64;

        let mut model_few = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw_few,
        );
        let mut model_many = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw_many,
        );

        let managed =
            make_managed(1_000_000, StatisticsSource::ExactCount);
        model_few.add_table("t".into(), managed.clone());
        model_many.add_table("t".into(), managed);

        assert!(model_many.sort_cost("t") < model_few.sort_cost("t"));
    }

    // ---- aggregate_cost ----

    #[test]
    fn aggregate_cost_positive() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        assert!(model.aggregate_cost("t", 100.0) > 0.0);
    }

    // ---- from_core_statistics ----

    #[test]
    fn from_core_statistics_creates_model() {
        let mut stats = HashMap::new();
        stats.insert(
            "users".into(),
            Statistics::new(50_000.0),
        );
        stats.insert(
            "orders".into(),
            Statistics::new(500_000.0),
        );

        let model = from_core_statistics(
            &stats,
            &HardwareProfile::cpu_only(),
            StatisticsProfile::standard(),
        );
        assert_eq!(model.table_count(), 2);

        let es = model.effective_statistics("users");
        assert!((es.row_count - 50_000.0).abs() < f64::EPSILON);
    }

    // ---- IntegratedCostFn ----

    #[test]
    fn integrated_cost_fn_row_count_fresh() {
        let mut stats = HashMap::new();
        stats.insert("t".into(), Statistics::new(5000.0));
        let staleness_map = HashMap::new();

        let cfn = IntegratedCostFn::new(
            HardwareProfile::cpu_only(),
            stats,
            staleness_map,
        );
        let rows = cfn.row_count_for("t");
        // No staleness entry => Unknown => 2.0x
        assert!((rows - 10_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn integrated_cost_fn_row_count_with_staleness() {
        let mut stats = HashMap::new();
        stats.insert("t".into(), Statistics::new(5000.0));
        let mut staleness_map = HashMap::new();
        staleness_map.insert("t".into(), Staleness::Fresh);

        let cfn = IntegratedCostFn::new(
            HardwareProfile::cpu_only(),
            stats,
            staleness_map,
        );
        let rows = cfn.row_count_for("t");
        assert!((rows - 5000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn integrated_cost_fn_unknown_table() {
        let cfn = IntegratedCostFn::new(
            HardwareProfile::cpu_only(),
            HashMap::new(),
            HashMap::new(),
        );
        let rows = cfn.row_count_for("missing");
        // default 1000 * Unknown 2.0
        assert!((rows - 2000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn integrated_cost_fn_from_model() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(5000, StatisticsSource::ExactCount),
        );

        let cfn = IntegratedCostFn::from_model(
            &model,
            &["t".to_string()],
        );
        let rows = cfn.row_count_for("t");
        assert!((rows - 5000.0).abs() < f64::EPSILON);
    }

    // ---- Profile-specific behavior ----

    #[test]
    fn realtime_profile_low_refresh_threshold() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::real_time(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_stale_managed(10_000, 2_000),
        );
        assert!(model.should_refresh("t"));
    }

    #[test]
    fn lazy_profile_high_refresh_threshold() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::lazy(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_stale_managed(10_000, 2_000),
        );
        assert!(!model.should_refresh("t"));
    }

    #[test]
    fn stale_profile_very_high_threshold() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::stale(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_stale_managed(10_000, 5_000),
        );
        assert!(!model.should_refresh("t"));
    }

    #[test]
    fn analytical_profile_characteristics() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::analytical(),
            HardwareProfile::cpu_only(),
        );
        assert_eq!(model.profile().name, "Analytical");
        assert!(model.profile().multi_column_stats);
        assert!(model.profile().correlation_stats);
    }

    #[test]
    fn streaming_profile_characteristics() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::streaming(),
            HardwareProfile::cpu_only(),
        );
        assert_eq!(model.profile().name, "Streaming");
        assert!(model.profile().use_sketches);
    }

    // ---- Hardware profiles affect costs ----

    #[test]
    fn gpu_server_profile_in_model() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::gpu_server(),
        );
        assert!(model.hardware().gpu_available);
    }

    #[test]
    fn fpga_profile_in_model() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::fpga_appliance(),
        );
        assert!(model.hardware().fpga_available);
    }

    // ---- Stale statistics inflate costs ----

    #[test]
    fn stale_stats_increase_scan_cost() {
        let hw = HardwareProfile::cpu_only();

        let mut model_fresh = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        model_fresh.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );

        let mut model_stale = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        model_stale.add_table(
            "t".into(),
            make_stale_managed(100_000, 50_000),
        );

        assert!(model_stale.scan_cost("t") > model_fresh.scan_cost("t"));
    }

    #[test]
    fn stale_stats_increase_join_cost() {
        let hw = HardwareProfile::cpu_only();

        let mut model_fresh = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        model_fresh.add_table(
            "a".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );
        model_fresh.add_table(
            "b".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        let mut model_stale = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        model_stale.add_table(
            "a".into(),
            make_stale_managed(100_000, 50_000),
        );
        model_stale.add_table(
            "b".into(),
            make_stale_managed(10_000, 5_000),
        );

        assert!(
            model_stale.join_cost("a", "b")
                > model_fresh.join_cost("a", "b")
        );
    }

    // ---- Low confidence increases costs ----

    #[test]
    fn low_confidence_increases_scan_cost() {
        let hw = HardwareProfile::cpu_only();

        let mut model_high = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        model_high.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );

        let mut model_low = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        model_low.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::Default),
        );

        assert!(
            model_low.scan_cost("t") > model_high.scan_cost("t")
        );
    }

    // ---- Sampled statistics ----

    #[test]
    fn sampled_stats_moderate_confidence() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(
                100_000,
                StatisticsSource::Sampled { sample_rate: 10 },
            ),
        );
        let qm = model.quality_metrics("t").expect("exists");
        assert!(qm.confidence < 1.0);
        assert!(qm.confidence > 0.0);
    }

    // ---- Multiple tables ----

    #[test]
    fn multiple_tables_independent_staleness() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "fresh".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        model.add_table(
            "stale".into(),
            make_stale_managed(10_000, 5_000),
        );

        assert_eq!(model.staleness("fresh"), Staleness::Fresh);
        assert_eq!(model.staleness("stale"), Staleness::VeryStale);
    }

    #[test]
    fn table_count_tracks_additions() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        assert_eq!(model.table_count(), 0);
        model.add_table(
            "a".into(),
            make_managed(1000, StatisticsSource::ExactCount),
        );
        assert_eq!(model.table_count(), 1);
        model.add_table(
            "b".into(),
            make_managed(2000, StatisticsSource::ExactCount),
        );
        assert_eq!(model.table_count(), 2);
    }

    // ---- Edge cases ----

    #[test]
    fn zero_row_table_cost() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "empty".into(),
            make_managed(0, StatisticsSource::ExactCount),
        );
        let cost = model.scan_cost("empty");
        assert!(cost >= 0.0);
        assert!(cost.is_finite());
    }

    #[test]
    fn very_large_table_cost() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "huge".into(),
            make_managed(1_000_000_000, StatisticsSource::ExactCount),
        );
        let cost = model.scan_cost("huge");
        assert!(cost > 0.0);
        assert!(cost.is_finite());
    }

    #[test]
    fn sort_cost_single_row() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "one".into(),
            make_managed(1, StatisticsSource::ExactCount),
        );
        let cost = model.sort_cost("one");
        assert!(cost >= 0.0);
        assert!(cost.is_finite());
    }

    #[test]
    fn aggregate_cost_zero_groups() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        let cost = model.aggregate_cost("t", 0.0);
        assert!(cost >= 0.0);
    }

    // ---- Staleness ordering ----

    #[test]
    fn staleness_factors_are_monotonic() {
        let fresh = staleness_factor(Staleness::Fresh);
        let slight = staleness_factor(Staleness::SlightlyStale);
        let moderate = staleness_factor(Staleness::ModeratelyStale);
        let very = staleness_factor(Staleness::VeryStale);
        let unknown = staleness_factor(Staleness::Unknown);

        assert!(fresh <= slight);
        assert!(slight <= moderate);
        assert!(moderate <= very);
        assert!(very <= unknown);
    }
}
