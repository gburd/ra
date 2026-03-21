# Rule: "Learned Index Structures for Query Optimization"

**Category:** experimental/ml-guided
**File:** `rules/experimental/ml-guided/learned-index-structures.rra`

## Metadata

- **ID:** `learned-index-structures`
- **Version:** "1.0.0"
- **Databases:** duckdb, singlestore
- **Tags:** learned-index, CDF, RMI, PGM, ML, index-selection, range-query
- **Authors:** "Kraska et al. 2018 - the case for learned index structures", "Ferragina & Vinciguerra 2020 - PGM index"


# Learned Index Structures for Query Optimization

## Description

Learned indexes replace traditional B-tree nodes with ML models that
predict the position of a key in sorted data. A B-tree is essentially
a model that maps keys to positions with guaranteed error bounds.
A learned index trains a model (linear regression, neural network) on
the CDF of the key distribution, achieving the same mapping with less
space and faster lookups for certain distributions.

**When to apply**: Read-heavy workloads on sorted, mostly-static data
where key distributions are learnable (numeric keys, timestamps,
sequential IDs). Learned indexes trade update performance for lookup
speed and space efficiency.

**Why it works**: A B-tree makes no assumptions about data distribution,
using O(n) space for n keys. If keys are uniformly distributed, a
single linear function predicts position perfectly. Real distributions
fall between these extremes: a small model (100-1000 parameters) can
approximate the CDF well enough that binary search over the error
bound is faster than B-tree traversal.

## Relational Algebra

```algebra
-- B-tree lookup cost:
btree_lookup(n) = log_B(n) * cache_miss_cost
  -- ~4 cache misses for 1B keys

-- Learned index lookup cost:
learned_lookup(n, model) =
  model_inference_cost  -- ~10-50ns for linear/small NN
  + binary_search(error_bound) * cache_miss_cost
  -- error_bound typically 16-256 for good models

-- Space comparison:
btree_space(n) = n * (key_size + pointer_size) / fill_factor
  -- ~16 bytes per key
learned_space(n, model) = model_parameters * sizeof(float)
  + n * key_size  -- still need sorted data
  -- Model: 1-100 KB vs B-tree internal nodes: MBs

-- PGM index:
pgm_lookup(n, epsilon) = log(n/epsilon) * cache_miss_cost
  -- epsilon = max prediction error
pgm_space(n, epsilon) = n / epsilon * model_size
```

## Implementation

```rust
use egg::{rewrite as rw, *};

struct LearnedIndexModel {
    index_type: LearnedIndexType,
    error_bound: usize,
    model_size_bytes: usize,
    inference_ns: f64,
}

enum LearnedIndexType {
    RMI { stages: usize, models_per_stage: usize },
    PGM { epsilon: usize },
    RadixSpline { num_splines: usize },
    ALEX { node_size: usize }, // Adaptive learned index
}

impl LearnedIndexModel {
    fn lookup_cost(&self, cache_miss_ns: f64) -> f64 {
        let inference = self.inference_ns;

        // Binary search within error bound
        let search_steps =
            (self.error_bound as f64).log2().ceil();
        let search_cost = search_steps * cache_miss_ns;

        inference + search_cost
    }

    fn btree_lookup_cost(
        n: usize,
        fanout: usize,
        cache_miss_ns: f64,
    ) -> f64 {
        let height = (n as f64).log(fanout as f64).ceil();
        height * cache_miss_ns
    }

    fn space_comparison(
        &self,
        n: usize,
        key_bytes: usize,
    ) -> SpaceAnalysis {
        let btree_internal = n as f64
            / 100.0 // ~100 keys per node
            * 4096.0; // 4KB per node

        let learned_overhead = self.model_size_bytes as f64;

        SpaceAnalysis {
            btree_internal_bytes: btree_internal as usize,
            learned_model_bytes: self.model_size_bytes,
            space_ratio: btree_internal
                / learned_overhead.max(1.0),
        }
    }

    fn should_use_learned(
        &self,
        n: usize,
        workload: &WorkloadProfile,
        cache_miss_ns: f64,
    ) -> bool {
        let learned = self.lookup_cost(cache_miss_ns);
        let btree = Self::btree_lookup_cost(n, 256, cache_miss_ns);

        let lookup_wins = learned < btree;

        // Learned indexes are worse for updates
        let update_acceptable =
            workload.write_fraction < 0.1; // <10% writes

        lookup_wins && update_acceptable
    }

    fn range_query_cost(
        &self,
        start_key: f64,
        end_key: f64,
        n: usize,
        selectivity: f64,
    ) -> f64 {
        // Find start position with learned lookup
        let start_cost = self.lookup_cost(100.0); // 100ns cache miss

        // Sequential scan from start to end
        let result_tuples = n as f64 * selectivity;
        let scan_cost = result_tuples * 10.0; // 10ns per tuple

        start_cost + scan_cost
    }
}

struct LearnedIndexAdvisor;

impl LearnedIndexAdvisor {
    fn recommend_index_type(
        &self,
        key_distribution: &Distribution,
        data_size: usize,
        workload: &WorkloadProfile,
    ) -> IndexRecommendation {
        match key_distribution {
            Distribution::Uniform => {
                // Perfect for learned: single linear model
                IndexRecommendation::LearnedIndex {
                    index_type: LearnedIndexType::RMI {
                        stages: 1,
                        models_per_stage: 1,
                    },
                    expected_error: 1,
                }
            }
            Distribution::Sequential => {
                // Monotonic: linear model with ~0 error
                IndexRecommendation::LearnedIndex {
                    index_type: LearnedIndexType::RadixSpline {
                        num_splines: 10,
                    },
                    expected_error: 2,
                }
            }
            Distribution::Skewed { .. } => {
                // PGM adapts to varying density
                IndexRecommendation::LearnedIndex {
                    index_type: LearnedIndexType::PGM {
                        epsilon: 64,
                    },
                    expected_error: 64,
                }
            }
            Distribution::Adversarial => {
                // Fall back to B-tree for unpredictable data
                IndexRecommendation::BTree
            }
        }
    }

    fn evaluate_distribution_learnability(
        &self,
        sample: &[f64],
    ) -> f64 {
        // Fit linear regression to CDF, measure R^2
        let n = sample.len() as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        for (i, &x) in sample.iter().enumerate() {
            let y = i as f64 / n;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let denom = n * sum_x2 - sum_x * sum_x;
        if denom.abs() < 1e-10 {
            return 0.0;
        }

        let slope = (n * sum_xy - sum_x * sum_y) / denom;
        let intercept = (sum_y - slope * sum_x) / n;

        // R^2 as learnability score
        let ss_res: f64 = sample
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let predicted = slope * x + intercept;
                let actual = i as f64 / n;
                (actual - predicted).powi(2)
            })
            .sum();

        let mean_y = sum_y / n;
        let ss_tot: f64 = (0..sample.len())
            .map(|i| {
                let y = i as f64 / n;
                (y - mean_y).powi(2)
            })
            .sum();

        if ss_tot > 0.0 {
            1.0 - ss_res / ss_tot
        } else {
            1.0
        }
    }
}
```

## Preconditions

```rust
fn applicable(
    table: &TableStats,
    column: &Column,
    workload: &WorkloadProfile,
) -> bool {
    // Learned indexes best for:
    // 1. Read-heavy workloads
    // 2. Sorted/sortable numeric keys
    // 3. Large tables where B-tree overhead matters
    workload.write_fraction < 0.2
        && column.is_numeric_or_timestamp()
        && table.num_tuples() > 100_000
}
```

**Restrictions:**
- Write performance: updates may require retraining or local adjustment
- ALEX (adaptive) handles updates but with higher lookup cost
- String keys require encoding to numeric domain first
- Worst-case guarantee: error bound must be maintained
- Multi-dimensional learned indexes still research-stage
- Production adoption limited (SageDB prototype, some columnar stores)

## Cost Model

```rust
fn learned_vs_btree_breakeven(
    model_inference_ns: f64,
    error_bound: usize,
    btree_fanout: usize,
    cache_miss_ns: f64,
) -> usize {
    // Find n where learned < B-tree
    // learned: inference + log2(error) * cache_miss
    // btree: log_B(n) * cache_miss
    let learned = model_inference_ns
        + (error_bound as f64).log2() * cache_miss_ns;
    let per_level = cache_miss_ns;

    // Solve: learned < levels * cache_miss
    let min_levels = (learned / per_level).ceil() as u32;
    (btree_fanout as f64).powi(min_levels as i32) as usize
}
```

**Typical results:**
- Uniform keys: learned 2-4x faster than B-tree, 100x less space
- Timestamp keys: learned 1.5-3x faster (nearly linear CDF)
- Random keys: learned ~= B-tree (CDF not compressible)
- High-update keys: B-tree wins (learned needs retrain)

## Test Cases

### Positive: Timestamp-keyed time series

```sql
-- events: 1B rows, key = timestamp (monotonically increasing)
-- CDF is nearly linear: single linear model, error <16
-- Learned lookup: 10ns inference + 4 * 5ns binary search = 30ns
-- B-tree lookup: 4 levels * 100ns cache miss = 400ns
-- 13x faster lookup with learned index
SELECT * FROM events WHERE ts = '2025-03-18 14:30:00';
```

### Positive: Uniformly distributed numeric key

```sql
-- users: 100M rows, key = uniformly distributed user_id
-- RMI with 2 stages: error bound ~32
-- Learned: 20ns + 5 * 5ns = 45ns
-- B-tree: 3 * 100ns = 300ns
-- 6x faster, 1000x less index memory
SELECT * FROM users WHERE user_id = 12345678;
```

### Negative: Highly skewed with frequent updates

```sql
-- inventory: 1M rows, frequent price updates
-- Key distribution: heavily skewed (90% in popular categories)
-- Model error high due to skew, and retraining on each update
-- B-tree: 3 * 100ns = 300ns, instant updates
-- Learned: 50ns + 8 * 100ns = 850ns + retrain overhead
SELECT * FROM inventory WHERE sku = 'ABC123';
```

## References

**Learned indexes:**
- Kraska et al., "The Case for Learned Index Structures", SIGMOD 2018
  - Original RMI proposal, 1.5-3x speedup over B-trees
- Ferragina & Vinciguerra, "The PGM-index: A Fully-Dynamic Compressed Learned Index with Provable Worst-Case Bounds", VLDB 2020
  - Optimal-space learned index with guaranteed error

**Updatable learned indexes:**
- Ding et al., "ALEX: An Updatable Adaptive Learned Index", SIGMOD 2020
  - Gapped arrays for in-place updates
- Galakatos et al., "FITing-Tree: A Data-aware Index Structure", SIGMOD 2019

**Practical evaluation:**
- Marcus et al., "Benchmarking Learned Indexes", VLDB 2020
  - Comprehensive comparison across distributions and workloads
- Wongkham et al., "Are Updatable Learned Indexes Ready?", VLDB 2022
