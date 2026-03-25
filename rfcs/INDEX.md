# RFC Index

This index tracks all RFCs in the RA optimizer project by status. See [README.md](README.md) for the RFC process and [TEMPLATE.md](TEMPLATE.md) for the RFC template.

## Implemented

| RFC | Title | Date | Implementation |
|-----|-------|------|----------------|
| [0001](text/0001-row-pattern-recognition.md) | Row Pattern Recognition | 2026-03-19 | Commit 2763fda |
| [0004](text/0004-formal-preconditions.md) | Formal Precondition System | 2026-03-20 | Core feature |
| [0005](text/0005-hardware-aware-optimization.md) | Hardware-Aware Optimization | 2026-03-20 | Core feature |
| [0006](text/0006-distributed-optimization.md) | Distributed Query Optimization | 2026-03-20 | Core feature |
| [0007](text/0007-statistics-timeline.md) | Statistics Timeline System | 2026-03-20 | Core feature |
| [0008](text/0008-dialect-translation.md) | Multi-Database Dialect Translation | 2026-03-20 | Core feature |
| [0009](text/0009-wasm-integration.md) | WASM Database Integration | 2026-03-20 | Core feature |
| [0010](text/0010-web-ui.md) | Web-Based Query Comparison UI | 2026-03-20 | Web UI |
| [0016](text/0016-hardware-adaptive-test-expectations.md) | Hardware-Adaptive Test Expectations | 2026-03-20 | Test framework |
| [0017](text/0017-large-join-graph-fallback.md) | Large Join Graph Optimization Fallback | 2026-03-20 | Join optimizer |
| [0018](text/0018-bitmap-index-scan.md) | Bitmap Index Scan | 2026-03-20 | Physical operators |
| [0020](text/0020-parallel-query-execution.md) | Parallel Query Execution | 2026-03-20 | Execution engine |
| [0021](text/0021-automatic-index-advisor.md) | Automatic Index Advisor | 2026-03-20 | Advisory system |
| [0033](text/0033-columnar-format-optimization.md) | Columnar Format Optimization | 2026-03-20 | Storage layer |
| [0052](_accepted/0052-progressive-reoptimization.md) | Progressive Re-Optimization (Plan Stitch) | 2026-03-22 | Commit 3246500a |
| [0058](_accepted/0058-rule-complexity-prioritization.md) | Rule Complexity Prioritization | 2026-03-24 | Commit 848aadaf |
| [0060](_accepted/0060-genetic-fingerprinting.md) | Genetic Fingerprinting for Query Plan Cache | 2026-03-23 | Plan cache system |
| [0062](_accepted/0062-documentdb-query-optimization.md) | DocumentDB / MongoDB Query Optimization | 2026-03-25 | DocumentDB optimizer |
| [0066](_accepted/0066-advanced-index-aware-planning.md) | Advanced Index-Aware Planning (BRIN) | 2026-03-25 | BRIN index advisor |
| [0068](_accepted/0068-hardware-calibrated-cost-model.md) | Hardware-Calibrated Cost Model | 2026-03-25 | Hardware calibration |
| [0078](_accepted/0078-remove-bayesian-pruning.md) | Remove Bayesian Adaptive Search Space Pruning | 2026-03-25 | Commit 32f9902f |

## Underway (In Development)

| RFC | Title | Date |
|-----|-------|------|
| [0002](text/0002-pgrx-extension.md) | pgrx PostgreSQL Extension | 2026-03-20 |
| [0011](text/0011-ascii-movie-recording.md) | ASCII Movie Recording (TUI) | 2026-03-20 |

## Accepted (Approved, Not Yet Implemented)

| RFC | Title | Date |
|-----|-------|------|
| [0003](text/0003-plan-advice-integration.md) | pg_plan_advice Integration | 2026-03-20 |
| [0012](text/0012-monitoring-system.md) | Monitoring and Advisory System | 2026-03-20 |
| [0019](text/0019-partition-pruning.md) | Partition Pruning and Partition-Wise Operations | 2026-03-20 |
| [0025](text/0025-physical-property-tracking.md) | Physical Property Tracking Framework | 2026-03-21 |
| [0026](text/0026-adaptive-cost-calibration.md) | Adaptive Cost Model Calibration | 2026-03-21 |
| [0027](text/0027-runtime-filters.md) | Runtime Filters and Sideways Information Passing | 2026-03-21 |
| [0028](text/0028-incremental-sort-reordering.md) | Incremental Sort and Key Reordering | 2026-03-21 |
| [0029](text/0029-self-join-elimination.md) | Self-Join Elimination and Outer-to-Inner Conversion | 2026-03-21 |
| [0030](text/0030-cardinality-estimation-enhancement.md) | Cardinality Estimation Enhancement | 2026-03-21 |
| [0031](text/0031-topn-sort-empty-propagation.md) | Top-N Sort and Empty Result Propagation | 2026-03-21 |
| [0032](text/0032-memoize-parameterized-scans.md) | Memoize for Parameterized Scans | 2026-03-21 |
| [0034](text/0034-expression-simplification.md) | Expression Simplification Extensions | 2026-03-21 |

## Under Review

| RFC | Title | Date |
|-----|-------|------|
| [0013](text/0013-query-regression-detection.md) | Query Regression Detection | 2026-03-20 |
| [0014](text/0014-index-recommendations.md) | Automatic Index Recommendations | 2026-03-20 |
| [0015](text/0015-configuration-auto-tuning.md) | Configuration Auto-Tuning | 2026-03-20 |
| [0022](text/0022-incremental-view-maintenance.md) | Incremental View Maintenance | 2026-03-21 |
| [0023](text/0023-adaptive-query-execution.md) | Adaptive Query Execution | 2026-03-21 |
| [0024](text/0024-query-result-caching.md) | Query Result Caching | 2026-03-21 |

## Proposed (Awaiting Review)

| RFC | Title | Date | Source |
|-----|-------|------|--------|
| [0035](text/0035-genetic-query-optimizer.md) | Genetic Query Optimizer for Large Join Graphs | 2026-03-21 | CMU research |
| [0036](text/0036-multi-query-optimization.md) | Multi-Query Optimization | 2026-03-21 | CMU research |
| [0037](text/0037-interesting-orders-framework.md) | Interesting Orders Framework | 2026-03-21 | CMU research |
| [0038](text/0038-loose-index-scan.md) | Loose Index Scan (Skip Scan) | 2026-03-21 | CMU research |
| [0039](text/0039-operator-class-aware-indexing.md) | Operator Class Aware Index Selection | 2026-03-21 | CMU research |
| [0040](text/0040-predicate-inference.md) | Predicate Inference and Transitivity Closure | 2026-03-21 | CMU research |
| [0041](text/0041-query-compilation.md) | Query Compilation and Code Generation | 2026-03-21 | CMU research |
| [0042](text/0042-magic-sets-recursive-queries.md) | Magic Sets for Recursive Queries | 2026-03-22 | Gap analysis |
| [0043](text/0043-groupjoin-eager-aggregation.md) | GroupJoin - Eager Aggregation Before Join | 2026-03-22 | Gap analysis |
| [0044](text/0044-sideways-information-passing.md) | Sideways Information Passing (SIP) | 2026-03-22 | Gap analysis |
| [0045](text/0045-runtime-filter-pushdown.md) | Runtime Filter Pushdown with Bloom Filters | 2026-03-22 | Gap analysis |
| [0047](text/0047-semi-join-reduction.md) | Semi-Join Reduction | 2026-03-22 | Gap analysis |
| [0048](text/0048-distinct-aggregation-rewrite.md) | Distinct Aggregation Rewrite | 2026-03-22 | Gap analysis |
| [0049](text/0049-partial-aggregation.md) | Partial Aggregation (Two-Phase) | 2026-03-22 | Gap analysis |
| [0050](text/0050-decorrelation-improvements.md) | Decorrelation Improvements | 2026-03-22 | Gap analysis |
| [0051](text/0051-materialized-view-matching.md) | Materialized View Matching and Rewriting | 2026-03-22 | High-priority optimization |
| [0053](text/0053-stored-procedure-dialect-support.md) | Stored Procedure Dialect Support | 2026-03-24 | Phase 2 extended roadmap |
| [0054](text/0054-streaming-plan-adjustments.md) | Streaming Plan Adjustments for Pre-compiled Plans | 2026-03-24 | Phase 2 extended roadmap |
| [0055](text/0055-rdbms-specific-type-support.md) | RDBMS-Specific Type Support | 2026-03-24 | Phase 2 extended roadmap |
| [0056](text/0056-postgresql-type-optimizations.md) | PostgreSQL Type-Specific Optimizations | 2026-03-24 | Phase 2 extended roadmap |
| [0057](text/0057-cross-database-type-adaptation.md) | Cross-Database Type Storage Adaptation | 2026-03-24 | Phase 2 extended roadmap |
| [0058](text/0058-opentracing-instrumentation.md) | OpenTracing Instrumentation for Query Planner | 2026-03-23 | Observability |
| [0059](text/0059-statistics-based-plan-cache-invalidation.md) | Statistics-Based Plan Cache Invalidation | 2026-03-24 | Phase 5 differential dataflow |
| [0061](text/0061-postgresql-extension-aware-optimization.md) | PostgreSQL Extension-Aware Optimization | 2026-03-24 | Phase 5 PostgreSQL extensions |
| [0063](text/0063-spatial-query-optimization.md) | Spatial Query Optimization | 2026-03-25 | Extension research |
| [0064](text/0064-vector-similarity-search-optimization.md) | Vector Similarity Search Optimization | 2026-03-25 | Extension research |
| [0065](text/0065-time-series-query-optimization.md) | Time-Series Query Optimization | 2026-03-25 | Extension research |
| [0067](text/0067-full-text-search-optimization.md) | Full-Text Search Optimization | 2026-03-25 | Extension research |
| [0069](text/0069-execution-feedback-loop.md) | Execution Feedback Loop | 2026-03-25 | Adaptive optimization |
| [0070](text/0070-memory-pressure-aware-joins.md) | Memory-Pressure-Aware Joins | 2026-03-25 | Adaptive optimization |
| [0071](text/0071-workload-classification.md) | Workload Classification | 2026-03-25 | Adaptive optimization |
| [0072](text/0072-adaptive-parallelism.md) | Adaptive Parallelism | 2026-03-25 | Adaptive optimization |
| [0073](text/0073-buffer-pool-aware-planning.md) | Buffer Pool-Aware Planning | 2026-03-25 | Adaptive optimization |
| [0074](text/0074-resource-aware-scheduling.md) | Resource-Aware Scheduling | 2026-03-25 | Adaptive optimization |
| [0075](text/0075-multi-objective-cost-model.md) | Multi-Objective Cost Model | 2026-03-25 | Adaptive optimization |
| [0076](text/0076-adaptive-mid-query-reoptimization.md) | Adaptive Mid-Query Re-Optimization | 2026-03-25 | Adaptive optimization |
| [0077](text/0077-numa-aware-execution.md) | NUMA-Aware Execution | 2026-03-25 | Adaptive optimization |

## Rejected

| RFC | Title | Date | Reason |
|-----|-------|------|--------|
| [0059](_rejected/0059-bayesian-pruning.md) | Bayesian Adaptive Search Space Pruning (v1) | 2026-03-23 | Not integrated, learning failed (0% cross-query learning), fingerprint collisions |

## Statistics

- **Total RFCs**: 79
- **Implemented**: 21 (27%)
- **Underway**: 2 (3%)
- **Accepted**: 12 (15%)
- **Under Review**: 6 (8%)
- **Proposed**: 37 (47%)
- **Rejected**: 1 (1%)

## Last Updated: 2026-03-25
