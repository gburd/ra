//! Runtime filters for sideways information passing.
//!
//! Implements bloom filters, min/max filters, and in-list filters
//! that pass information between join sides to prune data early.
//! During the hash join build phase, a filter is constructed on
//! the join key; it is then pushed to the probe-side scan to
//! reduce the number of rows read.
//!
//! This technique can achieve 10-100x reduction in probe-side
//! data for star schema joins and other selective join patterns.

use std::collections::HashSet;

/// A runtime filter built during the hash join build phase and
/// applied to the probe-side scan to reduce rows read.
#[derive(Debug, Clone)]
pub enum RuntimeFilter {
    /// Bloom filter: probabilistic membership test on join keys.
    /// False positives possible, false negatives are not.
    BloomFilter(BloomFilterState),
    /// Min/max filter: rejects probe rows whose join key falls
    /// outside the range observed on the build side.
    MinMaxFilter(MinMaxFilterState),
    /// In-list filter: exact membership test for small build
    /// sides (fewer than `MAX_IN_LIST_SIZE` distinct values).
    InListFilter(InListFilterState),
}

/// Maximum number of distinct values for an in-list filter
/// before falling back to a bloom filter.
const MAX_IN_LIST_SIZE: usize = 1024;

/// Default bloom filter false-positive rate.
const DEFAULT_FPR: f64 = 0.01;

/// Number of hash functions per element for the bloom filter.
/// Derived from optimal k = (m/n) * ln(2).
const HASH_FUNCTION_COUNT: u32 = 7;

/// State for a bloom filter runtime filter.
#[derive(Debug, Clone)]
pub struct BloomFilterState {
    /// Bit vector backing the bloom filter.
    bits: Vec<u64>,
    /// Number of bits in the filter.
    num_bits: usize,
    /// Number of hash functions.
    num_hashes: u32,
    /// Number of elements inserted.
    num_elements: usize,
}

impl BloomFilterState {
    /// Create a new bloom filter sized for `expected_elements`
    /// with the default false-positive rate.
    #[must_use]
    pub fn new(expected_elements: usize) -> Self {
        Self::with_fpr(expected_elements, DEFAULT_FPR)
    }

    /// Create a bloom filter with a specific false-positive rate.
    #[must_use]
    pub fn with_fpr(expected_elements: usize, fpr: f64) -> Self {
        let expected = expected_elements.max(1);
        let fpr = fpr.clamp(1e-10, 0.5);
        // m = -n * ln(p) / (ln(2))^2
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let num_bits = (-(expected as f64) * fpr.ln() / (2.0_f64.ln().powi(2))).ceil() as usize;
        let num_bits = num_bits.max(64);
        let words = (num_bits + 63) / 64;
        Self {
            bits: vec![0u64; words],
            num_bits,
            num_hashes: HASH_FUNCTION_COUNT,
            num_elements: 0,
        }
    }

    /// Insert a hashed value into the bloom filter.
    pub fn insert(&mut self, hash: u64) {
        for i in 0..self.num_hashes {
            let idx = self.bit_index(hash, i);
            self.bits[idx / 64] |= 1u64 << (idx % 64);
        }
        self.num_elements += 1;
    }

    /// Test whether a hashed value might be in the filter.
    #[must_use]
    pub fn might_contain(&self, hash: u64) -> bool {
        for i in 0..self.num_hashes {
            let idx = self.bit_index(hash, i);
            if self.bits[idx / 64] & (1u64 << (idx % 64)) == 0 {
                return false;
            }
        }
        true
    }

    /// Number of elements inserted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.num_elements
    }

    /// Whether the filter is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.num_elements == 0
    }

    /// Estimated false-positive rate based on current fill.
    #[must_use]
    pub fn estimated_fpr(&self) -> f64 {
        if self.num_elements == 0 {
            return 0.0;
        }
        let m = self.num_bits as f64;
        let k = f64::from(self.num_hashes);
        let n = self.num_elements as f64;
        (1.0 - (-k * n / m).exp()).powf(k)
    }

    /// Memory usage in bytes.
    #[must_use]
    pub fn memory_bytes(&self) -> usize {
        self.bits.len() * 8
    }

    /// Compute the bit index for a given hash and hash function.
    fn bit_index(&self, hash: u64, i: u32) -> usize {
        // Enhanced double hashing with Kirsch-Mitzenmacker scheme:
        // h(i) = h1 + i*h2 + i^2, which provides better
        // independence between hash functions.
        let h1 = Self::mix(hash) as usize;
        let h2 = Self::mix(hash.wrapping_mul(0x9E37_79B9_7F4A_7C15)) as usize | 1;
        let i_usize = i as usize;
        h1.wrapping_add(h2.wrapping_mul(i_usize))
            .wrapping_add(i_usize.wrapping_mul(i_usize))
            % self.num_bits
    }

    /// Finalizer mix function (splitmix64-style) for better
    /// distribution of sequential inputs.
    fn mix(mut x: u64) -> u64 {
        x ^= x >> 30;
        x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 31;
        x
    }
}

/// State for a min/max range filter.
#[derive(Debug, Clone)]
pub struct MinMaxFilterState {
    /// Minimum observed value (as i64 for integer keys).
    pub min_value: i64,
    /// Maximum observed value.
    pub max_value: i64,
    /// Number of values observed.
    pub count: usize,
}

impl MinMaxFilterState {
    /// Create a new, empty min/max filter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            min_value: i64::MAX,
            max_value: i64::MIN,
            count: 0,
        }
    }

    /// Insert a value into the filter, updating min/max.
    pub fn insert(&mut self, value: i64) {
        if value < self.min_value {
            self.min_value = value;
        }
        if value > self.max_value {
            self.max_value = value;
        }
        self.count += 1;
    }

    /// Test whether a value falls within the observed range.
    #[must_use]
    pub fn might_contain(&self, value: i64) -> bool {
        if self.count == 0 {
            return false;
        }
        value >= self.min_value && value <= self.max_value
    }

    /// Whether the filter has no observations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for MinMaxFilterState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for an in-list (exact membership) filter.
#[derive(Debug, Clone)]
pub struct InListFilterState {
    /// Set of distinct join key values observed on the build side.
    values: HashSet<i64>,
}

impl InListFilterState {
    /// Create a new, empty in-list filter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: HashSet::new(),
        }
    }

    /// Insert a value. Returns `false` if the in-list is full
    /// (exceeds `MAX_IN_LIST_SIZE`) and callers should switch
    /// to a bloom filter.
    pub fn insert(&mut self, value: i64) -> bool {
        self.values.insert(value);
        self.values.len() <= MAX_IN_LIST_SIZE
    }

    /// Test whether a value is in the list.
    #[must_use]
    pub fn contains(&self, value: i64) -> bool {
        self.values.contains(&value)
    }

    /// Number of distinct values in the list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Whether the list has exceeded maximum capacity.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.values.len() > MAX_IN_LIST_SIZE
    }
}

impl Default for InListFilterState {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeFilter {
    /// Test whether a hashed value might pass the filter.
    ///
    /// For bloom and in-list filters, tests membership.
    /// For min/max filters, tests range containment.
    #[must_use]
    pub fn might_contain_hash(&self, hash: u64) -> bool {
        match self {
            RuntimeFilter::BloomFilter(bf) => bf.might_contain(hash),
            RuntimeFilter::MinMaxFilter(mm) => mm.might_contain(hash as i64),
            RuntimeFilter::InListFilter(il) => il.contains(hash as i64),
        }
    }

    /// Whether the filter is empty (no values inserted).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            RuntimeFilter::BloomFilter(bf) => bf.is_empty(),
            RuntimeFilter::MinMaxFilter(mm) => mm.is_empty(),
            RuntimeFilter::InListFilter(il) => il.is_empty(),
        }
    }
}

// ---------------------------------------------------------------
// Filter builder: constructs the appropriate filter type during
// the hash join build phase.
// ---------------------------------------------------------------

/// Strategy for selecting which runtime filter type to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterStrategy {
    /// Always use a bloom filter.
    BloomOnly,
    /// Use in-list when cardinality is low, bloom otherwise.
    Auto,
    /// Use min/max filter (only effective for range predicates).
    MinMax,
}

/// Configuration for building runtime filters.
#[derive(Debug, Clone)]
pub struct FilterConfig {
    /// Which filter strategy to use.
    pub strategy: FilterStrategy,
    /// Expected number of distinct build-side values.
    pub expected_distinct: usize,
    /// Target false-positive rate for bloom filters.
    pub target_fpr: f64,
    /// Minimum selectivity improvement to justify filter cost.
    /// Filter is only applied if estimated selectivity < this.
    pub min_selectivity_benefit: f64,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            strategy: FilterStrategy::Auto,
            expected_distinct: 10_000,
            target_fpr: DEFAULT_FPR,
            min_selectivity_benefit: 0.5,
        }
    }
}

/// Builds a runtime filter from a stream of build-side values.
#[derive(Debug)]
pub struct FilterBuilder {
    config: FilterConfig,
    bloom: BloomFilterState,
    min_max: MinMaxFilterState,
    in_list: InListFilterState,
    switched_to_bloom: bool,
}

impl FilterBuilder {
    /// Create a new filter builder with the given configuration.
    #[must_use]
    pub fn new(config: FilterConfig) -> Self {
        let bloom = BloomFilterState::with_fpr(config.expected_distinct, config.target_fpr);
        Self {
            config,
            bloom,
            min_max: MinMaxFilterState::new(),
            in_list: InListFilterState::new(),
            switched_to_bloom: false,
        }
    }

    /// Insert a build-side join key value.
    ///
    /// The `hash` is the hash of the join key; `raw_value` is the
    /// integer representation for min/max and in-list filters.
    pub fn insert(&mut self, hash: u64, raw_value: i64) {
        self.bloom.insert(hash);
        self.min_max.insert(raw_value);

        if !self.switched_to_bloom {
            let fits = self.in_list.insert(raw_value);
            if !fits {
                self.switched_to_bloom = true;
            }
        }
    }

    /// Finalize the builder and return the best runtime filter.
    #[must_use]
    pub fn build(self) -> RuntimeFilter {
        match self.config.strategy {
            FilterStrategy::BloomOnly => RuntimeFilter::BloomFilter(self.bloom),
            FilterStrategy::MinMax => RuntimeFilter::MinMaxFilter(self.min_max),
            FilterStrategy::Auto => {
                if !self.switched_to_bloom && !self.in_list.is_empty() {
                    RuntimeFilter::InListFilter(self.in_list)
                } else {
                    RuntimeFilter::BloomFilter(self.bloom)
                }
            }
        }
    }

    /// Number of values inserted so far.
    #[must_use]
    pub fn count(&self) -> usize {
        self.bloom.len()
    }
}

// ---------------------------------------------------------------
// Filter effectiveness tracking
// ---------------------------------------------------------------

/// Tracks runtime filter effectiveness during execution.
#[derive(Debug, Clone)]
pub struct FilterEffectiveness {
    /// Total rows evaluated against the filter.
    pub rows_tested: u64,
    /// Rows that passed the filter.
    pub rows_passed: u64,
    /// Join key column name.
    pub column: String,
}

impl FilterEffectiveness {
    /// Create a new effectiveness tracker.
    #[must_use]
    pub fn new(column: String) -> Self {
        Self {
            rows_tested: 0,
            rows_passed: 0,
            column,
        }
    }

    /// Record a filter test result.
    pub fn record(&mut self, passed: bool) {
        self.rows_tested += 1;
        if passed {
            self.rows_passed += 1;
        }
    }

    /// Selectivity: fraction of rows that passed the filter.
    /// Returns 1.0 if no rows have been tested.
    #[must_use]
    pub fn selectivity(&self) -> f64 {
        if self.rows_tested == 0 {
            return 1.0;
        }
        self.rows_passed as f64 / self.rows_tested as f64
    }

    /// Rows filtered out.
    #[must_use]
    pub fn rows_filtered(&self) -> u64 {
        self.rows_tested - self.rows_passed
    }
}

// ---------------------------------------------------------------
// Cost model extension for runtime filters
// ---------------------------------------------------------------

/// Cost estimates for applying a runtime filter.
#[derive(Debug, Clone)]
pub struct RuntimeFilterCost {
    /// Cost to build the filter during hash join build phase.
    pub build_cost: f64,
    /// Cost to apply the filter per probe-side row.
    pub apply_cost_per_row: f64,
    /// Estimated selectivity (fraction of rows passing).
    /// 0.0 = filters everything, 1.0 = filters nothing.
    pub estimated_selectivity: f64,
    /// Estimated reduction in probe-side rows.
    pub estimated_rows_saved: f64,
    /// Net benefit: rows_saved * scan_cost_per_row - build_cost
    /// - apply_cost. Positive means the filter is beneficial.
    pub net_benefit: f64,
}

/// Estimate the cost and benefit of applying a runtime filter.
///
/// `build_side_rows`: number of rows on the hash join build side
/// `probe_side_rows`: number of rows on the probe side
/// `build_side_ndv`: number of distinct values on the build side
/// `probe_side_ndv`: number of distinct values on the probe side
/// `scan_cost_per_row`: cost to read one probe-side row
#[must_use]
pub fn estimate_filter_cost(
    build_side_rows: f64,
    probe_side_rows: f64,
    build_side_ndv: f64,
    probe_side_ndv: f64,
    scan_cost_per_row: f64,
) -> RuntimeFilterCost {
    // Build cost: proportional to build-side rows
    let build_cost = build_side_rows * 10e-9;

    // Apply cost: per-row cost for bloom filter lookup
    let apply_cost_per_row = 20e-9;

    // Selectivity estimate: fraction of probe-side NDV that
    // matches build-side NDV. Conservative estimate.
    let selectivity = if probe_side_ndv > 0.0 {
        (build_side_ndv / probe_side_ndv).min(1.0)
    } else {
        1.0
    };

    let rows_saved = probe_side_rows * (1.0 - selectivity);
    let total_apply_cost = probe_side_rows * apply_cost_per_row;
    let savings = rows_saved * scan_cost_per_row;
    let net_benefit = savings - build_cost - total_apply_cost;

    RuntimeFilterCost {
        build_cost,
        apply_cost_per_row,
        estimated_selectivity: selectivity,
        estimated_rows_saved: rows_saved,
        net_benefit,
    }
}

/// Determine whether a runtime filter is worth applying.
///
/// Returns true if the estimated net benefit is positive and
/// the estimated selectivity is below the threshold.
#[must_use]
pub fn should_apply_filter(cost: &RuntimeFilterCost, selectivity_threshold: f64) -> bool {
    cost.net_benefit > 0.0 && cost.estimated_selectivity < selectivity_threshold
}

// ---------------------------------------------------------------
// Optimization rules: identify opportunities for runtime filters
// ---------------------------------------------------------------

/// Describes a runtime filter opportunity identified by the
/// optimizer.
#[derive(Debug, Clone)]
pub struct FilterOpportunity {
    /// Build-side table name.
    pub build_table: String,
    /// Probe-side table name.
    pub probe_table: String,
    /// Join key column on the build side.
    pub build_column: String,
    /// Join key column on the probe side.
    pub probe_column: String,
    /// Estimated cost/benefit of applying the filter.
    pub cost: RuntimeFilterCost,
}

/// Identify runtime filter opportunities in a join plan.
///
/// For each hash join where the build side is significantly
/// smaller than the probe side, suggests a runtime filter.
///
/// `join_pairs`: list of (build_table, probe_table, build_col,
///   probe_col, build_rows, probe_rows, build_ndv, probe_ndv)
/// `scan_cost_per_row`: base cost of reading one probe-side row
#[must_use]
pub fn identify_filter_opportunities(
    join_pairs: &[(String, String, String, String, f64, f64, f64, f64)],
    scan_cost_per_row: f64,
) -> Vec<FilterOpportunity> {
    let mut opportunities = Vec::new();

    for (
        build_table,
        probe_table,
        build_col,
        probe_col,
        build_rows,
        probe_rows,
        build_ndv,
        probe_ndv,
    ) in join_pairs
    {
        let cost = estimate_filter_cost(
            *build_rows,
            *probe_rows,
            *build_ndv,
            *probe_ndv,
            scan_cost_per_row,
        );

        if should_apply_filter(&cost, 0.5) {
            opportunities.push(FilterOpportunity {
                build_table: build_table.clone(),
                probe_table: probe_table.clone(),
                build_column: build_col.clone(),
                probe_column: probe_col.clone(),
                cost,
            });
        }
    }

    opportunities
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    // ---- BloomFilterState ----

    #[test]
    fn bloom_filter_new_empty() {
        let bf = BloomFilterState::new(1000);
        assert!(bf.is_empty());
        assert_eq!(bf.len(), 0);
    }

    #[test]
    fn bloom_filter_insert_and_query() {
        let mut bf = BloomFilterState::new(100);
        bf.insert(42);
        bf.insert(123);
        bf.insert(999);

        assert!(bf.might_contain(42));
        assert!(bf.might_contain(123));
        assert!(bf.might_contain(999));
        assert_eq!(bf.len(), 3);
    }

    #[test]
    fn bloom_filter_no_false_negatives() {
        let mut bf = BloomFilterState::new(10_000);
        let values: Vec<u64> = (0..1000).collect();
        for &v in &values {
            bf.insert(v);
        }
        for &v in &values {
            assert!(
                bf.might_contain(v),
                "bloom filter must not produce false negatives"
            );
        }
    }

    #[test]
    fn bloom_filter_reasonable_fpr() {
        let mut bf = BloomFilterState::new(1000);
        for i in 0..1000_u64 {
            bf.insert(i);
        }

        let mut false_positives = 0;
        let test_count = 10_000;
        for i in 1_000_000..1_000_000 + test_count {
            if bf.might_contain(i) {
                false_positives += 1;
            }
        }
        let observed_fpr = false_positives as f64 / test_count as f64;
        // Should be close to 1%, allow up to 5%
        assert!(observed_fpr < 0.05, "FPR too high: {observed_fpr}");
    }

    #[test]
    fn bloom_filter_estimated_fpr_empty() {
        let bf = BloomFilterState::new(100);
        assert_eq!(bf.estimated_fpr(), 0.0);
    }

    #[test]
    fn bloom_filter_estimated_fpr_populated() {
        let mut bf = BloomFilterState::new(1000);
        for i in 0..1000_u64 {
            bf.insert(i);
        }
        let fpr = bf.estimated_fpr();
        assert!(fpr > 0.0);
        assert!(fpr < 0.1);
    }

    #[test]
    fn bloom_filter_memory_bytes() {
        let bf = BloomFilterState::new(1000);
        assert!(bf.memory_bytes() > 0);
        // Reasonable size: ~1.2 KB for 1000 elements at 1% FPR
        assert!(bf.memory_bytes() < 100_000);
    }

    #[test]
    fn bloom_filter_custom_fpr() {
        let bf_loose = BloomFilterState::with_fpr(1000, 0.1);
        let bf_tight = BloomFilterState::with_fpr(1000, 0.001);
        // Tighter FPR requires more bits
        assert!(bf_tight.memory_bytes() > bf_loose.memory_bytes());
    }

    // ---- MinMaxFilterState ----

    #[test]
    fn min_max_new_empty() {
        let mm = MinMaxFilterState::new();
        assert!(mm.is_empty());
        assert!(!mm.might_contain(0));
    }

    #[test]
    fn min_max_single_value() {
        let mut mm = MinMaxFilterState::new();
        mm.insert(50);
        assert!(mm.might_contain(50));
        assert!(!mm.might_contain(49));
        assert!(!mm.might_contain(51));
    }

    #[test]
    fn min_max_range() {
        let mut mm = MinMaxFilterState::new();
        mm.insert(10);
        mm.insert(20);
        mm.insert(15);

        assert!(mm.might_contain(10));
        assert!(mm.might_contain(15));
        assert!(mm.might_contain(20));
        assert!(mm.might_contain(12));
        assert!(!mm.might_contain(9));
        assert!(!mm.might_contain(21));
    }

    #[test]
    fn min_max_negative_values() {
        let mut mm = MinMaxFilterState::new();
        mm.insert(-100);
        mm.insert(100);

        assert!(mm.might_contain(-100));
        assert!(mm.might_contain(0));
        assert!(mm.might_contain(100));
        assert!(!mm.might_contain(-101));
        assert!(!mm.might_contain(101));
    }

    #[test]
    fn min_max_default() {
        let mm = MinMaxFilterState::default();
        assert!(mm.is_empty());
    }

    // ---- InListFilterState ----

    #[test]
    fn in_list_new_empty() {
        let il = InListFilterState::new();
        assert!(il.is_empty());
        assert_eq!(il.len(), 0);
    }

    #[test]
    fn in_list_insert_and_query() {
        let mut il = InListFilterState::new();
        assert!(il.insert(1));
        assert!(il.insert(2));
        assert!(il.insert(3));

        assert!(il.contains(1));
        assert!(il.contains(2));
        assert!(il.contains(3));
        assert!(!il.contains(4));
        assert_eq!(il.len(), 3);
    }

    #[test]
    fn in_list_duplicate_values() {
        let mut il = InListFilterState::new();
        il.insert(42);
        il.insert(42);
        il.insert(42);
        assert_eq!(il.len(), 1);
    }

    #[test]
    fn in_list_overflow_to_full() {
        let mut il = InListFilterState::new();
        for i in 0..=MAX_IN_LIST_SIZE as i64 {
            il.insert(i);
        }
        assert!(il.is_full());
    }

    #[test]
    fn in_list_default() {
        let il = InListFilterState::default();
        assert!(il.is_empty());
    }

    // ---- RuntimeFilter ----

    #[test]
    fn runtime_filter_bloom_might_contain() {
        let mut bf = BloomFilterState::new(100);
        bf.insert(42);
        let filter = RuntimeFilter::BloomFilter(bf);
        assert!(filter.might_contain_hash(42));
        assert!(!filter.is_empty());
    }

    #[test]
    fn runtime_filter_min_max_might_contain() {
        let mut mm = MinMaxFilterState::new();
        mm.insert(10);
        mm.insert(20);
        let filter = RuntimeFilter::MinMaxFilter(mm);
        assert!(filter.might_contain_hash(15));
        assert!(!filter.might_contain_hash(5));
    }

    #[test]
    fn runtime_filter_in_list_might_contain() {
        let mut il = InListFilterState::new();
        il.insert(100);
        il.insert(200);
        let filter = RuntimeFilter::InListFilter(il);
        assert!(filter.might_contain_hash(100));
        assert!(!filter.might_contain_hash(300));
    }

    #[test]
    fn runtime_filter_empty_variants() {
        let bf = RuntimeFilter::BloomFilter(BloomFilterState::new(10));
        assert!(bf.is_empty());

        let mm = RuntimeFilter::MinMaxFilter(MinMaxFilterState::new());
        assert!(mm.is_empty());

        let il = RuntimeFilter::InListFilter(InListFilterState::new());
        assert!(il.is_empty());
    }

    // ---- FilterBuilder ----

    #[test]
    fn builder_auto_uses_in_list_for_small() {
        let config = FilterConfig {
            strategy: FilterStrategy::Auto,
            expected_distinct: 100,
            ..FilterConfig::default()
        };
        let mut builder = FilterBuilder::new(config);
        for i in 0..50 {
            builder.insert(i as u64, i);
        }
        let filter = builder.build();
        assert!(
            matches!(filter, RuntimeFilter::InListFilter(_)),
            "small build side should produce in-list filter"
        );
    }

    #[test]
    fn builder_auto_falls_back_to_bloom() {
        let config = FilterConfig {
            strategy: FilterStrategy::Auto,
            expected_distinct: 10_000,
            ..FilterConfig::default()
        };
        let mut builder = FilterBuilder::new(config);
        for i in 0..2000 {
            builder.insert(i as u64, i);
        }
        let filter = builder.build();
        assert!(
            matches!(filter, RuntimeFilter::BloomFilter(_)),
            "large build side should produce bloom filter"
        );
    }

    #[test]
    fn builder_bloom_only_strategy() {
        let config = FilterConfig {
            strategy: FilterStrategy::BloomOnly,
            expected_distinct: 10,
            ..FilterConfig::default()
        };
        let mut builder = FilterBuilder::new(config);
        builder.insert(1, 1);
        let filter = builder.build();
        assert!(matches!(filter, RuntimeFilter::BloomFilter(_)));
    }

    #[test]
    fn builder_min_max_strategy() {
        let config = FilterConfig {
            strategy: FilterStrategy::MinMax,
            expected_distinct: 100,
            ..FilterConfig::default()
        };
        let mut builder = FilterBuilder::new(config);
        builder.insert(10, 10);
        builder.insert(20, 20);
        let filter = builder.build();
        assert!(matches!(filter, RuntimeFilter::MinMaxFilter(_)));
    }

    #[test]
    fn builder_count() {
        let config = FilterConfig::default();
        let mut builder = FilterBuilder::new(config);
        assert_eq!(builder.count(), 0);
        builder.insert(1, 1);
        builder.insert(2, 2);
        assert_eq!(builder.count(), 2);
    }

    // ---- FilterEffectiveness ----

    #[test]
    fn effectiveness_new() {
        let eff = FilterEffectiveness::new("key".to_string());
        assert_eq!(eff.rows_tested, 0);
        assert_eq!(eff.rows_passed, 0);
        assert_eq!(eff.selectivity(), 1.0);
        assert_eq!(eff.rows_filtered(), 0);
    }

    #[test]
    fn effectiveness_records_results() {
        let mut eff = FilterEffectiveness::new("key".to_string());
        eff.record(true);
        eff.record(true);
        eff.record(false);
        eff.record(false);
        eff.record(false);

        assert_eq!(eff.rows_tested, 5);
        assert_eq!(eff.rows_passed, 2);
        assert_eq!(eff.rows_filtered(), 3);
        assert!((eff.selectivity() - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn effectiveness_all_pass() {
        let mut eff = FilterEffectiveness::new("id".to_string());
        for _ in 0..100 {
            eff.record(true);
        }
        assert_eq!(eff.selectivity(), 1.0);
        assert_eq!(eff.rows_filtered(), 0);
    }

    #[test]
    fn effectiveness_none_pass() {
        let mut eff = FilterEffectiveness::new("id".to_string());
        for _ in 0..100 {
            eff.record(false);
        }
        assert_eq!(eff.selectivity(), 0.0);
        assert_eq!(eff.rows_filtered(), 100);
    }

    // ---- Cost estimation ----

    #[test]
    fn estimate_filter_cost_selective_join() {
        // Star schema: small dimension (1K rows, 1K NDV)
        // joined to large fact table (1M rows, 100K NDV)
        let cost = estimate_filter_cost(
            1_000.0,     // build_side_rows
            1_000_000.0, // probe_side_rows
            1_000.0,     // build_side_ndv
            100_000.0,   // probe_side_ndv
            1e-6,        // scan_cost_per_row
        );

        // Selectivity: 1000/100000 = 0.01
        assert!(
            cost.estimated_selectivity < 0.02,
            "selectivity should be ~0.01"
        );
        assert!(
            cost.estimated_rows_saved > 900_000.0,
            "should save most probe rows"
        );
        assert!(cost.net_benefit > 0.0, "filter should be beneficial");
    }

    #[test]
    fn estimate_filter_cost_unselective_join() {
        // Both sides have similar NDV: filter not very selective
        let cost = estimate_filter_cost(
            100_000.0, // build_side_rows
            100_000.0, // probe_side_rows
            100_000.0, // build_side_ndv
            100_000.0, // probe_side_ndv
            1e-6,      // scan_cost_per_row
        );

        assert!(
            cost.estimated_selectivity >= 0.99,
            "selectivity should be ~1.0"
        );
        assert!(cost.estimated_rows_saved < 1_000.0);
    }

    #[test]
    fn estimate_filter_cost_zero_probe_ndv() {
        let cost = estimate_filter_cost(1_000.0, 1_000_000.0, 1_000.0, 0.0, 1e-6);
        assert_eq!(cost.estimated_selectivity, 1.0);
    }

    #[test]
    fn should_apply_filter_beneficial() {
        let cost = estimate_filter_cost(1_000.0, 1_000_000.0, 1_000.0, 100_000.0, 1e-6);
        assert!(should_apply_filter(&cost, 0.5));
    }

    #[test]
    fn should_apply_filter_not_beneficial() {
        let cost = estimate_filter_cost(100_000.0, 100_000.0, 100_000.0, 100_000.0, 1e-6);
        assert!(!should_apply_filter(&cost, 0.5));
    }

    // ---- Filter opportunity identification ----

    #[test]
    fn identify_opportunities_star_schema() {
        let pairs = vec![(
            "dim_product".to_string(),
            "fact_sales".to_string(),
            "product_id".to_string(),
            "product_id".to_string(),
            1_000.0,
            10_000_000.0,
            1_000.0,
            1_000_000.0,
        )];
        let opportunities = identify_filter_opportunities(&pairs, 1e-6);
        assert_eq!(
            opportunities.len(),
            1,
            "star schema join should produce opportunity"
        );
        assert_eq!(opportunities[0].build_table, "dim_product");
        assert_eq!(opportunities[0].probe_table, "fact_sales");
    }

    #[test]
    fn identify_opportunities_no_benefit() {
        let pairs = vec![(
            "t1".to_string(),
            "t2".to_string(),
            "id".to_string(),
            "id".to_string(),
            100_000.0,
            100_000.0,
            100_000.0,
            100_000.0,
        )];
        let opportunities = identify_filter_opportunities(&pairs, 1e-6);
        assert!(
            opportunities.is_empty(),
            "equal-sized join should not produce opportunity"
        );
    }

    #[test]
    fn identify_opportunities_multiple_joins() {
        let pairs = vec![
            (
                "dim_a".to_string(),
                "fact".to_string(),
                "a_id".to_string(),
                "a_id".to_string(),
                100.0,
                1_000_000.0,
                100.0,
                100_000.0,
            ),
            (
                "dim_b".to_string(),
                "fact".to_string(),
                "b_id".to_string(),
                "b_id".to_string(),
                500.0,
                1_000_000.0,
                500.0,
                100_000.0,
            ),
            (
                "big_a".to_string(),
                "big_b".to_string(),
                "id".to_string(),
                "id".to_string(),
                500_000.0,
                500_000.0,
                500_000.0,
                500_000.0,
            ),
        ];
        let opportunities = identify_filter_opportunities(&pairs, 1e-6);
        // First two star schema joins should produce
        // opportunities; third should not.
        assert_eq!(opportunities.len(), 2);
    }

    #[test]
    fn identify_opportunities_empty_input() {
        let opportunities = identify_filter_opportunities(&[], 1e-6);
        assert!(opportunities.is_empty());
    }

    // ---- Integration: build and apply ----

    #[test]
    fn end_to_end_bloom_filter_build_and_apply() {
        let config = FilterConfig {
            strategy: FilterStrategy::BloomOnly,
            expected_distinct: 100,
            ..FilterConfig::default()
        };
        let mut builder = FilterBuilder::new(config);

        // Build side: keys 0..100
        for i in 0..100_u64 {
            builder.insert(i, i as i64);
        }
        let filter = builder.build();

        // Probe side: test keys
        let mut effectiveness = FilterEffectiveness::new("id".to_string());
        for i in 0..200_u64 {
            let passed = filter.might_contain_hash(i);
            effectiveness.record(passed);
        }

        // All build-side keys must pass (no false negatives)
        assert!(effectiveness.rows_passed >= 100);
        // Some probe-only keys should be filtered
        assert!(effectiveness.selectivity() < 1.0);
    }

    #[test]
    fn end_to_end_in_list_filter_build_and_apply() {
        let config = FilterConfig {
            strategy: FilterStrategy::Auto,
            expected_distinct: 10,
            ..FilterConfig::default()
        };
        let mut builder = FilterBuilder::new(config);

        for i in 0..10_i64 {
            builder.insert(i as u64, i);
        }
        let filter = builder.build();
        assert!(matches!(filter, RuntimeFilter::InListFilter(_)));

        // Exact membership: no false positives
        for i in 0..10_u64 {
            assert!(filter.might_contain_hash(i));
        }
        for i in 10..20_u64 {
            assert!(!filter.might_contain_hash(i));
        }
    }

    #[test]
    fn end_to_end_min_max_filter_build_and_apply() {
        let config = FilterConfig {
            strategy: FilterStrategy::MinMax,
            expected_distinct: 100,
            ..FilterConfig::default()
        };
        let mut builder = FilterBuilder::new(config);

        for i in 100..200_i64 {
            builder.insert(i as u64, i);
        }
        let filter = builder.build();
        assert!(matches!(filter, RuntimeFilter::MinMaxFilter(_)));

        // Values in range pass
        assert!(filter.might_contain_hash(150));
        // Values outside range rejected
        assert!(!filter.might_contain_hash(50));
        assert!(!filter.might_contain_hash(250));
    }
}
