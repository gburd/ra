# ra-stats

Statistics abstraction system for query optimization with accuracy and staleness modeling.

## Overview

This crate provides a comprehensive framework for modeling database statistics systems, inspired by PostgreSQL, MySQL, and DuckDB. It tracks not just the statistics themselves, but also their quality, staleness, and the cost of gathering them.

## Features

### Statistics Types (20+ types cataloged)

#### Table-level
- Row count, page count, average row size
- Table size in bytes
- Live/dead tuples (MVCC systems)
- Last analyzed timestamp

#### Column-level
- NDV (Number of Distinct Values / cardinality)
- Null fraction
- Average column width
- Most Common Values (MCV) with frequencies
- Histograms (equi-width, equi-depth, end-biased, T-Digest)
- Correlation with physical row order

#### Index
- Clustering factor
- Leaf pages and tree height
- Average leaf density
- Distinct keys

#### Correlation
- Functional dependencies
- Multi-column NDV for join estimation
- Pearson correlation coefficients

#### Sketches
- HyperLogLog for NDV estimation
- Count-Min Sketch for frequency estimation
- Bloom filters for membership testing

### Accuracy & Staleness Modeling

Tracks the reliability of statistics through:
- **Source tracking**: Exact count, sampled, histogram, ML model, derived, or default
- **Staleness classification**: Fresh, slightly stale, moderately stale, very stale
- **Confidence scoring**: 0.0 to 1.0 based on source and age
- **Refresh thresholds**: Age, modifications, staleness level, confidence

### Gathering Cost Model

Estimates the cost of collecting statistics:
- **CPU time**: Per-row processing cost
- **I/O operations**: Pages read from disk
- **Memory usage**: Working set size
- **Query interference**: Impact on concurrent queries (0.0 to 1.0)
- **Wall-clock time**: Total elapsed time

Supports multiple gathering methods:
- Full scan
- Block-level sampling
- Row-level sampling
- Index-only scan
- Incremental updates
- Sketch-based (online algorithms)

### Configuration Profiles

Six pre-configured profiles for different workload patterns:

1. **RealTime**: Aggressive gathering for OLTP (high accuracy, frequent updates)
2. **Standard**: Balanced approach for mixed workloads
3. **Lazy**: Minimal overhead for read-mostly workloads
4. **Stale**: Sketch-based approximations for append-only data
5. **Analytical**: Comprehensive statistics for OLAP workloads
6. **Streaming**: Incremental updates for continuous data ingestion

## Examples

### Track statistics staleness

```rust
use ra_stats::accuracy::{StatisticsState, StatisticsSource, Staleness};

let mut state = StatisticsState::new(StatisticsSource::ExactCount, 1_000_000);
assert_eq!(state.staleness(), Staleness::Fresh);

// Simulate modifications
state.record_modifications(100_000);
assert_eq!(state.staleness(), Staleness::ModeratelyStale);
```

### Estimate gathering cost

```rust
use ra_stats::gathering_cost::{CostEstimator, GatheringMethod};

let estimator = CostEstimator::default();
let cost = estimator.estimate(
    GatheringMethod::BlockSample { sample_rate: 10 },
    1_000_000,  // total rows
    10_000,     // total pages
);
println!("CPU: {}ms, I/O: {} ops", cost.cpu_time_ms, cost.io_operations);
```

### Choose a profile

```rust
use ra_stats::profiles::StatisticsProfile;

// For OLTP workloads
let profile = StatisticsProfile::real_time();

// For data warehouses
let profile = StatisticsProfile::analytical();

// For streaming systems
let profile = StatisticsProfile::streaming();
```

### Automatic profile selection

```rust
use ra_stats::profiles::ProfileSelector;

let selector = ProfileSelector {
    writes_per_second: 100.0,
    reads_per_second: 900.0,
    table_size: 10_000_000,
    latency_sensitivity: 0.5,
};
let profile = selector.recommend();
```

## Architecture

```
types.rs          - Statistics type definitions (20+ types)
accuracy.rs       - Staleness and confidence modeling
gathering_cost.rs - Cost estimation for statistics collection
profiles.rs       - Pre-configured profiles for different workloads
lib.rs           - Public API and documentation
```

## Design Philosophy

The statistics system models real-world database behavior where:
- Statistics become stale as data changes
- Gathering has measurable cost and interference with queries
- Different workloads need different accuracy/cost tradeoffs
- Statistics quality directly affects query plan quality

## References

This implementation draws inspiration from:
- **PostgreSQL**: ANALYZE, pg_statistic, extended statistics
- **MySQL**: InnoDB persistent statistics, histograms
- **DuckDB**: HyperLogLog sketches, perfect hash aggregates
- **Apache Calcite**: RelMetadataQuery, statistics providers
