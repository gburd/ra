# Changelog

## Phase 19: Distributed Query Optimization

### Added

**Network Cost Modeling** (`ra-hardware`, `ra-engine`)
- `NetworkTopology` struct modeling cluster connectivity with bandwidth,
  latency, and cloud billing costs per link
- `LinkType` enum: IntraRack, IntraDatacenter, CrossDatacenter,
  CrossRegion, Internet with realistic default parameters
- 5 predefined topology profiles: single datacenter cluster,
  multi-datacenter, cloud federation (AWS+GCP+Azure), edge+cloud,
  and data warehouse (Snowflake-style)
- `NetworkCostModel` integrating topology with table placement for
  transfer cost estimation
- `DistributionStrategy` with Broadcast, Shuffle, and CoLocated options
- `recommend_join_strategy()` for automatic broadcast vs shuffle selection
- 118 unit tests

**Distribution Strategies** (`ra-core`, `ra-engine`)
- `DataDistribution` modeling for hash, range, broadcast, and replicated
  partitioning
- `DistributedOptimizer` for rewriting plans with distribution-aware
  operators
- 34 optimization rules for join distribution, filter pushdown, partition
  pruning, locality awareness, and skew handling
- 84 unit tests

**Distributed Aggregation** (`ra-core`, `ra-stats`)
- Two-phase aggregation: local pre-aggregation + global merge for
  decomposable aggregates (SUM, COUNT, MIN, MAX)
- Three-phase aggregation: adds redistribution phase for high-cardinality
  GROUP BY
- `SkewDetector` identifying skewed key distributions using histograms
  and coefficient of variation
- 25 optimization rules for aggregation pushdown, phase selection,
  and skew-aware strategies
- 170 unit tests

**Federated Queries** (`ra-core`, `ra-engine`)
- `FederatedCostModel` estimating cost of pushing operations to remote
  databases vs pulling data locally
- `FederatedOptimizer` rewriting plans for multi-database execution
- Capability-aware optimization respecting per-database SQL support
- 24 optimization rules for federated pushdown
- 89 unit tests

### Metrics

| Metric              | Value     |
|---------------------|-----------|
| Lines of Rust       | 12,492    |
| New tests           | 461+      |
| New .rra rules      | 83        |
| Documentation pages | 2 new     |
| Crates modified     | 4         |
| Breaking changes    | None      |
