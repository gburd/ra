# RFC Index

This index tracks all RFCs in the RA optimizer project by status. See [README.md](README.md) for the RFC process and [TEMPLATE.md](TEMPLATE.md) for the RFC template.

## Underway (In Development)

| RFC | Title | Author | Date |
|-----|-------|--------|------|
| [0002](text/0002-pgrx-extension.md) | pgrx PostgreSQL Extension | RA Contributors | 2026-03-20 |
| [0011](text/0011-ascii-movie-recording.md) | ASCII Movie Recording (TUI) | RA Contributors | 2026-03-20 |

## Accepted (Approved, Not Yet Implemented)

| RFC | Title | Author | Date |
|-----|-------|--------|------|
| [0003](text/0003-plan-advice-integration.md) | Plan Advice Integration (pg_plan_advice) | RA Contributors | 2026-03-20 |
| [0012](text/0012-monitoring-system.md) | Monitoring & Advisory System | RA Contributors | 2026-03-20 |
| [0013](text/0013-query-regression-detection.md) | Query Regression Detection | RA Contributors | 2026-03-20 |
| [0014](text/0014-index-recommendations.md) | Automatic Index Recommendations | RA Contributors | 2026-03-20 |
| [0015](text/0015-configuration-auto-tuning.md) | Configuration Auto-Tuning | RA Contributors | 2026-03-20 |
| [0019](text/0019-partition-pruning.md) | Partition Pruning Optimization | RA Contributors | 2026-03-20 |

## Under Review

| RFC | Title | Author | Date |
|-----|-------|--------|------|
| [0022](text/0022-incremental-view-maintenance.md) | Incremental View Maintenance | RA Contributors | 2026-03-21 |
| [0023](text/0023-adaptive-query-execution.md) | Adaptive Query Execution | RA Contributors | 2026-03-21 |
| [0024](text/0024-query-result-caching.md) | Query Result Caching | RA Contributors | 2026-03-21 |

## Implemented

| RFC | Title | Author | Date | Implementation |
|-----|-------|--------|------|----------------|
| [0001](text/0001-row-pattern-recognition.md) | Row Pattern Recognition | RA Contributors | 2026-03-19 | Commit 2763fda |
| [0004](text/0004-formal-preconditions.md) | Formal Precondition System | RA Contributors | 2026-03-20 | Core feature |
| [0005](text/0005-hardware-aware-optimization.md) | Hardware-Aware Optimization | RA Contributors | 2026-03-20 | Core feature |
| [0006](text/0006-distributed-optimization.md) | Distributed Query Optimization | RA Contributors | 2026-03-20 | Core feature |
| [0007](text/0007-statistics-timeline.md) | Statistics Timeline System | RA Contributors | 2026-03-20 | Core feature |
| [0008](text/0008-dialect-translation.md) | Multi-Database Dialect Translation | RA Contributors | 2026-03-20 | Core feature |
| [0009](text/0009-wasm-integration.md) | WASM Database Integration | RA Contributors | 2026-03-20 | Core feature |
| [0010](text/0010-web-ui.md) | Web-Based Query Comparison | RA Contributors | 2026-03-20 | Web UI |
| [0016](text/0016-hardware-adaptive-test-expectations.md) | Hardware-Adaptive Test Expectations | RA Contributors | 2026-03-20 | Test framework |
| [0017](text/0017-large-join-graph-fallback.md) | Large Join Graph Optimization Fallback | RA Contributors | 2026-03-20 | Join optimizer |
| [0018](text/0018-bitmap-index-scan.md) | Bitmap Index Scan | RA Contributors | 2026-03-20 | Physical operators |
| [0020](text/0020-parallel-query-execution.md) | Parallel Query Execution | RA Contributors | 2026-03-20 | Execution engine |
| [0021](text/0021-automatic-index-advisor.md) | Automatic Index Advisor | RA Contributors | 2026-03-20 | Advisory system |
| [0033](text/0033-columnar-format-optimization.md) | Columnar Format Optimization | RA Contributors | 2026-03-20 | Storage layer |

## Rejected

| RFC | Title | Author | Date | Reason |
|-----|-------|--------|------|--------|
| (none) | | | | |

## RFC Numbering Gaps

The following RFC numbers are missing from the sequence and represent features that were either:
- Never formally proposed
- Merged into other RFCs
- Reserved for future use

Missing numbers: 0025-0032 (reserved for future features)

## Statistics

- **Total RFCs**: 24
- **Implemented**: 14 (58%)
- **Underway**: 2 (8%)
- **Accepted**: 6 (25%)
- **Under Review**: 3 (13%)
- **Rejected**: 0 (0%)

## Recent Activity

### Last Updated: 2026-03-21

Recent changes:
- Established comprehensive RFC system
- Documented all existing features as retroactive RFCs
- Created prospective RFCs for planned features
- Set up RFC lifecycle and archival process