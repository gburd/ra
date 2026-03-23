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
            Statistics::new(DEFAULT_ROW_COUNT)
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

    /// Estimate cost for a hash join with a runtime filter applied.
    ///
    /// Models the reduced probe-side cost when a bloom/min-max/in-list
    /// filter is built during the hash join build phase and pushed to
    /// the probe-side scan. The `filter_selectivity` is the estimated
    /// fraction of probe rows that pass the filter (0.0 = all
    /// filtered, 1.0 = nothing filtered).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn join_cost_with_runtime_filter(
        &self,
        build_table: &str,
        probe_table: &str,
        filter_selectivity: f64,
    ) -> f64 {
        let build_stats = self.effective_statistics(build_table);
        let probe_stats = self.effective_statistics(probe_table);

        let cache_mb =
            self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
        let cache_factor = 16.0 / cache_mb.max(1.0);

        let build_rows = build_stats.row_count;
        let probe_rows = probe_stats.row_count;

        // Filter build cost: proportional to build side
        let filter_build_cost = build_rows * 10e-9;
        // Filter apply cost: per probe row
        let filter_apply_cost = probe_rows * 20e-9;
        // Effective probe rows after filtering
        let sel = filter_selectivity.clamp(0.0, 1.0);
        let effective_probe = probe_rows * sel;

        let join_cost = (build_rows * 100e-6
            + effective_probe * 50e-6)
            * cache_factor;

        let total = join_cost + filter_build_cost + filter_apply_cost;

        let disc_build = self.confidence_for_table(build_table);
        let disc_probe = self.confidence_for_table(probe_table);
        total * disc_build.max(disc_probe)
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

    /// Estimate cost for an incremental sort operator.
    ///
    /// The cost model assumes the input is already sorted by prefix
    /// columns with `prefix_ndv` distinct values. Only the suffix
    /// columns within each group need sorting.
    ///
    /// Cost = groups * (group_size * log(group_size)) * per_row_factor.
    #[must_use]
    pub fn incremental_sort_cost(
        &self,
        table: &str,
        prefix_ndv: f64,
    ) -> f64 {
        let stats = self.effective_statistics(table);
        let n = stats.row_count.max(1.0);
        let groups = prefix_ndv.max(1.0).min(n);
        let avg_group_size = n / groups;

        let group_sort = avg_group_size
            * avg_group_size.log2().max(1.0);
        let total = groups * group_sort;

        let par_factor =
            8.0 / f64::from(self.hardware.cpu_cores).max(1.0);
        let cost = total * 200e-9 * par_factor.max(0.5);

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

    /// Estimate cost for a covering index (index-only) scan.
    ///
    /// Eliminates heap fetches by reading all needed columns from the
    /// index.  Cost is approximately 30% of a regular scan.
    #[must_use]
    pub fn covering_index_scan_cost(&self, table: &str) -> f64 {
        self.scan_cost(table) * 0.3
    }

    /// Estimate cost for a bitmap index scan.
    ///
    /// Cost includes:
    /// 1. Index scan to build bitmap (random I/O)
    /// 2. Bitmap construction overhead
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn bitmap_index_scan_cost(
        &self,
        table: &str,
        selectivity: f64,
    ) -> f64 {
        let stats = self.effective_statistics(table);
        let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;

        // Index scan cost (random I/O)
        let index_pages = (stats.row_count * selectivity / 100.0).max(1.0);
        let index_cost = index_pages * storage_factor * 0.3;

        // Bitmap construction (CPU cost, very cheap)
        let bitmap_cost = stats.row_count / 64.0 * 1e-9;

        let disc = self.confidence_for_table(table);
        (index_cost + bitmap_cost) * disc
    }

    /// Estimate cost for combining bitmaps with AND/OR.
    ///
    /// Bitwise operations run at memory bandwidth speed.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn bitmap_combine_cost(
        &self,
        table: &str,
        num_bitmaps: usize,
    ) -> f64 {
        let stats = self.effective_statistics(table);
        // Bitmap size in 64-bit words
        let bitmap_words = (stats.row_count / 64.0).max(1.0);
        // AND/OR operations are extremely fast (memory bandwidth)
        let ops_per_bitmap = bitmap_words * 1e-10;
        ops_per_bitmap * num_bitmaps as f64
    }

    /// Estimate cost for bitmap heap scan.
    ///
    /// After combining bitmaps, heap pages are accessed in physical
    /// order, which is much cheaper than random access.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn bitmap_heap_scan_cost(
        &self,
        table: &str,
        combined_selectivity: f64,
    ) -> f64 {
        let stats = self.effective_statistics(table);

        // Sequential page cost (much cheaper than random)
        let pages_accessed = (stats.row_count * combined_selectivity / 100.0).max(1.0);
        let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;

        // Sequential access is ~4x faster than random
        let heap_cost = pages_accessed * storage_factor * 0.25;

        // Recheck condition overhead (CPU)
        let recheck_cost = stats.row_count * combined_selectivity * 5e-9;

        let disc = self.confidence_for_table(table);
        (heap_cost + recheck_cost) * disc
    }

    /// Estimate total cost for a bitmap scan with multiple predicates.
    ///
    /// This combines index scan costs, bitmap combine cost, and heap
    /// scan cost. Returns the total cost and whether bitmap scan is
    /// cheaper than alternatives.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn full_bitmap_scan_cost(
        &self,
        table: &str,
        selectivities: &[f64],
    ) -> f64 {
        if selectivities.is_empty() {
            return self.scan_cost(table);
        }

        // Cost of individual index scans
        let index_costs: f64 = selectivities
            .iter()
            .map(|&sel| self.bitmap_index_scan_cost(table, sel))
            .sum();

        // Cost of combining bitmaps
        let combine_cost = self.bitmap_combine_cost(table, selectivities.len());

        // Combined selectivity (product for AND)
        let combined_sel: f64 = selectivities.iter().product();

        // Heap scan cost with combined selectivity
        let heap_cost = self.bitmap_heap_scan_cost(table, combined_sel);

        index_costs + combine_cost + heap_cost
    }

    /// Estimate cost for a Parquet scan with row group pruning.
    ///
    /// The `pruning_selectivity` is the fraction of row groups that
    /// survive predicate pushdown (0.0 = all pruned, 1.0 = full
    /// scan). This discounts the base scan cost proportionally.
    #[must_use]
    pub fn parquet_scan_cost(
        &self,
        table: &str,
        pruning_selectivity: f64,
    ) -> f64 {
        let base = self.scan_cost(table);
        let sel = pruning_selectivity.clamp(0.0, 1.0);
        // Even with full pruning there's a small metadata cost
        let metadata_overhead = base * 0.01;
        base * sel + metadata_overhead
    }

    /// Adjust an operator cost for startup cost optimization when
    /// LIMIT is present.
    ///
    /// When a query has `LIMIT n`, we only need a prefix of the
    /// output. Plans with low startup cost (streaming operators)
    /// should be preferred over plans with high startup cost
    /// (blocking operators like sort or hash join build).
    ///
    /// `total_cost`: the full cost of the child plan.
    /// `limit_rows`: the LIMIT value.
    /// `estimated_total_rows`: estimated total rows without LIMIT.
    ///
    /// Returns the adjusted cost accounting for early termination.
    #[must_use]
    pub fn limit_adjusted_cost(
        &self,
        total_cost: f64,
        limit_rows: f64,
        estimated_total_rows: f64,
    ) -> f64 {
        if estimated_total_rows <= 0.0 || limit_rows <= 0.0 {
            return total_cost;
        }
        let fraction =
            (limit_rows / estimated_total_rows).clamp(0.0, 1.0);
        // Even with a tiny LIMIT, there is a minimum startup cost
        // of ~10% of the total (hash table build, sort init, etc.)
        let startup_floor = 0.1;
        let effective_fraction = fraction.max(startup_floor);
        total_cost * effective_fraction
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

    /// Apply execution feedback to adjust confidence and staleness.
    ///
    /// For each feedback entry, computes the Q-error between estimated
    /// and actual rows. High Q-errors reduce confidence in the
    /// affected table's statistics, signaling that re-analysis may
    /// be warranted.
    ///
    /// Confidence adjustments:
    /// - Q-error <= 1.5: no change (acceptable estimate)
    /// - Q-error <= 3.0: reduce confidence by 10%
    /// - Q-error <= 10.0: reduce confidence by 25%
    /// - Q-error > 10.0: reduce confidence by 50%
    ///
    /// Returns the number of tables whose confidence was adjusted.
    pub fn apply_execution_feedback(
        &mut self,
        feedback: &[ra_stats::timeline::ExecutionFeedback],
    ) -> usize {
        let mut adjusted_tables =
            std::collections::HashSet::<String>::new();

        for fb in feedback {
            let q_err = fb.q_error();

            let reduction = if q_err <= 1.5 {
                0.0
            } else if q_err <= 3.0 {
                0.10
            } else if q_err <= 10.0 {
                0.25
            } else {
                0.50
            };

            if reduction == 0.0 {
                continue;
            }

            // Extract table name from operator field or query.
            let table_name = fb
                .operator
                .as_deref()
                .and_then(extract_table_from_operator)
                .or_else(|| extract_table_from_query(&fb.query));

            if let Some(name) = table_name {
                if let Some(managed) =
                    self.adapter.get_table_stats_mut(&name)
                {
                    managed.state.confidence =
                        (managed.state.confidence - reduction).max(0.0);
                    adjusted_tables.insert(name);
                }
            }
        }

        adjusted_tables.len()
    }

    // ===== Parallel Query Execution Cost Functions =====

    /// Estimate cost for a parallel scan operator.
    ///
    /// Distributes the scan across multiple workers, reducing wall-clock
    /// time but increasing total resource consumption due to coordination.
    #[must_use]
    pub fn parallel_scan_cost(&self, table: &str, workers: usize) -> f64 {
        let seq_cost = self.scan_cost(table);
        let parallel_speedup = self.parallel_efficiency(workers);

        // Parallel scan cost = sequential cost / speedup + coordination overhead
        let parallel_cost = seq_cost / parallel_speedup;
        let coordination_overhead = self.parallel_coordination_cost(workers);

        parallel_cost + coordination_overhead
    }

    /// Calculate parallel efficiency based on Amdahl's law.
    ///
    /// Models diminishing returns from parallelism due to:
    /// - Serial portions that can't be parallelized
    /// - Coordination overhead between workers
    /// - Resource contention (memory bandwidth, cache)
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn parallel_efficiency(&self, workers: usize) -> f64 {
        if workers <= 1 {
            return 1.0;
        }

        let workers_f = workers as f64;

        // Amdahl's law: speedup = 1 / (s + p/n)
        // where s = serial fraction, p = parallel fraction, n = workers
        let serial_fraction = 0.05;  // 5% of work is inherently serial
        let parallel_fraction = 1.0 - serial_fraction;

        // Theoretical speedup from Amdahl's law
        let amdahl_speedup = 1.0 / (serial_fraction + parallel_fraction / workers_f);

        // Additional efficiency losses
        let coordination_factor = 0.95_f64.powf(workers_f - 1.0);  // 5% loss per worker
        let contention_factor = (1.0 - 0.1 * (workers_f - 1.0).min(5.0)).max(0.5);  // Up to 50% loss

        // Combined efficiency
        amdahl_speedup * coordination_factor * contention_factor
    }

    /// Estimate coordination overhead for parallel execution.
    ///
    /// Includes costs for:
    /// - Worker startup and shutdown
    /// - Synchronization barriers
    /// - Result gathering
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn parallel_coordination_cost(&self, workers: usize) -> f64 {
        if workers <= 1 {
            return 0.0;
        }

        let workers_f = workers as f64;

        // Fixed startup cost per worker (in microseconds)
        let startup_cost = 1000.0 * workers_f;

        // Synchronization cost grows with workers
        let sync_cost = 100.0 * workers_f * workers_f.log2();

        // Gathering cost (merging results)
        let gather_cost = 50.0 * workers_f;

        // Convert to normalized cost units
        (startup_cost + sync_cost + gather_cost) * 1e-6
    }

    /// Estimate cost for a parallel hash join.
    ///
    /// Build phase creates a shared hash table (sequential).
    /// Probe phase is parallelized across workers.
    #[must_use]
    pub fn parallel_hash_join_cost(
        &self,
        build_table: &str,
        probe_table: &str,
        workers: usize,
    ) -> f64 {
        let build_stats = self.effective_statistics(build_table);
        let probe_stats = self.effective_statistics(probe_table);

        // Build phase is sequential (shared hash table)
        let build_cost = build_stats.row_count * 100e-6;

        // Probe phase is parallelized
        let sequential_probe_cost = probe_stats.row_count * 50e-6;
        let parallel_speedup = self.parallel_efficiency(workers);
        let parallel_probe_cost = sequential_probe_cost / parallel_speedup;

        // Add coordination overhead
        let coordination_overhead = self.parallel_coordination_cost(workers);

        // Total cost
        let total = build_cost + parallel_probe_cost + coordination_overhead;

        // Apply confidence discount
        let disc_build = self.confidence_for_table(build_table);
        let disc_probe = self.confidence_for_table(probe_table);
        total * disc_build.max(disc_probe)
    }

    /// Estimate cost for parallel aggregation.
    ///
    /// Uses two-phase aggregation:
    /// 1. Partial aggregation in each worker
    /// 2. Final aggregation to combine partial results
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn parallel_aggregate_cost(
        &self,
        table: &str,
        group_count: f64,
        workers: usize,
    ) -> f64 {
        let stats = self.effective_statistics(table);
        let input_rows = stats.row_count;

        // Phase 1: Partial aggregation in each worker
        let rows_per_worker = input_rows / workers as f64;
        let groups_per_worker = (group_count / workers as f64).max(1.0).min(rows_per_worker);

        // Cost of partial aggregation (parallelized)
        let partial_cost = rows_per_worker * 80e-9;
        let parallel_speedup = self.parallel_efficiency(workers);
        let parallel_partial_cost = partial_cost / parallel_speedup;

        // Phase 2: Final aggregation (combining partial results)
        let combine_rows = groups_per_worker * workers as f64;
        let combine_cost = combine_rows * 100e-9;

        // Add coordination overhead
        let coordination_overhead = self.parallel_coordination_cost(workers);

        // Total cost
        let cost = parallel_partial_cost + combine_cost + coordination_overhead;

        // Apply confidence discount
        let disc = self.confidence_for_table(table);
        cost * disc
    }

    /// Determine optimal number of workers for a parallel operation.
    ///
    /// Balances speedup against coordination overhead.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn optimal_worker_count(
        &self,
        estimated_rows: f64,
        max_workers: usize,
    ) -> usize {
        // No parallelism for small inputs
        if estimated_rows < 10_000.0 {
            return 1;
        }

        let cpu_cores = self.hardware.cpu_cores as usize;

        // Scale workers based on input size
        // 1 worker per 100k rows, up to CPU core count
        let size_based = (estimated_rows / 100_000.0).ceil() as usize;

        // Don't exceed hardware limits or configured maximum
        size_based.min(cpu_cores).min(max_workers).max(1)
    }
}

/// Extract a table name from an operator description like
/// `SeqScan on lineitem` or `Index Scan on orders`.
fn extract_table_from_operator(operator: &str) -> Option<String> {
    let lower = operator.to_lowercase();
    if let Some(pos) = lower.find(" on ") {
        let after = &operator[pos + 4..];
        let name = after
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .next()?;
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    } else {
        None
    }
}

/// Extract the first table name from a SQL query's FROM clause.
fn extract_table_from_query(query: &str) -> Option<String> {
    let lower = query.to_lowercase();
    let from_pos = lower.find(" from ")?;
    let after = &query[from_pos + 6..];
    let trimmed = after.trim_start();
    let name = trimmed
        .split(|c: char| c.is_whitespace() || c == ',' || c == '(')
        .next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
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

/// Hardware cost calibration coefficients.
///
/// Derived from a hardware profile, these coefficients adjust the
/// raw cost model to account for real hardware characteristics.
/// The calibration normalizes costs to a "reference" machine
/// (8 cores, 16 MB L3, 256-bit SIMD, 3.5 GB/s storage).
#[derive(Debug, Clone)]
pub struct CostCalibration {
    /// Scan cost multiplier (lower = faster storage).
    pub scan_factor: f64,
    /// Filter cost multiplier (lower = wider SIMD).
    pub filter_factor: f64,
    /// Join cost multiplier (lower = bigger cache).
    pub join_factor: f64,
    /// Sort cost multiplier (lower = more cores).
    pub sort_factor: f64,
    /// Aggregate cost multiplier (lower = bigger cache).
    pub aggregate_factor: f64,
    /// Whether GPU acceleration is available.
    pub gpu_available: bool,
    /// Whether FPGA acceleration is available.
    pub fpga_available: bool,
}

impl CostCalibration {
    /// Calibrate from a hardware profile.
    ///
    /// Reference machine: 8 cores, 16 MB L3 cache, 256-bit SIMD,
    /// 3.5 GB/s storage bandwidth.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn from_hardware(hw: &HardwareProfile) -> Self {
        let ref_storage_bw = 3.5;
        let ref_simd_bits = 256.0;
        let ref_cache_mb = 16.0;
        let ref_cores = 8.0;

        let storage_bw = hw.storage_bandwidth_gbps.max(0.01);
        let simd_bits = f64::from(hw.simd_width_bits).max(1.0);
        let cache_mb =
            (hw.l3_cache_bytes as f64 / (1024.0 * 1024.0)).max(1.0);
        let cores = f64::from(hw.cpu_cores).max(1.0);

        Self {
            scan_factor: ref_storage_bw / storage_bw,
            filter_factor: ref_simd_bits / simd_bits,
            join_factor: ref_cache_mb / cache_mb,
            sort_factor: (ref_cores / cores).max(0.5),
            aggregate_factor: ref_cache_mb / cache_mb,
            gpu_available: hw.gpu_available,
            fpga_available: hw.fpga_available,
        }
    }

    /// Return calibration for the reference machine (all factors 1.0).
    #[must_use]
    pub fn reference() -> Self {
        Self {
            scan_factor: 1.0,
            filter_factor: 1.0,
            join_factor: 1.0,
            sort_factor: 1.0,
            aggregate_factor: 1.0,
            gpu_available: false,
            fpga_available: false,
        }
    }

    /// Overall speedup relative to the reference machine.
    ///
    /// Values < 1.0 indicate faster hardware; > 1.0 slower.
    #[must_use]
    pub fn overall_factor(&self) -> f64 {
        (self.scan_factor
            + self.filter_factor
            + self.join_factor
            + self.sort_factor
            + self.aggregate_factor)
            / 5.0
    }
}

/// Extended cost function for egg that uses integrated statistics
/// and hardware information.
///
/// Replaces the basic `RelCostFn` when full stats/hardware integration
/// is desired.
#[derive(Debug, Clone)]
pub struct IntegratedCostFn {
    hardware: HardwareProfile,
    table_stats: std::sync::Arc<HashMap<String, Statistics>>,
    staleness_map: std::sync::Arc<HashMap<String, Staleness>>,
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
            table_stats: std::sync::Arc::new(table_stats),
            staleness_map: std::sync::Arc::new(staleness_map),
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
            table_stats: std::sync::Arc::new(table_stats),
            staleness_map: std::sync::Arc::new(staleness_map),
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
            RelLang::IncrementalSort(_) => {
                let par_factor =
                    8.0 / f64::from(self.hardware.cpu_cores);
                60.0 * par_factor.max(0.5)
            }
            RelLang::Limit([n_id, _off_id, child_id]) => {
                // Startup cost optimization: when LIMIT is present
                // we only need a prefix of the child output. Plans
                // with low startup cost (streaming operators like
                // index scans, nested-loop joins) are preferred over
                // plans with high startup cost (sort, hash join build).
                //
                // Model: effective_cost = limit_overhead + n_cost
                //        + child_cost * startup_fraction
                //
                // The 0.3 fraction means LIMIT pays ~30% of the full
                // child cost, biasing toward cheaper-to-start plans.
                let child_cost = costs(*child_id);
                let n_cost = costs(*n_id);
                let startup_fraction = 0.3;
                return 0.5 + n_cost + child_cost * startup_fraction;
            }
            RelLang::Union(_)
            | RelLang::Intersect(_)
            | RelLang::Except(_) => 50.0,
            RelLang::RecursiveCTE(_) => {
                #[allow(clippy::cast_precision_loss)]
                let cache_mb = self.hardware.l3_cache_bytes as f64
                    / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb.max(1.0);
                1000.0 * cache_factor
            }
            RelLang::BitmapIndexScan(_) => {
                // Index scan cost (random I/O, cheaper than full index)
                10.0
            }
            RelLang::BitmapAnd(_) | RelLang::BitmapOr(_) => {
                // Bitwise operations are extremely cheap
                0.1
            }
            RelLang::BitmapHeapScan(_) => {
                // Sequential heap access
                let storage_factor =
                    100.0 / self.hardware.storage_bandwidth_gbps;
                5.0 * storage_factor
            }
            RelLang::MetadataLookup(_) => {
                // O(1) metadata lookup, cheaper than any scan
                return 1.0;
            }
            RelLang::IndexOnlyScan([table_id, _, _, _]) => {
                // Index-only scan: ~30% of full scan cost (no heap
                // fetch).
                let child_cost = costs(*table_id);
                let storage_factor =
                    100.0 / self.hardware.storage_bandwidth_gbps;
                return child_cost + (30.0 * storage_factor);
            }
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

    // ---- CostCalibration ----

    #[test]
    fn calibration_reference_all_ones() {
        let cal = CostCalibration::reference();
        assert_eq!(cal.scan_factor, 1.0);
        assert_eq!(cal.filter_factor, 1.0);
        assert_eq!(cal.join_factor, 1.0);
        assert_eq!(cal.sort_factor, 1.0);
        assert_eq!(cal.aggregate_factor, 1.0);
        assert!(!cal.gpu_available);
        assert!(!cal.fpga_available);
    }

    #[test]
    fn calibration_reference_overall_one() {
        let cal = CostCalibration::reference();
        assert!((cal.overall_factor() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn calibration_cpu_only() {
        let cal = CostCalibration::from_hardware(
            &HardwareProfile::cpu_only(),
        );
        assert!(cal.scan_factor > 0.0);
        assert!(cal.filter_factor > 0.0);
        assert!(cal.join_factor > 0.0);
        assert!(cal.sort_factor > 0.0);
        assert!(cal.aggregate_factor > 0.0);
        assert!(!cal.gpu_available);
    }

    #[test]
    fn calibration_gpu_server_has_gpu() {
        let cal = CostCalibration::from_hardware(
            &HardwareProfile::gpu_server(),
        );
        assert!(cal.gpu_available);
    }

    #[test]
    fn calibration_fpga_has_fpga() {
        let cal = CostCalibration::from_hardware(
            &HardwareProfile::fpga_appliance(),
        );
        assert!(cal.fpga_available);
    }

    #[test]
    fn calibration_fast_storage_lowers_scan_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.storage_bandwidth_gbps = 14.0;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.scan_factor < 1.0);
    }

    #[test]
    fn calibration_slow_storage_raises_scan_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.storage_bandwidth_gbps = 0.15;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.scan_factor > 1.0);
    }

    #[test]
    fn calibration_wide_simd_lowers_filter_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.simd_width_bits = 512;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.filter_factor < 1.0);
    }

    #[test]
    fn calibration_narrow_simd_raises_filter_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.simd_width_bits = 128;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.filter_factor > 1.0);
    }

    #[test]
    fn calibration_big_cache_lowers_join_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.l3_cache_bytes = 128 * 1024 * 1024;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.join_factor < 1.0);
    }

    #[test]
    fn calibration_small_cache_raises_join_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.l3_cache_bytes = 4 * 1024 * 1024;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.join_factor > 1.0);
    }

    #[test]
    fn calibration_many_cores_lowers_sort_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.cpu_cores = 64;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.sort_factor < 1.0);
    }

    #[test]
    fn calibration_few_cores_raises_sort_factor() {
        let mut hw = HardwareProfile::cpu_only();
        hw.cpu_cores = 2;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.sort_factor > 1.0);
    }

    #[test]
    fn calibration_sort_factor_has_minimum() {
        let mut hw = HardwareProfile::cpu_only();
        hw.cpu_cores = 255;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.sort_factor >= 0.5);
    }

    #[test]
    fn calibration_overall_faster_machine() {
        let mut hw = HardwareProfile::cpu_only();
        hw.storage_bandwidth_gbps = 14.0;
        hw.simd_width_bits = 512;
        hw.l3_cache_bytes = 128 * 1024 * 1024;
        hw.cpu_cores = 64;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.overall_factor() < 1.0);
    }

    #[test]
    fn calibration_overall_slower_machine() {
        let mut hw = HardwareProfile::cpu_only();
        hw.storage_bandwidth_gbps = 0.15;
        hw.simd_width_bits = 128;
        hw.l3_cache_bytes = 2 * 1024 * 1024;
        hw.cpu_cores = 2;
        let cal = CostCalibration::from_hardware(&hw);
        assert!(cal.overall_factor() > 1.0);
    }

    // ---- IntegratedCostFn row_count_for staleness levels ----

    #[test]
    fn cost_fn_row_count_slightly_stale() {
        let mut stats = HashMap::new();
        stats.insert("t".into(), Statistics::new(10_000.0));
        let mut staleness = HashMap::new();
        staleness.insert("t".into(), Staleness::SlightlyStale);

        let cfn = IntegratedCostFn::new(
            HardwareProfile::cpu_only(),
            stats,
            staleness,
        );
        let rows = cfn.row_count_for("t");
        assert!((rows - 10_500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_fn_row_count_moderately_stale() {
        let mut stats = HashMap::new();
        stats.insert("t".into(), Statistics::new(10_000.0));
        let mut staleness = HashMap::new();
        staleness.insert("t".into(), Staleness::ModeratelyStale);

        let cfn = IntegratedCostFn::new(
            HardwareProfile::cpu_only(),
            stats,
            staleness,
        );
        let rows = cfn.row_count_for("t");
        assert!((rows - 12_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_fn_row_count_very_stale() {
        let mut stats = HashMap::new();
        stats.insert("t".into(), Statistics::new(10_000.0));
        let mut staleness = HashMap::new();
        staleness.insert("t".into(), Staleness::VeryStale);

        let cfn = IntegratedCostFn::new(
            HardwareProfile::cpu_only(),
            stats,
            staleness,
        );
        let rows = cfn.row_count_for("t");
        assert!((rows - 15_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_fn_missing_staleness_uses_unknown() {
        let mut stats = HashMap::new();
        stats.insert("t".into(), Statistics::new(10_000.0));

        let cfn = IntegratedCostFn::new(
            HardwareProfile::cpu_only(),
            stats,
            HashMap::new(),
        );
        let rows = cfn.row_count_for("t");
        assert!((rows - 20_000.0).abs() < f64::EPSILON);
    }

    // ---- Cost ordering: larger tables cost more ----

    #[test]
    fn scan_cost_scales_with_row_count() {
        let hw = HardwareProfile::cpu_only();
        let mut small = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        small.add_table(
            "t".into(),
            make_managed(1_000, StatisticsSource::ExactCount),
        );

        let mut large = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        large.add_table(
            "t".into(),
            make_managed(1_000_000, StatisticsSource::ExactCount),
        );

        assert!(large.scan_cost("t") > small.scan_cost("t"));
    }

    #[test]
    fn filter_cost_scales_with_row_count() {
        let hw = HardwareProfile::cpu_only();
        let mut small = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        small.add_table(
            "t".into(),
            make_managed(1_000, StatisticsSource::ExactCount),
        );

        let mut large = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        large.add_table(
            "t".into(),
            make_managed(1_000_000, StatisticsSource::ExactCount),
        );

        assert!(large.filter_cost("t") > small.filter_cost("t"));
    }

    #[test]
    fn sort_cost_scales_with_row_count() {
        let hw = HardwareProfile::cpu_only();
        let mut small = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        small.add_table(
            "t".into(),
            make_managed(1_000, StatisticsSource::ExactCount),
        );

        let mut large = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        large.add_table(
            "t".into(),
            make_managed(1_000_000, StatisticsSource::ExactCount),
        );

        assert!(large.sort_cost("t") > small.sort_cost("t"));
    }

    // ---- Cross-cutting: all profiles produce valid costs ----

    #[test]
    fn all_profiles_produce_finite_scan_costs() {
        let profiles = [
            StatisticsProfile::real_time(),
            StatisticsProfile::standard(),
            StatisticsProfile::lazy(),
            StatisticsProfile::stale(),
            StatisticsProfile::analytical(),
            StatisticsProfile::streaming(),
        ];
        for profile in profiles {
            let mut model = IntegratedCostModel::new(
                profile,
                HardwareProfile::cpu_only(),
            );
            model.add_table(
                "t".into(),
                make_managed(
                    50_000,
                    StatisticsSource::ExactCount,
                ),
            );
            let cost = model.scan_cost("t");
            assert!(cost > 0.0, "scan cost must be positive");
            assert!(cost.is_finite(), "scan cost must be finite");
        }
    }

    #[test]
    fn all_profiles_produce_finite_join_costs() {
        let profiles = [
            StatisticsProfile::real_time(),
            StatisticsProfile::standard(),
            StatisticsProfile::lazy(),
            StatisticsProfile::stale(),
            StatisticsProfile::analytical(),
            StatisticsProfile::streaming(),
        ];
        for profile in profiles {
            let mut model = IntegratedCostModel::new(
                profile,
                HardwareProfile::cpu_only(),
            );
            model.add_table(
                "a".into(),
                make_managed(
                    10_000,
                    StatisticsSource::ExactCount,
                ),
            );
            model.add_table(
                "b".into(),
                make_managed(
                    5_000,
                    StatisticsSource::ExactCount,
                ),
            );
            let cost = model.join_cost("a", "b");
            assert!(cost > 0.0);
            assert!(cost.is_finite());
        }
    }

    // ---- Cross-cutting: all hardware profiles ----

    #[test]
    fn all_hardware_profiles_produce_valid_costs() {
        let profiles = [
            HardwareProfile::cpu_only(),
            HardwareProfile::gpu_server(),
            HardwareProfile::fpga_appliance(),
        ];
        for hw in profiles {
            let mut model = IntegratedCostModel::new(
                StatisticsProfile::standard(),
                hw,
            );
            model.add_table(
                "t".into(),
                make_managed(
                    10_000,
                    StatisticsSource::ExactCount,
                ),
            );
            assert!(model.scan_cost("t") > 0.0);
            assert!(model.filter_cost("t") > 0.0);
            assert!(model.sort_cost("t") > 0.0);
            assert!(model.aggregate_cost("t", 100.0) > 0.0);
        }
    }

    // ---- from_core_statistics edge cases ----

    #[test]
    fn from_core_statistics_empty_map() {
        let stats = HashMap::new();
        let model = from_core_statistics(
            &stats,
            &HardwareProfile::cpu_only(),
            StatisticsProfile::standard(),
        );
        assert_eq!(model.table_count(), 0);
    }

    #[test]
    fn from_core_statistics_preserves_row_count() {
        let mut stats = HashMap::new();
        stats.insert(
            "t".into(),
            Statistics::new(42_000.0),
        );
        let model = from_core_statistics(
            &stats,
            &HardwareProfile::cpu_only(),
            StatisticsProfile::standard(),
        );
        let es = model.effective_statistics("t");
        assert!((es.row_count - 42_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn from_core_statistics_tables_are_fresh() {
        let mut stats = HashMap::new();
        stats.insert("t".into(), Statistics::new(1000.0));
        let model = from_core_statistics(
            &stats,
            &HardwareProfile::cpu_only(),
            StatisticsProfile::standard(),
        );
        assert_eq!(model.staleness("t"), Staleness::Fresh);
    }

    #[test]
    fn from_core_statistics_many_tables() {
        let mut stats = HashMap::new();
        for i in 0..20 {
            stats.insert(
                format!("table_{i}"),
                Statistics::new(f64::from(i + 1) * 1000.0),
            );
        }
        let model = from_core_statistics(
            &stats,
            &HardwareProfile::cpu_only(),
            StatisticsProfile::standard(),
        );
        assert_eq!(model.table_count(), 20);
    }

    // ---- Stale stats affect all operator types ----

    #[test]
    fn stale_stats_increase_filter_cost() {
        let hw = HardwareProfile::cpu_only();
        let mut fresh = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        fresh.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );

        let mut stale = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        stale.add_table(
            "t".into(),
            make_stale_managed(100_000, 50_000),
        );

        assert!(stale.filter_cost("t") > fresh.filter_cost("t"));
    }

    #[test]
    fn stale_stats_increase_sort_cost() {
        let hw = HardwareProfile::cpu_only();
        let mut fresh = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        fresh.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );

        let mut stale = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        stale.add_table(
            "t".into(),
            make_stale_managed(100_000, 50_000),
        );

        assert!(stale.sort_cost("t") > fresh.sort_cost("t"));
    }

    #[test]
    fn stale_stats_increase_aggregate_cost() {
        let hw = HardwareProfile::cpu_only();
        let mut fresh = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        fresh.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );

        let mut stale = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        stale.add_table(
            "t".into(),
            make_stale_managed(100_000, 50_000),
        );

        assert!(
            stale.aggregate_cost("t", 100.0)
                > fresh.aggregate_cost("t", 100.0)
        );
    }

    // ---- Low confidence affects all operator types ----

    #[test]
    fn low_confidence_increases_filter_cost() {
        let hw = HardwareProfile::cpu_only();
        let mut high = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        high.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );

        let mut low = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        low.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::Default),
        );

        assert!(low.filter_cost("t") > high.filter_cost("t"));
    }

    #[test]
    fn low_confidence_increases_sort_cost() {
        let hw = HardwareProfile::cpu_only();
        let mut high = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        high.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );

        let mut low = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        low.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::Default),
        );

        assert!(low.sort_cost("t") > high.sort_cost("t"));
    }

    #[test]
    fn low_confidence_increases_join_cost() {
        let hw = HardwareProfile::cpu_only();
        let mut high = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw.clone(),
        );
        high.add_table(
            "a".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );
        high.add_table(
            "b".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        let mut low = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            hw,
        );
        low.add_table(
            "a".into(),
            make_managed(100_000, StatisticsSource::Default),
        );
        low.add_table(
            "b".into(),
            make_managed(10_000, StatisticsSource::Default),
        );

        assert!(
            low.join_cost("a", "b") > high.join_cost("a", "b")
        );
    }

    // ---- IntegratedCostFn from_model round-trip ----

    #[test]
    fn cost_fn_from_model_preserves_staleness() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "fresh".into(),
            make_managed(5000, StatisticsSource::ExactCount),
        );
        model.add_table(
            "stale".into(),
            make_stale_managed(5000, 3000),
        );

        let cfn = IntegratedCostFn::from_model(
            &model,
            &["fresh".into(), "stale".into()],
        );
        assert!(
            cfn.row_count_for("stale")
                > cfn.row_count_for("fresh")
        );
    }

    #[test]
    fn cost_fn_from_model_empty_tables() {
        let model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        let cfn =
            IntegratedCostFn::from_model(&model, &[]);
        let rows = cfn.row_count_for("missing");
        assert!((rows - 2000.0).abs() < f64::EPSILON);
    }

    // ---- Confidence discount boundary tests ----

    #[test]
    fn confidence_discount_at_boundaries() {
        assert_eq!(confidence_discount(0.0), 2.0);
        assert_eq!(confidence_discount(0.25), 1.75);
        assert_eq!(confidence_discount(0.75), 1.25);
        assert_eq!(confidence_discount(1.0), 1.0);
    }

    #[test]
    fn confidence_discount_is_monotonically_decreasing() {
        let values: Vec<f64> =
            (0..=10).map(|i| f64::from(i) / 10.0).collect();
        for window in values.windows(2) {
            assert!(
                confidence_discount(window[0])
                    >= confidence_discount(window[1])
            );
        }
    }

    // ---- Staleness factor consistency ----

    #[test]
    fn staleness_factor_all_positive() {
        let all = [
            Staleness::Fresh,
            Staleness::SlightlyStale,
            Staleness::ModeratelyStale,
            Staleness::VeryStale,
            Staleness::Unknown,
        ];
        for s in all {
            assert!(staleness_factor(s) > 0.0);
        }
    }

    #[test]
    fn staleness_factor_fresh_is_one() {
        assert_eq!(staleness_factor(Staleness::Fresh), 1.0);
    }

    // ---- Replacing existing table stats ----

    // ---- Aggregate cost scales with group count ----

    #[test]
    fn aggregate_cost_more_groups_costs_more() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );
        assert!(
            model.aggregate_cost("t", 10_000.0)
                > model.aggregate_cost("t", 10.0)
        );
    }

    #[test]
    fn join_cost_symmetric_for_same_size() {
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
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        let ab = model.join_cost("a", "b");
        let ba = model.join_cost("b", "a");
        assert!((ab - ba).abs() < f64::EPSILON);
    }

    #[test]
    fn add_table_replaces_existing() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(1_000, StatisticsSource::ExactCount),
        );
        let cost_before = model.scan_cost("t");

        model.add_table(
            "t".into(),
            make_managed(
                1_000_000,
                StatisticsSource::ExactCount,
            ),
        );
        let cost_after = model.scan_cost("t");

        assert!(cost_after > cost_before);
        assert_eq!(model.table_count(), 1);
    }

    // ---- apply_execution_feedback ----

    fn make_feedback(
        estimated: f64,
        actual: f64,
        operator: &str,
    ) -> ra_stats::timeline::ExecutionFeedback {
        ra_stats::timeline::ExecutionFeedback {
            time_offset: 0,
            query: "SELECT * FROM t".to_string(),
            operator: Some(operator.to_string()),
            estimated_rows: estimated,
            actual_rows: actual,
            estimated_cost: None,
            actual_time_ms: None,
        }
    }

    #[test]
    fn feedback_good_estimate_no_adjustment() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );
        let confidence_before = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;

        let feedback = [make_feedback(1000.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);

        assert_eq!(adjusted, 0);
        let confidence_after = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!(
            (confidence_after - confidence_before).abs() < f64::EPSILON
        );
    }

    #[test]
    fn feedback_moderate_error_reduces_confidence() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 2.0, in the (1.5, 3.0] range => 10% reduction
        let feedback =
            [make_feedback(2000.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);

        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_large_error_reduces_confidence_more() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 5.0, in the (3.0, 10.0] range => 25% reduction
        let feedback =
            [make_feedback(5000.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);

        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_extreme_error_halves_confidence() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 20.0 => 50% reduction
        let feedback =
            [make_feedback(20_000.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);

        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_multiple_entries_accumulate() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Two moderate errors: 10% + 10% = confidence goes 1.0 -> 0.8
        let feedback = [
            make_feedback(2000.0, 1000.0, "SeqScan on t"),
            make_feedback(1000.0, 2000.0, "SeqScan on t"),
        ];
        let adjusted = model.apply_execution_feedback(&feedback);

        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_confidence_never_negative() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::Default),
        );

        // Default confidence = 0.3; extreme error reduces by 0.5
        let feedback =
            [make_feedback(100_000.0, 1.0, "SeqScan on t")];
        model.apply_execution_feedback(&feedback);

        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!(confidence >= 0.0);
    }

    #[test]
    fn feedback_unknown_table_ignored() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        let feedback =
            [make_feedback(20_000.0, 1000.0, "SeqScan on unknown")];
        let adjusted = model.apply_execution_feedback(&feedback);

        assert_eq!(adjusted, 0);
    }

    #[test]
    fn feedback_empty_input() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        let adjusted = model.apply_execution_feedback(&[]);
        assert_eq!(adjusted, 0);
    }

    #[test]
    fn feedback_extracts_table_from_query_when_no_operator() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "orders".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        let fb = ra_stats::timeline::ExecutionFeedback {
            time_offset: 0,
            query: "SELECT * FROM orders WHERE id > 10".to_string(),
            operator: None,
            estimated_rows: 5000.0,
            actual_rows: 1000.0,
            estimated_cost: None,
            actual_time_ms: None,
        };
        let adjusted = model.apply_execution_feedback(&[fb]);

        assert_eq!(adjusted, 1);
    }

    #[test]
    fn feedback_multiple_tables() {
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
            make_managed(5_000, StatisticsSource::ExactCount),
        );

        let feedback = [
            make_feedback(5000.0, 1000.0, "SeqScan on a"),
            make_feedback(5000.0, 1000.0, "Index Scan on b"),
        ];
        let adjusted = model.apply_execution_feedback(&feedback);

        assert_eq!(adjusted, 2);
    }

    #[test]
    fn feedback_increases_costs() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(100_000, StatisticsSource::ExactCount),
        );
        let cost_before = model.scan_cost("t");

        let feedback =
            [make_feedback(100_000.0, 10_000.0, "SeqScan on t")];
        model.apply_execution_feedback(&feedback);

        let cost_after = model.scan_cost("t");
        assert!(cost_after > cost_before);
    }

    #[test]
    fn feedback_at_threshold_boundary_1_5() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 1.5 exactly => no adjustment
        let feedback =
            [make_feedback(1500.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);
        assert_eq!(adjusted, 0);
    }

    #[test]
    fn feedback_just_above_threshold_1_5() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 1.6 => 10% reduction
        let feedback =
            [make_feedback(1600.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);
        assert_eq!(adjusted, 1);
    }

    #[test]
    fn feedback_at_threshold_boundary_3_0() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 3.0 exactly => 10% reduction
        let feedback =
            [make_feedback(3000.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);
        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_just_above_threshold_3_0() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 3.1 => 25% reduction
        let feedback =
            [make_feedback(3100.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);
        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_at_threshold_boundary_10_0() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 10.0 exactly => 25% reduction
        let feedback =
            [make_feedback(10_000.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);
        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn feedback_just_above_threshold_10_0() {
        let mut model = IntegratedCostModel::new(
            StatisticsProfile::standard(),
            HardwareProfile::cpu_only(),
        );
        model.add_table(
            "t".into(),
            make_managed(10_000, StatisticsSource::ExactCount),
        );

        // Q-error = 11.0 => 50% reduction
        let feedback =
            [make_feedback(11_000.0, 1000.0, "SeqScan on t")];
        let adjusted = model.apply_execution_feedback(&feedback);
        assert_eq!(adjusted, 1);
        let confidence = model
            .quality_metrics("t")
            .expect("exists")
            .confidence;
        assert!((confidence - 0.5).abs() < f64::EPSILON);
    }

    // ---- extract_table_from_operator ----

    #[test]
    fn extract_table_seq_scan() {
        assert_eq!(
            extract_table_from_operator("SeqScan on lineitem"),
            Some("lineitem".to_string())
        );
    }

    #[test]
    fn extract_table_index_scan() {
        assert_eq!(
            extract_table_from_operator("Index Scan on orders"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn extract_table_with_parenthetical() {
        assert_eq!(
            extract_table_from_operator(
                "Bitmap Heap Scan on users (cost=100)"
            ),
            Some("users".to_string())
        );
    }

    #[test]
    fn extract_table_no_on_clause() {
        assert_eq!(
            extract_table_from_operator("Hash Join"),
            None
        );
    }

    #[test]
    fn extract_table_empty_after_on() {
        assert_eq!(
            extract_table_from_operator("Scan on "),
            None
        );
    }

    // ---- extract_table_from_query ----

    #[test]
    fn extract_table_from_select() {
        assert_eq!(
            extract_table_from_query(
                "SELECT * FROM orders WHERE id > 10"
            ),
            Some("orders".to_string())
        );
    }

    #[test]
    fn extract_table_from_select_with_join() {
        assert_eq!(
            extract_table_from_query(
                "SELECT * FROM lineitem,orders WHERE l_orderkey = o_orderkey"
            ),
            Some("lineitem".to_string())
        );
    }

    #[test]
    fn extract_table_no_from() {
        assert_eq!(
            extract_table_from_query("SELECT 1 + 1"),
            None
        );
    }
}
